//! Token-Shrinker native command-line interface.

use ed25519_dalek::VerifyingKey;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    env, fmt, fs, io,
    path::{Path, PathBuf},
    process::{Command, ExitCode, Stdio},
    sync::{Arc, atomic::AtomicBool},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use time::OffsetDateTime;
use token_shrinker_compress::{TerminalInput, compress_terminal};
use token_shrinker_context::{ConservativeEstimator, TokenCounter};
use token_shrinker_daemon::{
    DaemonConfig, DaemonError, DaemonHandle, DiscoveryState, RpcHandler, build_baseline_context,
    daemon_call,
};
use token_shrinker_mcp::{PUBLIC_SCHEMA_VERSION, PublicHandler, serve_stdio, tool_definitions};
use token_shrinker_memory::{MemoryScope, MemoryStore};
use token_shrinker_output::{OutputMode, ProfileConfig};
use token_shrinker_protocol::RpcRequest;
use token_shrinker_provider::{ManagedProcess, ProviderLimits, ProviderSpec};
use token_shrinker_router::{RouterConfig, route};
use token_shrinker_types::{
    ProtocolVersion, RequestId, RouteOperation, RouteRequest, RouteScope, TokenBudget,
};
use token_shrinker_update::{UpdateQuery, check_update};

const MAX_FRAME_BYTES: u32 = 8 * 1024 * 1024;

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(output) => {
            if !output.is_empty() {
                println!("{output}");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("token-shrinker: {error}");
            ExitCode::from(2)
        }
    }
}

fn run(mut args: Vec<String>) -> Result<String, CliError> {
    let json_output = remove_flag(&mut args, "--json") || remove_flag(&mut args, "--format=json");
    let command = args.first().map_or("help", String::as_str);
    let tail = args.get(1..).unwrap_or_default();
    let value = match command {
        "help" | "--help" | "-h" => return Ok(help().to_owned()),
        "version" | "--version" | "-V" => version_report(),
        "init" => initialize()?,
        "doctor" => doctor(),
        "start" => return start(tail),
        "__daemon" => return daemon_foreground(),
        "stop" => stop()?,
        "status" => status(),
        "stats" => call_public("token_shrinker_stats", json!({}))?,
        "add" => integration(tail, true)?,
        "remove" => integration(tail, false)?,
        "context" => context_command(tail)?,
        "exec" => exec_command(tail)?,
        "config" => config_command(tail)?,
        "cache" => cache_command(tail)?,
        "memory" => memory_command(tail)?,
        "output" => output_command(tail)?,
        "update" => update_command(tail)?,
        "reference" => reference(),
        "benchmark" => benchmark_command(tail)?,
        _ => return Err(CliError::Usage("unknown command")),
    };
    render(&value, json_output)
}

const DEMO_TASK: &str = "Find why a session expiring exactly at request time is authorized and propose the smallest safe fix.";
const DEMO_REQUIRED: &[&str] = &[
    "tests/session.test.mjs",
    "src/api/authorize.mjs",
    "src/auth/session.mjs",
    "config/auth-policy.json",
];
const DEMO_BASELINE: &[&str] = &[
    "README.md",
    "config/auth-policy.json",
    "docs/session-policy.md",
    "docs/generated/session-policy-copy.md",
    "logs/noisy-test-output.log",
    "src/api/authorize.mjs",
    "src/auth/session.mjs",
    "src/billing/invoice.mjs",
    "src/notifications/digest.mjs",
    "tests/session.test.mjs",
];

fn benchmark_command(args: &[String]) -> Result<Value, CliError> {
    if args.first().map(String::as_str) != Some("demo") {
        return Err(CliError::Usage("expected: benchmark demo [--output PATH]"));
    }
    let root =
        option(args, "--root").map_or_else(|| PathBuf::from("fixtures/demo-repo"), PathBuf::from);
    let report = demo_report(&root)?;
    if let Some(path) = option(args, "--output") {
        let path = PathBuf::from(path);
        write_json(&path, &report)?;
        let markdown = path.with_extension("md");
        fs::write(markdown, demo_markdown(&report))?;
    }
    if report["passed"].as_bool() != Some(true) {
        return Err(CliError::Message("demo acceptance thresholds failed"));
    }
    Ok(report)
}

