//! MCP stdio server, public tool metadata, and shared tool handlers.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    env, fs,
    io::{self, BufRead, Write},
    path::{Path, PathBuf},
    sync::{Mutex, atomic::AtomicBool},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use token_shrinker_compress::compress_terminal;
use token_shrinker_daemon::{
    CapabilityRegistry, HealthState, RpcHandler, ServiceError, build_baseline_context,
};
use token_shrinker_exec::{ExecutionEngine, ExecutionPolicy, ExecutionRequest, TerminationReason};
use token_shrinker_memory::{MemoryScope, MemoryStore, NewMemory};
use token_shrinker_output::{FormatRequest, OutputMode, ProfileConfig, resolve};
use token_shrinker_protocol::RpcRequest;
use token_shrinker_repo::RepositoryProvider;
use token_shrinker_router::{RouterConfig, route};
use token_shrinker_telemetry::{
    EventStatus, RequestEvent, TelemetryStore, TokenDirection, TokenEvent,
};
use token_shrinker_types::{ProtocolVersion, RequestId, RouteMode, TokenBudget};

/// Stable MCP protocol revision implemented by the stdio transport.
pub const MCP_PROTOCOL_VERSION: &str = "2025-11-25";
/// Previous MCP revision retained for clients that have not adopted the latest stable revision.
pub const MCP_PROTOCOL_VERSION_COMPAT: &str = "2025-06-18";
/// Public JSON schema generation.
pub const PUBLIC_SCHEMA_VERSION: u16 = 1;

/// Source-of-truth metadata for one advertised MCP tool.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDefinition {
    /// Stable MCP tool name.
    pub name: &'static str,
    /// Human-readable title.
    pub title: &'static str,
    /// Contract description.
    pub description: &'static str,
    /// JSON Schema 2020-12 input object.
    pub input_schema: Value,
    /// JSON Schema 2020-12 output object.
    pub output_schema: Value,
    /// Safety and mutability hints.
    pub annotations: ToolAnnotations,
}

/// MCP tool behavior annotations.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_excessive_bools)]
pub struct ToolAnnotations {
    /// Whether the tool only reads state.
    pub read_only_hint: bool,
    /// Whether the operation can destroy data.
    pub destructive_hint: bool,
    /// Whether equal calls have equal effects.
    pub idempotent_hint: bool,
    /// Whether the tool communicates outside the local machine.
    pub open_world_hint: bool,
}

const READ_ONLY: ToolAnnotations = ToolAnnotations {
    read_only_hint: true,
    destructive_hint: false,
    idempotent_hint: true,
    open_world_hint: false,
};

/// Returns all tools in the required stable order.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn tool_definitions() -> Vec<ToolDefinition> {
    let envelope = json!({
        "type": "object",
        "required": ["protocolVersion", "requestId", "warnings", "data"],
        "properties": {
            "protocolVersion": {"type": "string"},
            "requestId": {"type": "string"},
            "warnings": {"type": "array", "items": {"type": "string"}},
            "data": {"type": "object"}
        }
    });
    let object = |properties: Value, required: &[&str]| {
        json!({"type":"object", "additionalProperties":false,
              "properties":properties, "required":required})
    };
    vec![
        tool(
            "token_shrinker_capabilities",
            "Capabilities",
            "List versions, limits, providers, and degradation reasons.",
            object(json!({}), &[]),
            envelope.clone(),
            READ_ONLY,
        ),
        tool(
            "token_shrinker_route",
            "Route",
            "Select and explain FAST, BUILD, or DEEP using deterministic rules.",
            object(
                json!({
                    "explicitMode":{"enum":["FAST","BUILD","DEEP"]},
                    "operations":{"type":"array","items":{"enum":["lookup","command","edit","debug","architecture","investigation"]}},
                    "scope":{"enum":["named","multi_file","repository"]},
                    "budgetOverride":{"type":"integer","minimum":1}
                }),
                &[],
            ),
            envelope.clone(),
            READ_ONLY,
        ),
        tool(
            "token_shrinker_build_context",
            "Build context",
            "Build a provenance-rich native repository context bundle.",
            object(
                json!({
                    "root":{"type":"string"}, "goal":{"type":"string"},
                    "budget":{"type":"integer","minimum":1}
                }),
                &["root", "goal", "budget"],
            ),
            envelope.clone(),
            READ_ONLY,
        ),
        tool(
            "token_shrinker_fetch_source",
            "Fetch source",
            "Fetch a previously cited repository source by addressable handle.",
            object(
                json!({
                    "root":{"type":"string"}, "sourceId":{"type":"string"}
                }),
                &["root", "sourceId"],
            ),
            envelope.clone(),
            READ_ONLY,
        ),
        tool(
            "token_shrinker_search_memory",
            "Search memory",
            "Search isolated local memory without external transport.",
            memory_search_schema(),
            envelope.clone(),
            READ_ONLY,
        ),
        tool(
            "token_shrinker_remember",
            "Remember",
            "Store an explicitly supplied local memory record.",
            memory_write_schema(),
            envelope.clone(),
            ToolAnnotations {
                read_only_hint: false,
                destructive_hint: false,
                idempotent_hint: true,
                open_world_hint: false,
            },
        ),
        tool(
            "token_shrinker_execute",
            "Execute",
            "Run an explicitly approved argument-array command under bounded policy.",
            execution_schema(),
            envelope.clone(),
            ToolAnnotations {
                read_only_hint: false,
                destructive_hint: true,
                idempotent_hint: false,
                open_world_hint: false,
            },
        ),
        tool(
            "token_shrinker_stats",
            "Stats",
            "Return local content-free token savings aggregates.",
            object(json!({}), &[]),
            envelope.clone(),
            READ_ONLY,
        ),
        tool(
            "token_shrinker_format_final",
            "Format final",
            "Resolve the selected final-response profile without changing machine-readable payloads.",
            format_schema(),
            envelope,
            READ_ONLY,
        ),
    ]
}