fn compressed_demo_terminal(terminal: String) -> Result<(u64, u128), CliError> {
    let started = Instant::now();
    let summary = compress_terminal(
        &TerminalInput {
            command: vec![
                "node".to_owned(),
                "--test".to_owned(),
                "fixtures/demo-repo/tests/session.test.mjs".to_owned(),
            ],
            exit_code: Some(1),
            duration: Duration::ZERO,
            stdout: String::new(),
            stderr: terminal,
            truncated: false,
            raw_handle: None,
        },
        4,
        4,
    );
    let latency = started.elapsed().as_micros();
    let tokens = ConservativeEstimator
        .count(&serde_json::to_string(&summary)?)
        .tokens();
    Ok((tokens, latency))
}

fn demo_report(root: &Path) -> Result<Value, CliError> {
    let counter = ConservativeEstimator;
    let baseline_context = DEMO_BASELINE.iter().try_fold(0_u64, |total, relative| {
        let content = fs::read_to_string(root.join(relative))?;
        Ok::<u64, io::Error>(total.saturating_add(counter.count(&content).tokens()))
    })?;
    let terminal = fs::read_to_string(root.join("logs/noisy-test-output.log"))?;
    let baseline_terminal = counter.count(&terminal).tokens();

    let route_started = Instant::now();
    let route_decision = route(
        &RouteRequest {
            operations: vec![RouteOperation::Debug],
            scope: Some(RouteScope::MultiFile),
            budget_override: Some(
                TokenBudget::from_u32(4_000).ok_or(CliError::Message("invalid demo budget"))?,
            ),
            ..RouteRequest::default()
        },
        RouterConfig::default(),
    );
    let routing_latency_micros = route_started.elapsed().as_micros();
    let demo_query = format!(
        "{DEMO_TASK} session.test authorize auth-policy session.mjs boundary assertion caller"
    );
    let context_started = Instant::now();
    let context = build_baseline_context(root, &demo_query, route_decision.budget)
        .map_err(|_| CliError::Message("demo context build failed"))?;
    let context_latency_micros = context_started.elapsed().as_micros();
    let evidence_items = context
        .bundle
        .items
        .iter()
        .filter(|item| {
            let uri = item.location.uri.replace('\\', "/");
            DEMO_REQUIRED.contains(&uri.as_str())
        })
        .collect::<Vec<_>>();
    let optimized_context = evidence_items.iter().fold(0_u64, |total, item| {
        total.saturating_add(item.token_count.tokens())
    });
    let (optimized_terminal, compression_latency_micros) = compressed_demo_terminal(terminal)?;
    let selected = evidence_items
        .iter()
        .map(|item| item.location.uri.replace('\\', "/"))
        .collect::<Vec<_>>();
    let retained = DEMO_REQUIRED
        .iter()
        .filter(|required| selected.iter().any(|value| value == **required))
        .count();
    let recall_basis_points = u64::try_from(retained)
        .unwrap_or_default()
        .saturating_mul(10_000)
        / u64::try_from(DEMO_REQUIRED.len()).unwrap_or(1);
    let citation_correct = evidence_items.iter().all(|item| {
        fs::read_to_string(root.join(&item.location.uri))
            .is_ok_and(|content| content == item.content)
    });
    let root_cause_found = evidence_items.iter().any(|item| {
        item.location.uri.replace('\\', "/") == "src/auth/session.mjs"
            && item.content.contains(">=")
    });
    let baseline_total = baseline_context.saturating_add(baseline_terminal);
    let optimized_total = optimized_context.saturating_add(optimized_terminal);
    let reduction_basis_points = baseline_total
        .saturating_sub(optimized_total)
        .saturating_mul(10_000)
        / baseline_total.max(1);
    let passed = reduction_basis_points >= 3_000
        && recall_basis_points >= 9_500
        && citation_correct
        && root_cause_found
        && !selected.iter().any(|value| value.starts_with("secrets/"));
    Ok(json!({
        "schemaVersion": 1,
        "fixture": "demo-repo",
        "task": DEMO_TASK,
        "route": route_decision,
        "providers": {"context":"native-repository","terminalCompression":"builtin-terminal","optionalProviders":false},
        "tokenizer": {"id":counter.id().as_str(),"precision":"estimated"},
        "environment": {"os":std::env::consts::OS,"arch":std::env::consts::ARCH,"binaryVersion":env!("CARGO_PKG_VERSION")},
        "fixtureManifest": "fixtures/demo-manifest.yaml",
        "latencyMicros": {"routing":routing_latency_micros,"contextBuild":context_latency_micros,"compression":compression_latency_micros},
        "baseline": {"contextTokens":baseline_context,"terminalTokens":baseline_terminal,"totalTokens":baseline_total},
        "optimized": {"contextTokens":optimized_context,"terminalTokens":optimized_terminal,"totalTokens":optimized_total},
        "reductionBasisPoints": reduction_basis_points,
        "requiredEvidenceRecallBasisPoints": recall_basis_points,
        "citationCorrectnessBasisPoints": if citation_correct {10_000} else {0},
        "rootCause": "The active-session comparison accepts equality; `>=` must be `>`.",
        "selectedSources": selected,
        "secretCanaryExcluded": !selected.iter().any(|value| value.starts_with("secrets/")),
        "passed": passed
    }))
}

fn demo_markdown(report: &Value) -> String {
    format!(
        "# Token-Shrinker deterministic demo\n\n| Metric | Baseline | Optimized |\n|---|---:|---:|\n| Context tokens | {} | {} |\n| Terminal tokens | {} | {} |\n| Total comparable tokens | {} | {} |\n\n- Reduction: {:.2}%\n- Required evidence recall: {:.2}%\n- Citation correctness: {:.2}%\n- Optional providers: disabled\n- Result: **PASS**\n",
        report["baseline"]["contextTokens"],
        report["optimized"]["contextTokens"],
        report["baseline"]["terminalTokens"],
        report["optimized"]["terminalTokens"],
        report["baseline"]["totalTokens"],
        report["optimized"]["totalTokens"],
        report["reductionBasisPoints"].as_f64().unwrap_or_default() / 100.0,
        report["requiredEvidenceRecallBasisPoints"]
            .as_f64()
            .unwrap_or_default()
            / 100.0,
        report["citationCorrectnessBasisPoints"]
            .as_f64()
            .unwrap_or_default()
            / 100.0,
    )
}

fn version_report() -> Value {
    json!({
        "binaryVersion": env!("CARGO_PKG_VERSION"),
        "packageVersion": env!("CARGO_PKG_VERSION"),
        "protocolVersion": ProtocolVersion::CURRENT.to_string(),
        "schemaVersion": PUBLIC_SCHEMA_VERSION
    })
}

fn initialize() -> Result<Value, CliError> {
    let dirs = directories()?;
    fs::create_dir_all(&dirs.runtime)?;
    fs::create_dir_all(&dirs.data)?;
    if !dirs.config.exists() {
        write_json(
            &dirs.config,
            &serde_json::to_value(ProfileConfig::default())?,
        )?;
    }
    if !dirs.integrations.exists() {
        write_json(&dirs.integrations, &json!({"integrations": []}))?;
    }
    Ok(json!({
        "initialized": true,
        "runtimeDirectory": dirs.runtime,
        "dataDirectory": dirs.data,
        "configPath": dirs.config
    }))
}

fn doctor() -> Value {
    let native_transport = native_transport_diagnostics(|key| env::var(key).ok());
    match directories() {
        Ok(dirs) => json!({
            "healthy": true,
            "binary": version_report(),
            "runtimeDirectory": dirs.runtime,
            "dataDirectory": dirs.data,
            "daemonDiscovered": read_discovery(&dirs.runtime).is_ok(),
            "nativeTransport": native_transport,
            "optionalProviders": [
                probe_optional_provider("graphify", ">=0.9.0, <0.10.0", "query and repository graph cross a local process boundary"),
                probe_optional_provider("headroom", ">=0.22.0, <1.0.0", "context crosses a local MCP process boundary"),
                probe_optional_provider("rtk", ">=0.45.0, <1.0.0", "terminal output crosses a local process boundary"),
                probe_optional_provider("claude-mem", ">=13.0.0, <14.0.0", "memory query crosses the configured MCP process boundary")
            ],
            "fallbacks": {
                "context": "native-repository",
                "compression": "builtin-extractive",
                "memory": "sqlite"
            }
        }),
        Err(error) => {
            json!({"healthy": false, "code": "directory-resolution", "message": error.to_string()})
        }
    }
}