fn tool(
    name: &'static str,
    title: &'static str,
    description: &'static str,
    input_schema: Value,
    output_schema: Value,
    annotations: ToolAnnotations,
) -> ToolDefinition {
    ToolDefinition {
        name,
        title,
        description,
        input_schema,
        output_schema,
        annotations,
    }
}

fn memory_search_schema() -> Value {
    json!({"type":"object","additionalProperties":false,"required":["query"],"properties":{"query":{"type":"string"},"repository":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":100}}})
}
fn memory_write_schema() -> Value {
    json!({"type":"object","additionalProperties":false,"required":["id","content"],"properties":{"id":{"type":"string"},"content":{"type":"string"},"repository":{"type":"string"},"expiresAtUnixMs":{"type":"integer"}}})
}
fn execution_schema() -> Value {
    json!({"type":"object","additionalProperties":false,"required":["program","workingDirectory"],"properties":{"program":{"type":"string"},"args":{"type":"array","items":{"type":"string"}},"workingDirectory":{"type":"string"},"environment":{"type":"object","additionalProperties":{"type":"string"}},"timeoutMs":{"type":"integer","minimum":1,"maximum":300_000}}})
}
fn format_schema() -> Value {
    json!({"type":"object","additionalProperties":false,"required":["text"],"properties":{"text":{"type":"string"},"mode":{"enum":["lite","full","ultra","wenyan-lite","wenyan-full","wenyan-ultra","off"]},"agent":{"type":"string"},"tool":{"type":"string"}}})
}

/// Shared implementation used by MCP, one-shot CLI, and daemon IPC.
pub struct PublicHandler {
    registry: CapabilityRegistry,
    memory: Mutex<MemoryStore>,
    output: Mutex<ProfileConfig>,
    telemetry: TelemetryStore,
}

impl std::fmt::Debug for PublicHandler {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PublicHandler")
            .finish_non_exhaustive()
    }
}

impl PublicHandler {
    /// Creates an isolated handler with the native provider baseline.
    ///
    /// # Errors
    ///
    /// Returns [`ServiceError`] if local `SQLite` memory initialization fails.
    pub fn new() -> Result<Self, ServiceError> {
        let mut registry = CapabilityRegistry::default();
        for (id, provider) in [
            ("context", "native-repository"),
            ("compression", "builtin-extractive"),
            ("memory", "sqlite"),
            ("execution", "native-process"),
            ("output", "caveman-policy"),
        ] {
            registry.register(id, provider, true, provider, false);
        }
        let telemetry = if cfg!(test) {
            TelemetryStore::open_in_memory()
        } else {
            let data = data_directory().ok_or(ServiceError::Internal("data-directory"))?;
            fs::create_dir_all(&data).map_err(|_| ServiceError::Internal("data-directory"))?;
            TelemetryStore::open(data.join("telemetry.db"), 4)
        }
        .map_err(|_| ServiceError::Internal("telemetry-open"))?;
        Ok(Self {
            registry,
            memory: Mutex::new(
                MemoryStore::open_in_memory().map_err(|_| ServiceError::Internal("memory-open"))?,
            ),
            output: Mutex::new(ProfileConfig::default()),
            telemetry,
        })
    }

    /// Replaces the global final-output mode.
    ///
    /// # Errors
    ///
    /// Returns an error if handler synchronization failed.
    pub fn set_output_mode(&self, mode: OutputMode) -> Result<(), ServiceError> {
        self.output
            .lock()
            .map_err(|_| ServiceError::Poisoned)?
            .default_mode = mode;
        Ok(())
    }

    fn wrap(request: &RpcRequest, data: &Value) -> Value {
        let provider = match request.method.as_str() {
            "token_shrinker_build_context" | "token_shrinker_fetch_source" => {
                Some(("context", "native-repository", "local-workspace"))
            }
            "token_shrinker_search_memory" | "token_shrinker_remember" => {
                Some(("memory", "sqlite", "local-database"))
            }
            "token_shrinker_execute" => Some(("execution", "native-process", "local-process")),
            "token_shrinker_format_final" => Some(("output", "caveman-policy", "in-process")),
            "token_shrinker_route" => Some(("routing", "deterministic-router", "in-process")),
            _ => None,
        };
        let attribution =
            provider.map_or_else(Vec::new, |(capability, provider, data_boundary)| {
                vec![json!({
                    "capability": capability,
                    "provider": provider,
                    "fallbackFrom": Value::Null,
                    "dataBoundary": data_boundary
                })]
            });
        json!({"protocolVersion": ProtocolVersion::CURRENT.to_string(), "requestId": request.id.as_str(), "warnings": [], "providerAttribution": attribution, "data": data})
    }

    fn dispatch_tool(
        &self,
        request: &RpcRequest,
        cancelled: &AtomicBool,
    ) -> Result<Value, ServiceError> {
        let data = match request.method.as_str() {
            "token_shrinker_capabilities" | "capabilities" => self.capabilities()?,
            "token_shrinker_route" => serde_json::to_value(route(
                &deserialize(&request.params)?,
                RouterConfig::default(),
            ))
            .map_err(ServiceError::Json)?,
            "token_shrinker_build_context" => self.build_context(request)?,
            "token_shrinker_fetch_source" => fetch_source(&request.params)?,
            "token_shrinker_search_memory" => self.search_memory(&request.params)?,
            "token_shrinker_remember" => self.remember(&request.params)?,
            "token_shrinker_execute" => execute(&request.params, cancelled)?,
            "token_shrinker_stats" => self.stats()?,
            "token_shrinker_format_final" => self.format_final(&request.params)?,
            "health" => {
                serde_json::to_value(self.registry.overall_health()).map_err(ServiceError::Json)?
            }
            _ => return Err(ServiceError::MethodNotFound),
        };
        Ok(Self::wrap(request, &data))
    }

    fn capabilities(&self) -> Result<Value, ServiceError> {
        let capabilities =
            serde_json::to_value(self.registry.list()).map_err(ServiceError::Json)?;
        Ok(json!({
            "binaryVersion": env!("CARGO_PKG_VERSION"),
            "packageVersion": env!("CARGO_PKG_VERSION"),
            "protocolVersion": ProtocolVersion::CURRENT.to_string(),
            "mcpProtocolVersion": MCP_PROTOCOL_VERSION,
            "schemaVersion": PUBLIC_SCHEMA_VERSION,
            "health": match self.registry.overall_health() { HealthState::Healthy => "healthy", HealthState::Degraded => "degraded", HealthState::Failed => "failed" },
            "capabilities": capabilities,
            "tools": tool_definitions().into_iter().map(|tool| tool.name).collect::<Vec<_>>()
            ,"nativeTransport": native_transport_diagnostics(|key| env::var(key).ok())
        }))
    }

    fn build_context(&self, request: &RpcRequest) -> Result<Value, ServiceError> {
        let started = Instant::now();
        let params: ContextParams = deserialize(&request.params)?;
        let budget =
            TokenBudget::from_u32(params.budget).ok_or(ServiceError::Internal("zero-budget"))?;
        let result = build_baseline_context(params.root, &params.goal, budget)
            .map_err(|_| ServiceError::Internal("context-build"))?;
        let now = unix_millis()?;
        self.telemetry
            .record_request(&RequestEvent {
                request_id: request.id.clone(),
                session_id: "local-mcp".to_owned(),
                agent: "mcp-client".to_owned(),
                mode: RouteMode::Build,
                started_at_ms: now,
                duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                status: EventStatus::Success,
            })
            .and_then(|()| {
                self.telemetry.record_tokens(&TokenEvent {
                    request_id: request.id.clone(),
                    stage: "context".to_owned(),
                    direction: TokenDirection::Input,
                    raw_tokens: result.discovered_tokens,
                    optimized_tokens: result.bundle.trace.used_tokens,
                    tokenizer: "byte_upper_bound_v1".to_owned(),
                    exact: false,
                    created_at_ms: now,
                })
            })
            .map_err(|_| ServiceError::Internal("telemetry-write"))?;
        let mut bundle = serde_json::to_value(&result.bundle).map_err(ServiceError::Json)?;
        let omission_total = bundle["omissions"].as_array().map_or(0, Vec::len);
        if let Some(omissions) = bundle["omissions"].as_array_mut() {
            omissions.truncate(100);
        }
        let omission_returned = bundle["omissions"].as_array().map_or(0, Vec::len);
        Ok(json!({"bundle": bundle, "omissionSummary": {
                "total": omission_total,
                "returned": omission_returned,
                "truncated": omission_total.saturating_sub(omission_returned)
            }, "warnings": result.warnings,
            "repositoryTrace": result.repository_trace}))
    }

    fn stats(&self) -> Result<Value, ServiceError> {
        let savings = self
            .telemetry
            .token_savings()
            .map_err(|_| ServiceError::Internal("telemetry-read"))?
            .into_iter()
            .map(|row| {
                json!({
                    "tokenizer": row.tokenizer,
                    "exact": row.exact,
                    "rawTokens": row.raw_tokens,
                    "optimizedTokens": row.optimized_tokens,
                    "savingsTokens": row.savings_tokens,
                    "savingsPercent": row.savings_percent(),
                    "eventCount": row.event_count
                })
            })
            .collect::<Vec<_>>();
        Ok(json!({"savings": savings, "contentTelemetry": false}))
    }

    fn search_memory(&self, value: &Value) -> Result<Value, ServiceError> {
        let params: MemorySearch = deserialize(value)?;
        let scope = params
            .repository
            .map_or(MemoryScope::User, MemoryScope::Repository);
        let now = unix_millis()?;
        let records = self
            .memory
            .lock()
            .map_err(|_| ServiceError::Poisoned)?
            .search(
                &scope,
                &params.query,
                now,
                params.limit.unwrap_or(20).min(100),
            )
            .map_err(|_| ServiceError::Internal("memory-search"))?;
        Ok(json!({"records": records.into_iter().map(memory_json).collect::<Vec<_>>() }))
    }

    fn remember(&self, value: &Value) -> Result<Value, ServiceError> {
        let params: MemoryWrite = deserialize(value)?;
        let memory = NewMemory {
            id: params.id.clone(),
            scope: params
                .repository
                .map_or(MemoryScope::User, MemoryScope::Repository),
            content: params.content,
            created_at_unix_ms: unix_millis()?,
            expires_at_unix_ms: params.expires_at_unix_ms,
        };
        self.memory
            .lock()
            .map_err(|_| ServiceError::Poisoned)?
            .remember(&memory)
            .map_err(|_| ServiceError::Internal("memory-write"))?;
        Ok(json!({"stored": true, "id": params.id}))
    }

    fn format_final(&self, value: &Value) -> Result<Value, ServiceError> {
        let params: FormatParams = deserialize(value)?;
        let mode = params.mode.as_deref().map(parse_output_mode).transpose()?;
        let request = FormatRequest {
            agent: params.agent,
            tool: params.tool,
            request_mode: mode,
            ..FormatRequest::default()
        };
        let output = self.output.lock().map_err(|_| ServiceError::Poisoned)?;
        let decision = resolve(&output, &request);
        Ok(json!({"text": params.text, "decision": decision}))
    }
}

impl RpcHandler for PublicHandler {
    fn handle(&self, request: &RpcRequest, cancelled: &AtomicBool) -> Result<Value, ServiceError> {
        self.dispatch_tool(request, cancelled)
    }
}

fn deserialize<T: for<'de> Deserialize<'de>>(value: &Value) -> Result<T, ServiceError> {
    serde_json::from_value(value.clone()).map_err(ServiceError::Json)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ContextParams {
    root: PathBuf,
    goal: String,
    budget: u32,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FetchParams {
    root: PathBuf,
    source_id: String,
}
fn fetch_source(value: &Value) -> Result<Value, ServiceError> {
    let params: FetchParams = deserialize(value)?;
    let source_id = token_shrinker_context::SourceId::new(params.source_id)
        .map_err(|_| ServiceError::Internal("source-id"))?;
    let source = RepositoryProvider::open(params.root)
        .and_then(|provider| provider.fetch(&source_id))
        .map_err(|_| ServiceError::Internal("source-fetch"))?;
    serde_json::to_value(source).map_err(ServiceError::Json)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MemorySearch {
    query: String,
    repository: Option<String>,
    limit: Option<u32>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MemoryWrite {
    id: String,
    content: String,
    repository: Option<String>,
    expires_at_unix_ms: Option<i64>,
}
fn memory_json(record: token_shrinker_memory::MemoryRecord) -> Value {
    let scope = match record.scope {
        MemoryScope::User => json!({"kind":"user"}),
        MemoryScope::Repository(key) => json!({"kind":"repository","key":key}),
    };
    json!({"id":record.id,"scope":scope,"content":record.content,"createdAtUnixMs":record.created_at_unix_ms,"expiresAtUnixMs":record.expires_at_unix_ms})
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExecuteParams {
    program: PathBuf,
    #[serde(default)]
    args: Vec<String>,
    working_directory: PathBuf,
    #[serde(default)]
    environment: BTreeMap<String, String>,
    timeout_ms: Option<u64>,
}
fn execute(value: &Value, cancelled: &AtomicBool) -> Result<Value, ServiceError> {
    let params: ExecuteParams = deserialize(value)?;
    let environment_keys = params.environment.keys().cloned().chain([
        "PATH".to_owned(),
        "SYSTEMROOT".to_owned(),
        "HOME".to_owned(),
        "TEMP".to_owned(),
        "TMP".to_owned(),
    ]);
    let policy = ExecutionPolicy::new(
        [params.program.clone()],
        Vec::new(),
        [params.working_directory.clone()],
        environment_keys,
        ["LD_PRELOAD".to_owned(), "DYLD_INSERT_LIBRARIES".to_owned()],
        Duration::from_mins(5),
        256 * 1024,
    )
    .map_err(|_| ServiceError::Internal("execution-policy"))?;
    let result = ExecutionEngine::new(policy)
        .execute(
            &ExecutionRequest {
                program: params.program,
                args: params.args,
                working_directory: params.working_directory,
                environment: params.environment,
                timeout: Duration::from_millis(params.timeout_ms.unwrap_or(30_000)),
            },
            cancelled,
        )
        .map_err(|_| ServiceError::Internal("execution"))?;
    let summary = compress_terminal(&result.terminal_input(None), 40, 40);
    let termination = match result.termination {
        TerminationReason::Completed => "completed",
        TerminationReason::TimedOut => "timed_out",
        TerminationReason::Cancelled => "cancelled",
    };
    Ok(
        json!({"command":result.command,"exitCode":result.exit_code,"termination":termination,"durationMs":result.duration.as_millis(),"stdout":{"text":String::from_utf8_lossy(&result.stdout.retained_bytes()),"totalBytes":result.stdout.total_bytes,"truncated":result.stdout.truncated},"stderr":{"text":String::from_utf8_lossy(&result.stderr.retained_bytes()),"totalBytes":result.stderr.total_bytes,"truncated":result.stderr.truncated},"summary":summary}),
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FormatParams {
    text: String,
    mode: Option<String>,
    agent: Option<String>,
    tool: Option<String>,
}
fn parse_output_mode(value: &str) -> Result<OutputMode, ServiceError> {
    match value {
        "lite" => Ok(OutputMode::Lite),
        "full" => Ok(OutputMode::Full),
        "ultra" => Ok(OutputMode::Ultra),
        "wenyan-lite" => Ok(OutputMode::WenyanLite),
        "wenyan-full" => Ok(OutputMode::WenyanFull),
        "wenyan-ultra" => Ok(OutputMode::WenyanUltra),
        "off" => Ok(OutputMode::Off),
        _ => Err(ServiceError::Internal("output-mode")),
    }
}

fn unix_millis() -> Result<i64, ServiceError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ServiceError::Internal("clock"))?
        .as_millis();
    i64::try_from(millis).map_err(|_| ServiceError::Internal("clock"))
}

fn data_directory() -> Option<PathBuf> {
    env::var_os("TOKEN_SHRINKER_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("LOCALAPPDATA").map(|path| PathBuf::from(path).join("Token-Shrinker/data"))
        })
        .or_else(|| {
            env::var_os("XDG_DATA_HOME").map(|path| PathBuf::from(path).join("token-shrinker"))
        })
        .or_else(|| {
            env::var_os("HOME").map(|path| PathBuf::from(path).join(".local/share/token-shrinker"))
        })
}

fn native_transport_diagnostics(get: impl Fn(&str) -> Option<String>) -> Value {
    const ENDPOINT_KEYS: &[&str] = &[
        "ANTHROPIC_BASE_URL",
        "OPENAI_BASE_URL",
        "GOOGLE_GEMINI_BASE_URL",
        "GOOGLE_VERTEX_BASE_URL",
    ];
    let overrides = ENDPOINT_KEYS
        .iter()
        .filter(|key| get(key).is_some_and(|value| !value.trim().is_empty()))
        .copied()
        .collect::<Vec<_>>();
    let wrapper = get("TOKEN_SHRINKER_BINARY").filter(|value| {
        let name = Path::new(value)
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        matches!(
            name.as_str(),
            "claude" | "codex" | "gemini" | "opencode" | "opencode2" | "aider"
        )
    });
    let safe = overrides.is_empty() && wrapper.is_none();
    let warning_codes = overrides
        .iter()
        .map(|_| "provider-endpoint-override")
        .chain(wrapper.iter().map(|_| "wrapper-recursion"))
        .collect::<Vec<_>>();
    json!({
        "safe": safe,
        "remoteControlEligible": !overrides.contains(&"ANTHROPIC_BASE_URL") && wrapper.is_none(),
        "configuredEndpointOverrides": overrides,
        "wrapperRecursionDetected": wrapper.is_some(),
        "warningCodes": warning_codes,
        "remediation": if safe { Value::Null } else {
            json!("Remove unintended provider base-URL overrides or agent wrappers; Token-Shrinker adapters must use MCP tools while the agent keeps its native model transport. No setting was changed.")
        }
    })
}

/// Serves MCP over newline-delimited UTF-8 stdio until input closes.
///
/// # Errors
///
/// Returns an I/O error if stdin or stdout fails.
pub fn serve_stdio(handler: &PublicHandler) -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if let Some(response) = handle_mcp_message(handler, &line) {
            serde_json::to_writer(&mut stdout, &response)?;
            stdout.write_all(b"\n")?;
            stdout.flush()?;
        }
    }
    Ok(())
}

/// Handles one MCP JSON-RPC message. Notifications intentionally return no response.
#[must_use]
pub fn handle_mcp_message(handler: &PublicHandler, line: &str) -> Option<Value> {
    let message: Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(_) => return Some(mcp_error(&Value::Null, -32700, "Parse error")),
    };
    let id = message.get("id").cloned();
    let method = message
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let id = id?;
    let result = match method {
        "initialize" => {
            let requested = message["params"]["protocolVersion"].as_str();
            let negotiated = match requested {
                Some(version @ (MCP_PROTOCOL_VERSION | MCP_PROTOCOL_VERSION_COMPAT)) => version,
                _ => MCP_PROTOCOL_VERSION,
            };
            Ok(
                json!({"protocolVersion":negotiated,"capabilities":{"tools":{"listChanged":false}},"serverInfo":{"name":"token-shrinker","title":"Token-Shrinker","version":env!("CARGO_PKG_VERSION"),"description":"Local context optimization service"},"instructions":"Use capabilities first; execution requires explicit user approval."}),
            )
        }
        "ping" => Ok(json!({})),
        "tools/list" => serde_json::to_value(json!({"tools":tool_definitions()}))
            .map_err(|_| ServiceError::Internal("tool-list")),
        "tools/call" => call_mcp_tool(handler, &message, &id),
        _ => Err(ServiceError::MethodNotFound),
    };
    Some(match result {
        Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
        Err(ServiceError::MethodNotFound) => mcp_error(&id, -32601, "Method not found"),
        Err(_) => mcp_error(&id, -32602, "Invalid parameters"),
    })
}

fn call_mcp_tool(
    handler: &PublicHandler,
    message: &Value,
    id: &Value,
) -> Result<Value, ServiceError> {
    let params = message
        .get("params")
        .ok_or(ServiceError::Internal("mcp-params"))?;
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or(ServiceError::Internal("mcp-name"))?;
    if !tool_definitions().iter().any(|tool| tool.name == name) {
        return Err(ServiceError::MethodNotFound);
    }
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let raw_id = id.as_str().map_or_else(|| id.to_string(), str::to_owned);
    let filtered = raw_id
        .to_ascii_lowercase()
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || *character == '-')
        .take(48)
        .collect::<String>();
    let request_id = RequestId::new(format!(
        "mcp-{}",
        if filtered.is_empty() {
            "request"
        } else {
            &filtered
        }
    ))
    .map_err(|_| ServiceError::Internal("request-id"))?;
    let request = RpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: request_id,
        protocol_version: ProtocolVersion::CURRENT,
        auth_token: "0".repeat(64),
        method: name.to_owned(),
        params: arguments,
        deadline_unix_ms: None,
    };
    match handler.handle(&request, &AtomicBool::new(false)) {
        Ok(structured) => Ok(
            json!({"content":[{"type":"text","text":serde_json::to_string(&structured).map_err(ServiceError::Json)?}],"structuredContent":structured,"isError":false}),
        ),
        Err(error) => {
            Ok(json!({"content":[{"type":"text","text":error.to_string()}],"isError":true}))
        }
    }
}

fn mcp_error(id: &Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(method: &str, params: Value) -> RpcRequest {
        RpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: RequestId::new("test-request").expect("id"),
            protocol_version: ProtocolVersion::CURRENT,
            auth_token: "0".repeat(64),
            method: method.to_owned(),
            params,
            deadline_unix_ms: None,
        }
    }

    #[test]
    fn tools_are_stable_annotated_and_invokable() {
        let tools = tool_definitions();
        assert_eq!(tools.len(), 9);
        assert_eq!(tools[0].name, "token_shrinker_capabilities");
        assert_eq!(tools[8].name, "token_shrinker_format_final");
        assert!(
            tools
                .iter()
                .all(|tool| tool.input_schema["type"] == "object")
        );
        let handler = PublicHandler::new().expect("handler");
        for tool in tools {
            if matches!(
                tool.name,
                "token_shrinker_build_context"
                    | "token_shrinker_fetch_source"
                    | "token_shrinker_execute"
                    | "token_shrinker_remember"
            ) {
                continue;
            }
            let params = match tool.name {
                "token_shrinker_search_memory" => json!({"query":"none"}),
                "token_shrinker_format_final" => json!({"text":"hello"}),
                _ => json!({}),
            };
            assert!(
                handler
                    .handle(&request(tool.name, params), &AtomicBool::new(false))
                    .is_ok(),
                "{}",
                tool.name
            );
        }
    }

    #[test]
    fn stdio_lifecycle_lists_and_calls_structured_tools() {
        let handler = PublicHandler::new().expect("handler");
        let initialized = handle_mcp_message(&handler, r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}"#).expect("response");
        assert_eq!(
            initialized["result"]["protocolVersion"],
            MCP_PROTOCOL_VERSION
        );
        let compatible = handle_mcp_message(&handler, r#"{"jsonrpc":"2.0","id":10,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"gemini","version":"test"}}}"#).expect("compatible response");
        assert_eq!(
            compatible["result"]["protocolVersion"],
            MCP_PROTOCOL_VERSION_COMPAT
        );
        let listed = handle_mcp_message(
            &handler,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        )
        .expect("response");
        assert_eq!(
            listed["result"]["tools"].as_array().expect("tools").len(),
            9
        );
        let called = handle_mcp_message(&handler, r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"token_shrinker_capabilities","arguments":{}}}"#).expect("response");
        assert_eq!(called["result"]["isError"], false);
        assert!(called["result"]["structuredContent"].is_object());
        assert!(
            handle_mcp_message(
                &handler,
                r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#
            )
            .is_none()
        );
    }

    #[test]
    fn build_and_fetch_preserve_addressable_source() {
        let directory = tempfile::tempdir().expect("temp");
        std::fs::write(directory.path().join("main.rs"), "fn target() {}\n").expect("source");
        let handler = PublicHandler::new().expect("handler");
        let built = handler
            .handle(
                &request(
                    "token_shrinker_build_context",
                    json!({"root":directory.path(),"goal":"target","budget":1000}),
                ),
                &AtomicBool::new(false),
            )
            .expect("build");
        let source_id = built["data"]["bundle"]["items"][0]["sourceId"]
            .as_str()
            .expect("source id");
        assert_eq!(
            built["providerAttribution"],
            json!([{
                "capability":"context",
                "provider":"native-repository",
                "fallbackFrom":Value::Null,
                "dataBoundary":"local-workspace"
            }])
        );
        let fetched = handler
            .handle(
                &request(
                    "token_shrinker_fetch_source",
                    json!({"root":directory.path(),"sourceId":source_id}),
                ),
                &AtomicBool::new(false),
            )
            .expect("fetch");
        assert_eq!(fetched["data"]["content"], "fn target() {}\n");

        let stats = handler
            .handle(
                &request("token_shrinker_stats", json!({})),
                &AtomicBool::new(false),
            )
            .expect("stats");
        assert_eq!(stats["data"]["contentTelemetry"], false);
        assert_eq!(stats["data"]["savings"][0]["eventCount"], 1);
        assert!(stats["data"]["savings"][0]["rawTokens"].as_u64().is_some());
    }

    #[test]
    fn bounded_context_includes_matching_range_from_large_source() {
        let directory = tempfile::tempdir().expect("temp");
        let mut source = (0..300)
            .map(|index| format!("export const filler{index} = {index};"))
            .collect::<Vec<_>>();
        source.insert(
            210,
            "export function selectWindowsNativeExecutable() { return 'token-shrinker.exe'; }"
                .to_owned(),
        );
        std::fs::write(directory.path().join("launcher.ts"), source.join("\n")).expect("source");
        let handler = PublicHandler::new().expect("handler");
        let built = handler
            .handle(
                &request(
                    "token_shrinker_build_context",
                    json!({"root":directory.path(),
                        "goal":"selectWindowsNativeExecutable Windows native executable",
                        "budget":4000}),
                ),
                &AtomicBool::new(false),
            )
            .expect("build");
        let items = built["data"]["bundle"]["items"].as_array().expect("items");
        let matching = items
            .iter()
            .find(|item| {
                item["content"]
                    .as_str()
                    .is_some_and(|content| content.contains("selectWindowsNativeExecutable"))
            })
            .expect("matching chunk included");
        assert!(
            matching["sourceId"]
                .as_str()
                .expect("source id")
                .contains("#L")
        );
        assert!(built["data"]["bundle"]["used"]["tokens"].as_u64().unwrap() <= 4000);
        assert_eq!(
            built["data"]["omissionSummary"]["returned"],
            built["data"]["bundle"]["omissions"]
                .as_array()
                .expect("omissions")
                .len()
        );
    }

    #[test]
    fn capabilities_report_model_transport_separately_from_execution() {
        let report = native_transport_diagnostics(|key| {
            (key == "OPENAI_BASE_URL").then(|| "http://proxy.invalid".to_owned())
        });
        assert_eq!(report["safe"], false);
        assert_eq!(report["wrapperRecursionDetected"], false);
        assert_eq!(report["configuredEndpointOverrides"][0], "OPENAI_BASE_URL");
    }

    #[test]
    fn machine_envelope_is_identical_across_output_modes() {
        let handler = PublicHandler::new().expect("handler");
        let rpc = request("token_shrinker_capabilities", json!({}));
        let full = handler.handle(&rpc, &AtomicBool::new(false)).expect("full");
        handler.set_output_mode(OutputMode::Ultra).expect("mode");
        let ultra = handler
            .handle(&rpc, &AtomicBool::new(false))
            .expect("ultra");
        assert_eq!(
            serde_json::to_vec(&full).expect("json"),
            serde_json::to_vec(&ultra).expect("json")
        );
    }

    #[test]
    fn write_and_execution_tools_are_invokable_with_explicit_inputs() {
        let handler = PublicHandler::new().expect("handler");
        let remembered = handler
            .handle(
                &request(
                    "token_shrinker_remember",
                    json!({"id":"mcp-memory","content":"explicit memory"}),
                ),
                &AtomicBool::new(false),
            )
            .expect("remember");
        assert_eq!(remembered["data"]["stored"], true);
        let searched = handler
            .handle(
                &request("token_shrinker_search_memory", json!({"query":"explicit"})),
                &AtomicBool::new(false),
            )
            .expect("search");
        assert_eq!(searched["data"]["records"][0]["id"], "mcp-memory");

        let directory = tempfile::tempdir().expect("working directory");
        let executed = handler
            .handle(
                &request(
                    "token_shrinker_execute",
                    json!({
                        "program": std::env::current_exe().expect("test executable"),
                        "args": ["--list"],
                        "workingDirectory": directory.path(),
                        "timeoutMs": 10_000
                    }),
                ),
                &AtomicBool::new(false),
            )
            .expect("execute");
        assert_eq!(executed["data"]["termination"], "completed");
        assert_eq!(executed["data"]["exitCode"], 0);
    }
}