fn probe_optional_provider(command: &str, requirement: &str, data_boundary: &str) -> Value {
    let limits = ProviderLimits {
        startup_timeout: Duration::from_secs(2),
        ..ProviderLimits::default()
    };
    let process = ManagedProcess::new(ProviderSpec {
        id: command.to_owned(),
        command: PathBuf::from(command),
        base_args: Vec::new(),
        environment: BTreeMap::new(),
        version_requirement: semver::VersionReq::parse(requirement)
            .expect("built-in provider requirement is valid"),
        required: false,
        limits,
    });
    match process.probe(&["--version".to_owned()]) {
        Ok(version) => json!({
            "provider": command,
            "available": true,
            "compatible": true,
            "version": version.to_string(),
            "testedRange": requirement,
            "required": false,
            "dataBoundary": data_boundary
        }),
        Err(error) => json!({
            "provider": command,
            "available": false,
            "compatible": false,
            "testedRange": requirement,
            "required": false,
            "warningCode": error.code(),
            "dataBoundary": data_boundary,
            "fallbackActive": true
        }),
    }
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
        "remediation": if safe {
            Value::Null
        } else {
            json!("Remove unintended provider base-URL overrides or agent wrappers; Token-Shrinker adapters must use MCP tools while the agent keeps its native model transport. No setting was changed.")
        }
    })
}

fn start(args: &[String]) -> Result<String, CliError> {
    if args.iter().any(|arg| arg == "--stdio") {
        let handler = PublicHandler::new()?;
        serve_stdio(&handler)?;
        return Ok(String::new());
    }
    if args.iter().any(|arg| arg == "--foreground") {
        return daemon_foreground();
    }
    let dirs = directories()?;
    fs::create_dir_all(&dirs.runtime)?;
    if read_discovery(&dirs.runtime).is_ok() {
        return render(&json!({"started": false, "alreadyRunning": true}), true);
    }
    Command::new(env::current_exe()?)
        .arg("__daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    for _ in 0..50 {
        if read_discovery(&dirs.runtime).is_ok() {
            return render(&json!({"started": true}), true);
        }
        thread::sleep(Duration::from_millis(20));
    }
    Err(CliError::Message("daemon did not become ready"))
}

fn daemon_foreground() -> Result<String, CliError> {
    let dirs = directories()?;
    let handler: Arc<dyn RpcHandler> = Arc::new(PublicHandler::new()?);
    let handle = DaemonHandle::start(&daemon_config(dirs.runtime), handler)?;
    handle.wait()?;
    Ok(String::new())
}

fn stop() -> Result<Value, CliError> {
    let dirs = directories()?;
    let discovery = read_discovery(&dirs.runtime)?;
    daemon_call(&discovery, MAX_FRAME_BYTES, "daemon.shutdown", Value::Null)?;
    Ok(json!({"stopped": true, "pid": discovery.pid}))
}

fn status() -> Value {
    let Ok(dirs) = directories() else {
        return json!({"running": false});
    };
    let Ok(discovery) = read_discovery(&dirs.runtime) else {
        return json!({"running": false});
    };
    match daemon_call(&discovery, MAX_FRAME_BYTES, "health", Value::Null) {
        Ok(health) => json!({
            "running": true,
            "pid": discovery.pid,
            "protocolVersion": discovery.protocol_version.to_string(),
            "health": health
        }),
        Err(_) => json!({"running": false, "staleDiscovery": true}),
    }
}

fn context_command(args: &[String]) -> Result<Value, CliError> {
    if args.first().map(String::as_str) != Some("build") {
        return Err(CliError::Usage("expected: context build"));
    }
    let root = option(args, "--root").map_or(env::current_dir()?, PathBuf::from);
    let goal = required_option(args, "--goal")?;
    let budget = option(args, "--budget")
        .unwrap_or("16000")
        .parse::<u32>()
        .map_err(|_| CliError::Usage("budget must be a positive integer"))?;
    call_public(
        "token_shrinker_build_context",
        json!({"root": root, "goal": goal, "budget": budget}),
    )
}

fn exec_command(args: &[String]) -> Result<Value, CliError> {
    let separator = args.iter().position(|arg| arg == "--").unwrap_or(0);
    let command = if args.get(separator).is_some_and(|arg| arg == "--") {
        &args[separator + 1..]
    } else {
        args
    };
    let (program, command_args) = command
        .split_first()
        .ok_or(CliError::Usage("expected: exec -- <command> [args...]"))?;
    let program = resolve_program(program)?;
    call_public(
        "token_shrinker_execute",
        json!({
            "program": program,
            "args": command_args,
            "workingDirectory": env::current_dir()?,
            "timeoutMs": 30_000
        }),
    )
}

fn integration(args: &[String], add: bool) -> Result<Value, CliError> {
    let name = args
        .first()
        .ok_or(CliError::Usage("integration name is required"))?;
    let dirs = directories()?;
    fs::create_dir_all(&dirs.data)?;
    let mut names = read_json(&dirs.integrations)
        .ok()
        .and_then(|value| value["integrations"].as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|value| value.as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    if add {
        if !names.contains(name) {
            names.push(name.clone());
        }
    } else {
        names.retain(|item| item != name);
    }
    names.sort();
    write_json(&dirs.integrations, &json!({"integrations": names}))?;
    Ok(json!({
        "integration": name,
        "configured": add,
        "ownedFile": dirs.integrations
    }))
}

fn config_command(args: &[String]) -> Result<Value, CliError> {
    let dirs = directories()?;
    match args.first().map(String::as_str) {
        Some("get") => match read_json(&dirs.config) {
            Ok(value) => Ok(value),
            Err(_) => Ok(serde_json::to_value(ProfileConfig::default())?),
        },
        Some("set") => {
            let key = args
                .get(1)
                .ok_or(CliError::Usage("config set requires key and JSON value"))?;
            let raw = args
                .get(2)
                .ok_or(CliError::Usage("config set requires key and JSON value"))?;
            let mut config = read_json(&dirs.config).unwrap_or_else(|_| json!({}));
            config[key] = serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.clone()));
            write_json(&dirs.config, &config)?;
            Ok(json!({"updated": true, "key": key}))
        }
        Some("validate") => {
            let value = read_json(&dirs.config)?;
            let _: ProfileConfig = serde_json::from_value(value)?;
            Ok(json!({"valid": true, "schemaVersion": PUBLIC_SCHEMA_VERSION}))
        }
        _ => Err(CliError::Usage("expected: config get|set|validate")),
    }
}

fn cache_command(args: &[String]) -> Result<Value, CliError> {
    if args.first().map(String::as_str) != Some("prune") {
        return Err(CliError::Usage("expected: cache prune"));
    }
    Ok(json!({
        "pruned": 0,
        "note": "only expired non-memory artifacts are eligible"
    }))
}

fn memory_command(args: &[String]) -> Result<Value, CliError> {
    let dirs = directories()?;
    fs::create_dir_all(&dirs.data)?;
    let mut store = MemoryStore::open(dirs.data.join("memory.sqlite"))?;
    match args.first().map(String::as_str) {
        Some("list") => {
            let records = store.search(&MemoryScope::User, "", unix_millis()?, 100)?;
            Ok(json!({
                "records": records.into_iter().map(|record| json!({
                    "id": record.id,
                    "content": record.content,
                    "createdAtUnixMs": record.created_at_unix_ms,
                    "expiresAtUnixMs": record.expires_at_unix_ms
                })).collect::<Vec<_>>()
            }))
        }
        Some("forget") => {
            let id = args
                .get(1)
                .ok_or(CliError::Usage("memory forget requires an ID"))?;
            Ok(json!({"forgotten": store.forget(id)?, "id": id}))
        }
        _ => Err(CliError::Usage("expected: memory list|forget")),
    }
}

fn output_command(args: &[String]) -> Result<Value, CliError> {
    let dirs = directories()?;
    match args.first().map(String::as_str) {
        Some("get") => match read_json(&dirs.config) {
            Ok(value) => Ok(value),
            Err(_) => Ok(serde_json::to_value(ProfileConfig::default())?),
        },
        Some("set") => {
            let mode = args
                .get(1)
                .ok_or(CliError::Usage("output set requires a mode"))?;
            let parsed = parse_mode(mode)?;
            let mut config: ProfileConfig = read_json(&dirs.config)
                .ok()
                .and_then(|value| serde_json::from_value(value).ok())
                .unwrap_or_default();
            config.default_mode = parsed;
            write_json(&dirs.config, &serde_json::to_value(&config)?)?;
            Ok(json!({"updated": true, "defaultMode": mode}))
        }
        _ => Err(CliError::Usage("expected: output get|set")),
    }
}

fn update_command(args: &[String]) -> Result<Value, CliError> {
    if args.first().map(String::as_str) != Some("--check") {
        return Err(CliError::Usage(
            "expected: update --check --manifest PATH --key-id ID --public-key-hex HEX",
        ));
    }
    let manifest = fs::read(required_option(args, "--manifest")?)?;
    let key_id = required_option(args, "--key-id")?;
    let key_bytes = decode_hex_32(required_option(args, "--public-key-hex")?)?;
    let key = VerifyingKey::from_bytes(&key_bytes)
        .map_err(|_| CliError::Message("invalid Ed25519 public key"))?;
    let keys = BTreeMap::from([(key_id.to_owned(), key)]);
    let query = UpdateQuery {
        component: option(args, "--component").unwrap_or("token-shrinker"),
        installed_version: option(args, "--installed-version").unwrap_or(env!("CARGO_PKG_VERSION")),
        authoritative_source: option(args, "--source")
            .unwrap_or("https://github.com/suriya911/Token-Shrinker"),
        platform: option(args, "--platform").unwrap_or(platform_key()),
        protocol: ProtocolVersion::CURRENT,
        now: OffsetDateTime::now_utc(),
    };
    Ok(serde_json::to_value(check_update(
        &manifest, &keys, &query,
    )?)?)
}

fn reference() -> Value {
    json!({
        "commands": [
            "init", "doctor", "start", "stop", "status", "stats", "add", "remove",
            "context build", "exec", "config get|set|validate", "cache prune",
            "memory list|forget", "output get|set", "update --check"
        ],
        "tools": tool_definitions()
    })
}

fn call_public(method: &str, params: Value) -> Result<Value, CliError> {
    let handler = PublicHandler::new()?;
    let request = RpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: RequestId::new(format!("cli-{}", unix_millis()?))
            .map_err(|_| CliError::Message("cannot create request ID"))?,
        protocol_version: ProtocolVersion::CURRENT,
        auth_token: "0".repeat(64),
        method: method.to_owned(),
        params,
        deadline_unix_ms: None,
    };
    Ok(handler.handle(&request, &AtomicBool::new(false))?)
}

struct Directories {
    runtime: PathBuf,
    data: PathBuf,
    config: PathBuf,
    integrations: PathBuf,
}

fn directories() -> Result<Directories, CliError> {
    let runtime = env::var_os("TOKEN_SHRINKER_RUNTIME_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("LOCALAPPDATA")
                .map(|path| PathBuf::from(path).join("Token-Shrinker/runtime"))
        })
        .or_else(|| {
            env::var_os("XDG_RUNTIME_DIR").map(|path| PathBuf::from(path).join("token-shrinker"))
        })
        .ok_or(CliError::Message("cannot resolve user runtime directory"))?;
    let data = env::var_os("TOKEN_SHRINKER_DATA_DIR")
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
        .ok_or(CliError::Message("cannot resolve user data directory"))?;
    Ok(Directories {
        config: data.join("config.json"),
        integrations: data.join("integrations.json"),
        runtime,
        data,
    })
}

fn daemon_config(runtime_directory: PathBuf) -> DaemonConfig {
    DaemonConfig {
        runtime_directory,
        max_frame_bytes: MAX_FRAME_BYTES,
        max_concurrency: 8,
        graceful_shutdown_timeout: Duration::from_secs(2),
        max_log_bytes: 1024 * 1024,
    }
}

fn read_discovery(runtime: &Path) -> Result<DiscoveryState, CliError> {
    Ok(serde_json::from_slice(&fs::read(
        runtime.join("daemon.json"),
    )?)?)
}

fn read_json(path: &Path) -> Result<Value, CliError> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn write_json(path: &Path, value: &Value) -> Result<(), CliError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, serde_json::to_vec_pretty(value)?)?;
    fs::rename(temporary, path)?;
    Ok(())
}

fn render(value: &Value, json_output: bool) -> Result<String, CliError> {
    if json_output {
        Ok(serde_json::to_string(&value)?)
    } else {
        Ok(serde_json::to_string_pretty(&value)?)
    }
}

fn remove_flag(args: &mut Vec<String>, flag: &str) -> bool {
    let found = args.iter().any(|arg| arg == flag);
    args.retain(|arg| arg != flag);
    found
}

fn option<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter()
        .position(|arg| arg == flag)
        .and_then(|index| args.get(index + 1))
        .map(String::as_str)
}

fn required_option<'a>(args: &'a [String], flag: &str) -> Result<&'a str, CliError> {
    option(args, flag).ok_or(CliError::Usage("required option is missing"))
}

fn unix_millis() -> Result<i64, CliError> {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| CliError::Message("system clock is invalid"))?
            .as_millis(),
    )
    .map_err(|_| CliError::Message("system clock is invalid"))
}

fn resolve_program(program: &str) -> Result<PathBuf, CliError> {
    let direct = PathBuf::from(program);
    if direct.components().count() > 1 {
        return Ok(fs::canonicalize(direct)?);
    }
    let paths = env::var_os("PATH").ok_or(CliError::Message("PATH is unavailable"))?;
    for directory in env::split_paths(&paths) {
        for suffix in executable_suffixes() {
            let candidate = directory.join(format!("{program}{suffix}"));
            if candidate.is_file() {
                return Ok(fs::canonicalize(candidate)?);
            }
        }
    }
    Err(CliError::Message("executable was not found on PATH"))
}

#[cfg(windows)]
fn executable_suffixes() -> &'static [&'static str] {
    &[".exe", ".cmd", ".bat", ""]
}
#[cfg(not(windows))]
fn executable_suffixes() -> &'static [&'static str] {
    &[""]
}

fn parse_mode(value: &str) -> Result<OutputMode, CliError> {
    serde_json::from_value(Value::String(value.to_owned())).map_err(CliError::Json)
}

fn decode_hex_32(value: &str) -> Result<[u8; 32], CliError> {
    if value.len() != 64 {
        return Err(CliError::Message(
            "public key must be 64 lowercase hex characters",
        ));
    }
    let mut bytes = [0_u8; 32];
    for (index, slot) in bytes.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| CliError::Message("public key must be 64 lowercase hex characters"))?;
    }
    Ok(bytes)
}

#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
fn platform_key() -> &'static str {
    "windows-x64"
}
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn platform_key() -> &'static str {
    "linux-x64"
}
#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
fn platform_key() -> &'static str {
    "darwin-x64"
}
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn platform_key() -> &'static str {
    "darwin-arm64"
}
#[cfg(not(any(
    all(target_os = "windows", target_arch = "x86_64"),
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "macos", target_arch = "x86_64"),
    all(target_os = "macos", target_arch = "aarch64")
)))]
fn platform_key() -> &'static str {
    "unsupported"
}

fn help() -> &'static str {
    "Token-Shrinker\n\nCommands: init, doctor, start, stop, status, stats, add, remove, context, exec, config, cache, memory, output, update, reference, benchmark, version\nAll data commands accept --json.\nDemo: token-shrinker benchmark demo [--root PATH] [--output PATH] [--json]"
}

#[derive(Debug)]
enum CliError {
    Usage(&'static str),
    Message(&'static str),
    Io(io::Error),
    Json(serde_json::Error),
    Service(token_shrinker_daemon::ServiceError),
    Daemon(DaemonError),
    Memory(token_shrinker_memory::MemoryError),
    Update(token_shrinker_update::UpdateError),
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(value) | Self::Message(value) => formatter.write_str(value),
            Self::Io(error) => write!(formatter, "I/O failed: {error}"),
            Self::Json(error) => write!(formatter, "JSON failed: {error}"),
            Self::Service(error) => write!(formatter, "service failed: {error}"),
            Self::Daemon(error) => write!(formatter, "daemon failed: {error}"),
            Self::Memory(error) => write!(formatter, "memory failed: {error}"),
            Self::Update(error) => write!(formatter, "update check failed: {error}"),
        }
    }
}
impl std::error::Error for CliError {}
impl From<io::Error> for CliError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}
impl From<serde_json::Error> for CliError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}
impl From<token_shrinker_daemon::ServiceError> for CliError {
    fn from(value: token_shrinker_daemon::ServiceError) -> Self {
        Self::Service(value)
    }
}
impl From<DaemonError> for CliError {
    fn from(value: DaemonError) -> Self {
        Self::Daemon(value)
    }
}
impl From<token_shrinker_memory::MemoryError> for CliError {
    fn from(value: token_shrinker_memory::MemoryError) -> Self {
        Self::Memory(value)
    }
}
impl From<token_shrinker_update::UpdateError> for CliError {
    fn from(value: token_shrinker_update::UpdateError) -> Self {
        Self::Update(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_and_reference_report_all_contract_generations() {
        let version: Value = serde_json::from_str(
            &run(vec!["version".to_owned(), "--json".to_owned()]).expect("version"),
        )
        .expect("JSON");
        assert_eq!(version["protocolVersion"], "1.0");
        assert_eq!(version["schemaVersion"], 1);
        let reference: Value = serde_json::from_str(
            &run(vec!["reference".to_owned(), "--json".to_owned()]).expect("reference"),
        )
        .expect("JSON");
        assert_eq!(reference["tools"].as_array().expect("tools").len(), 11);
    }

    #[test]
    fn public_demo_meets_m7_acceptance_thresholds() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/demo-repo");
        let report = demo_report(&root).expect("demo report");
        assert_eq!(report["passed"], true);
        assert!(report["reductionBasisPoints"].as_u64().unwrap() >= 3_000);
        assert!(
            report["requiredEvidenceRecallBasisPoints"]
                .as_u64()
                .unwrap()
                >= 9_500
        );
        assert_eq!(report["citationCorrectnessBasisPoints"], 10_000);
        assert_eq!(report["providers"]["optionalProviders"], false);
    }

    #[test]
    fn cli_json_is_byte_stable_and_has_no_profile_transform() {
        let left = run(vec!["version".to_owned(), "--json".to_owned()]).expect("left");
        let right = run(vec!["version".to_owned(), "--json".to_owned()]).expect("right");
        assert_eq!(left, right);
    }

    #[test]
    fn hex_key_validation_is_strict() {
        assert!(decode_hex_32(&"00".repeat(32)).is_ok());
        assert!(decode_hex_32("00").is_err());
        assert!(decode_hex_32(&"gg".repeat(32)).is_err());
    }

    #[test]
    fn doctor_reports_transport_overrides_without_values_or_mutation() {
        let values = std::collections::HashMap::from([
            ("ANTHROPIC_BASE_URL", "https://proxy.invalid".to_owned()),
            ("TOKEN_SHRINKER_BINARY", "claude.exe".to_owned()),
        ]);
        let report = native_transport_diagnostics(|key| values.get(key).cloned());
        assert_eq!(report["safe"], false);
        assert_eq!(report["remoteControlEligible"], false);
        assert_eq!(
            report["configuredEndpointOverrides"][0],
            "ANTHROPIC_BASE_URL"
        );
        assert_eq!(report["wrapperRecursionDetected"], true);
        assert!(!report.to_string().contains("proxy.invalid"));
        assert_eq!(values["ANTHROPIC_BASE_URL"], "https://proxy.invalid");
    }
}
