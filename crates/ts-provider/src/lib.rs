//! Safe process and MCP-stdio boundary for optional providers.

use std::collections::BTreeMap;
use std::fmt;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use semver::{Version, VersionReq};
use serde_json::{Value, json};

/// Content-free record of the provider path used for a request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderAttribution {
    /// Provider that produced the returned value.
    pub provider: String,
    /// Preferred provider that failed before fallback, if any.
    pub fallback_from: Option<String>,
    /// Stable failure warning attached to a fallback.
    pub warning_code: Option<&'static str>,
}

/// Value paired with content-free provider attribution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderOutcome<T> {
    /// Returned provider or fallback value.
    pub value: T,
    /// Provider selection metadata.
    pub attribution: ProviderAttribution,
}

/// Comparable quality sample for optional-provider benchmarks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderQuality {
    /// Raw input tokens under one labeled counter.
    pub raw_tokens: u64,
    /// Provider output tokens under the same counter.
    pub optimized_tokens: u64,
    /// Known relevant evidence units in the fixture.
    pub relevant_total: u64,
    /// Relevant evidence units retained by the provider output.
    pub relevant_retained: u64,
}

impl ProviderQuality {
    /// Token reduction in basis points, clamped at zero for expansion.
    #[must_use]
    pub fn reduction_basis_points(self) -> u64 {
        if self.raw_tokens == 0 || self.optimized_tokens >= self.raw_tokens {
            0
        } else {
            (self.raw_tokens - self.optimized_tokens).saturating_mul(10_000) / self.raw_tokens
        }
    }

    /// Evidence recall in basis points.
    #[must_use]
    pub fn recall_basis_points(self) -> u64 {
        self.relevant_retained
            .min(self.relevant_total)
            .saturating_mul(10_000)
            .checked_div(self.relevant_total)
            .unwrap_or(10_000)
    }
}

/// Applies the required-provider policy or evaluates a built-in fallback.
///
/// # Errors
///
/// Returns the provider error when the failed provider is marked required.
pub fn resolve_with_fallback<T>(
    spec: &ProviderSpec,
    attempt: Result<T, ProviderError>,
    fallback_id: &str,
    fallback: impl FnOnce() -> T,
) -> Result<ProviderOutcome<T>, ProviderError> {
    match attempt {
        Ok(value) => Ok(ProviderOutcome {
            value,
            attribution: ProviderAttribution {
                provider: spec.id.clone(),
                fallback_from: None,
                warning_code: None,
            },
        }),
        Err(error) if spec.required => Err(error),
        Err(error) => Ok(ProviderOutcome {
            value: fallback(),
            attribution: ProviderAttribution {
                provider: fallback_id.to_owned(),
                fallback_from: Some(spec.id.clone()),
                warning_code: Some(error.code()),
            },
        }),
    }
}

/// Hard safety defaults shared by optional providers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderLimits {
    /// Time allowed for version and MCP initialization probes.
    pub startup_timeout: Duration,
    /// Time allowed for one provider operation.
    pub operation_timeout: Duration,
    /// Maximum bytes accepted from one response stream.
    pub max_response_bytes: usize,
    /// Consecutive failures before the circuit opens.
    pub failure_threshold: u32,
    /// Time an open circuit remains unavailable.
    pub cooldown: Duration,
}

impl Default for ProviderLimits {
    fn default() -> Self {
        Self {
            startup_timeout: Duration::from_secs(3),
            operation_timeout: Duration::from_secs(10),
            max_response_bytes: 1024 * 1024,
            failure_threshold: 3,
            cooldown: Duration::from_secs(30),
        }
    }
}

/// Exact executable configuration; arguments are never interpreted by a shell.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderSpec {
    /// Stable provider identifier used in attribution and warnings.
    pub id: String,
    /// Executable path or name resolved by the operating system.
    pub command: PathBuf,
    /// Arguments prepended to every invocation.
    pub base_args: Vec<String>,
    /// Explicit environment additions for the provider process.
    pub environment: BTreeMap<String, String>,
    /// Supported semantic-version range.
    pub version_requirement: VersionReq,
    /// Whether provider failure must fail the request instead of falling back.
    pub required: bool,
    /// Runtime resource limits.
    pub limits: ProviderLimits,
}

/// Normalized failure classes used by every optional adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderError {
    /// Executable could not be started.
    Unavailable,
    /// Provider did not respond before its deadline.
    Timeout,
    /// Provider exited unsuccessfully or disconnected.
    Crashed,
    /// Provider response violated its documented schema.
    Malformed,
    /// Provider version is outside the tested range.
    Incompatible { found: String, expected: String },
    /// Provider response exceeded the configured boundary.
    ResponseTooLarge,
    /// Repeated failures temporarily disabled the provider.
    CircuitOpen,
    /// Requested MCP capability is not advertised.
    MissingCapability(String),
}

impl ProviderError {
    /// Stable warning code suitable for public response metadata.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Unavailable => "provider-unavailable",
            Self::Timeout => "provider-timeout",
            Self::Crashed => "provider-crashed",
            Self::Malformed => "provider-malformed-response",
            Self::Incompatible { .. } => "provider-incompatible-version",
            Self::ResponseTooLarge => "provider-response-too-large",
            Self::CircuitOpen => "provider-circuit-open",
            Self::MissingCapability(_) => "provider-missing-capability",
        }
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Incompatible { found, expected } => {
                write!(
                    formatter,
                    "provider version {found} does not satisfy {expected}"
                )
            }
            Self::MissingCapability(tool) => write!(formatter, "provider does not expose {tool}"),
            other => formatter.write_str(other.code()),
        }
    }
}

impl std::error::Error for ProviderError {}

#[derive(Clone, Copy, Debug, Default)]
struct CircuitState {
    consecutive_failures: u32,
    opened_at: Option<Instant>,
}

/// Thread-safe failure circuit shared by cloned adapters.
#[derive(Clone, Debug)]
pub struct CircuitBreaker {
    state: Arc<Mutex<CircuitState>>,
    threshold: u32,
    cooldown: Duration,
}

impl CircuitBreaker {
    /// Creates a circuit with a nonzero failure threshold.
    #[must_use]
    pub fn new(threshold: u32, cooldown: Duration) -> Self {
        Self {
            state: Arc::new(Mutex::new(CircuitState::default())),
            threshold: threshold.max(1),
            cooldown,
        }
    }

    /// Rejects calls while the circuit is open, and permits a probe after cooldown.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError::CircuitOpen`] while the cooldown is active.
    pub fn permit(&self) -> Result<(), ProviderError> {
        let mut state = self.state.lock().map_err(|_| ProviderError::CircuitOpen)?;
        if let Some(opened_at) = state.opened_at {
            if opened_at.elapsed() < self.cooldown {
                return Err(ProviderError::CircuitOpen);
            }
            state.opened_at = None;
            state.consecutive_failures = 0;
        }
        Ok(())
    }

    /// Records a successful call and closes the circuit.
    pub fn success(&self) {
        if let Ok(mut state) = self.state.lock() {
            *state = CircuitState::default();
        }
    }

    /// Records a failed call and opens the circuit at the configured threshold.
    pub fn failure(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.consecutive_failures = state.consecutive_failures.saturating_add(1);
            if state.consecutive_failures >= self.threshold {
                state.opened_at = Some(Instant::now());
            }
        }
    }
}

/// Captured provider process result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessOutput {
    /// Standard output bytes.
    pub stdout: Vec<u8>,
    /// Standard error bytes.
    pub stderr: Vec<u8>,
}

/// Version-probed process provider with shared circuit-breaker state.
#[derive(Clone, Debug)]
pub struct ManagedProcess {
    spec: ProviderSpec,
    circuit: CircuitBreaker,
}

impl ManagedProcess {
    /// Creates a managed process from an exact provider specification.
    #[must_use]
    pub fn new(spec: ProviderSpec) -> Self {
        let circuit = CircuitBreaker::new(spec.limits.failure_threshold, spec.limits.cooldown);
        Self { spec, circuit }
    }

    /// Returns the immutable provider specification.
    #[must_use]
    pub const fn spec(&self) -> &ProviderSpec {
        &self.spec
    }

    /// Runs the provider's version command and validates its tested range.
    ///
    /// # Errors
    ///
    /// Returns a normalized launch, deadline, output, or compatibility error.
    pub fn probe(&self, version_args: &[String]) -> Result<Version, ProviderError> {
        self.run(version_args, &[], self.spec.limits.startup_timeout)
            .and_then(|output| {
                let text =
                    String::from_utf8(output.stdout).map_err(|_| ProviderError::Malformed)?;
                validate_version(&text, &self.spec.version_requirement)
            })
    }

    /// Executes one bounded provider operation.
    ///
    /// # Errors
    ///
    /// Returns a normalized circuit, launch, deadline, process, or output error.
    pub fn invoke(&self, args: &[String], input: &[u8]) -> Result<ProcessOutput, ProviderError> {
        self.run(args, input, self.spec.limits.operation_timeout)
    }

    fn run(
        &self,
        args: &[String],
        input: &[u8],
        timeout: Duration,
    ) -> Result<ProcessOutput, ProviderError> {
        self.circuit.permit()?;
        let result = run_process(&self.spec, args, input, timeout);
        if result.is_ok() {
            self.circuit.success();
        } else {
            self.circuit.failure();
        }
        result
    }
}

/// Runs an exact provider command within a deadline and bounded response size.
///
/// # Errors
///
/// Returns a normalized launch, deadline, process, or response-boundary error.
pub fn run_process(
    spec: &ProviderSpec,
    args: &[String],
    input: &[u8],
    timeout: Duration,
) -> Result<ProcessOutput, ProviderError> {
    let mut command = Command::new(&spec.command);
    command.args(&spec.base_args).args(args);
    command.envs(&spec.environment);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|_| ProviderError::Unavailable)?;
    if let Some(mut stdin) = child.stdin.take() {
        let input = input.to_vec();
        thread::spawn(move || {
            let _ = stdin.write_all(&input);
        });
    }
    let stdout = child.stdout.take().ok_or(ProviderError::Crashed)?;
    let stderr = child.stderr.take().ok_or(ProviderError::Crashed)?;
    let out_rx = read_bounded(stdout, spec.limits.max_response_bytes);
    let err_rx = read_bounded(stderr, spec.limits.max_response_bytes);
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let output = ProcessOutput {
                    stdout: receive_bytes(&out_rx)?,
                    stderr: receive_bytes(&err_rx)?,
                };
                return if status.success() {
                    Ok(output)
                } else {
                    Err(ProviderError::Crashed)
                };
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ProviderError::Timeout);
            }
            Err(_) => return Err(ProviderError::Crashed),
        }
    }
}

fn read_bounded(
    stream: impl Read + Send + 'static,
    limit: usize,
) -> Receiver<Result<Vec<u8>, ProviderError>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let read = stream
            .take(u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|_| ProviderError::Crashed);
        let result = match read {
            Err(error) => Err(error),
            Ok(_) if bytes.len() > limit => Err(ProviderError::ResponseTooLarge),
            Ok(_) => Ok(bytes),
        };
        let _ = sender.send(result);
    });
    receiver
}

fn receive_bytes(
    receiver: &Receiver<Result<Vec<u8>, ProviderError>>,
) -> Result<Vec<u8>, ProviderError> {
    receiver.recv().map_err(|_| ProviderError::Crashed)?
}

/// Extracts and validates the first semantic version embedded in command output.
///
/// # Errors
///
/// Returns malformed or incompatible when no supported semantic version is present.
pub fn validate_version(output: &str, requirement: &VersionReq) -> Result<Version, ProviderError> {
    let found = output
        .split_whitespace()
        .find_map(|part| Version::parse(part.trim_start_matches('v')).ok())
        .ok_or(ProviderError::Malformed)?;
    if requirement.matches(&found) {
        Ok(found)
    } else {
        Err(ProviderError::Incompatible {
            found: found.to_string(),
            expected: requirement.to_string(),
        })
    }
}

/// Parses one bounded MCP JSON-RPC response without interpreting its content.
///
/// # Errors
///
/// Returns too-large or malformed when the line exceeds the boundary or is not a JSON-RPC object.
pub fn parse_mcp_response(line: &str, max_bytes: usize) -> Result<Value, ProviderError> {
    if line.len() > max_bytes {
        return Err(ProviderError::ResponseTooLarge);
    }
    let value: Value = serde_json::from_str(line).map_err(|_| ProviderError::Malformed)?;
    if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0") || !value.is_object() {
        return Err(ProviderError::Malformed);
    }
    Ok(value)
}

/// Short-lived MCP stdio session for a single capability call.
pub struct McpSession {
    child: Child,
    requests: mpsc::Sender<Vec<u8>>,
    responses: Receiver<Result<String, ProviderError>>,
    next_id: u64,
    max_response_bytes: usize,
}

impl McpSession {
    /// Starts and initializes an MCP server, validating its advertised tool set.
    ///
    /// # Errors
    ///
    /// Returns a normalized process, protocol, version, capability, or deadline error.
    pub fn connect(spec: &ProviderSpec, required_tool: &str) -> Result<Self, ProviderError> {
        Self::connect_inner(spec, required_tool, true)
    }

    /// Starts MCP and validates a tool when package version is probed separately.
    ///
    /// # Errors
    ///
    /// Returns a normalized process, protocol, capability, or deadline error.
    pub fn connect_capability(
        spec: &ProviderSpec,
        required_tool: &str,
    ) -> Result<Self, ProviderError> {
        Self::connect_inner(spec, required_tool, false)
    }

    fn connect_inner(
        spec: &ProviderSpec,
        required_tool: &str,
        validate_server_version: bool,
    ) -> Result<Self, ProviderError> {
        let mut child = Command::new(&spec.command)
            .args(&spec.base_args)
            .envs(&spec.environment)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| ProviderError::Unavailable)?;
        let mut stdin = child.stdin.take().ok_or(ProviderError::Crashed)?;
        let (request_sender, request_receiver) = mpsc::channel::<Vec<u8>>();
        thread::spawn(move || {
            for request in request_receiver {
                if stdin.write_all(&request).is_err() || stdin.flush().is_err() {
                    break;
                }
            }
        });
        let stdout = child.stdout.take().ok_or(ProviderError::Crashed)?;
        let responses = read_lines(stdout, spec.limits.max_response_bytes);
        let mut session = Self {
            child,
            requests: request_sender,
            responses,
            next_id: 1,
            max_response_bytes: spec.limits.max_response_bytes,
        };
        let initialized = session.request(
            "initialize",
            &json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"token-shrinker","version":env!("CARGO_PKG_VERSION")}}),
            spec.limits.startup_timeout,
        )?;
        if validate_server_version {
            let version_text = initialized
                .pointer("/result/serverInfo/version")
                .and_then(Value::as_str)
                .ok_or(ProviderError::Malformed)?;
            validate_version(version_text, &spec.version_requirement)?;
        }
        session.notify("notifications/initialized", &json!({}))?;
        let tools = session.request("tools/list", &json!({}), spec.limits.startup_timeout)?;
        let exists = tools
            .pointer("/result/tools")
            .and_then(Value::as_array)
            .is_some_and(|items| {
                items
                    .iter()
                    .any(|tool| tool.get("name").and_then(Value::as_str) == Some(required_tool))
            });
        if !exists {
            return Err(ProviderError::MissingCapability(required_tool.to_owned()));
        }
        Ok(session)
    }

    /// Calls a previously probed MCP tool and returns its result object.
    ///
    /// # Errors
    ///
    /// Returns a normalized protocol, schema, provider, or deadline error.
    pub fn call_tool(
        &mut self,
        name: &str,
        arguments: &Value,
        timeout: Duration,
    ) -> Result<Value, ProviderError> {
        let response = self.request(
            "tools/call",
            &json!({"name":name,"arguments":arguments}),
            timeout,
        )?;
        if response.get("error").is_some()
            || response.pointer("/result/isError").and_then(Value::as_bool) == Some(true)
        {
            return Err(ProviderError::Crashed);
        }
        response
            .get("result")
            .cloned()
            .ok_or(ProviderError::Malformed)
    }

    fn notify(&mut self, method: &str, params: &Value) -> Result<(), ProviderError> {
        self.write_json(&json!({"jsonrpc":"2.0","method":method,"params":params}))
    }

    fn request(
        &mut self,
        method: &str,
        params: &Value,
        timeout: Duration,
    ) -> Result<Value, ProviderError> {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.write_json(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))?;
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(ProviderError::Timeout);
            }
            let line = self
                .responses
                .recv_timeout(remaining)
                .map_err(|error| match error {
                    mpsc::RecvTimeoutError::Timeout => ProviderError::Timeout,
                    mpsc::RecvTimeoutError::Disconnected => ProviderError::Crashed,
                })??;
            let value = parse_mcp_response(&line, self.max_response_bytes)?;
            if value.get("id").and_then(Value::as_u64) == Some(id) {
                return Ok(value);
            }
        }
    }

    fn write_json(&mut self, value: &Value) -> Result<(), ProviderError> {
        let mut request = serde_json::to_vec(value).map_err(|_| ProviderError::Malformed)?;
        request.push(b'\n');
        if request.len() > self.max_response_bytes {
            return Err(ProviderError::ResponseTooLarge);
        }
        self.requests
            .send(request)
            .map_err(|_| ProviderError::Crashed)
    }
}

impl Drop for McpSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn read_lines(
    stream: impl Read + Send + 'static,
    limit: usize,
) -> Receiver<Result<String, ProviderError>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        loop {
            let mut line = String::new();
            let read = reader
                .by_ref()
                .take(u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1))
                .read_line(&mut line);
            match read {
                Ok(0) => break,
                Ok(_) if line.len() > limit => {
                    let _ = sender.send(Err(ProviderError::ResponseTooLarge));
                    break;
                }
                Ok(_) => {
                    if sender.send(Ok(line)).is_err() {
                        break;
                    }
                }
                Err(_) => {
                    let _ = sender.send(Err(ProviderError::Crashed));
                    break;
                }
            }
        }
    });
    receiver
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fake_spec(mode: &str) -> ProviderSpec {
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("fake_provider.py");
        ProviderSpec {
            id: "fake".to_owned(),
            command: PathBuf::from("python"),
            base_args: vec![script.to_string_lossy().into_owned(), mode.to_owned()],
            environment: BTreeMap::new(),
            version_requirement: VersionReq::parse(">=1.0.0, <2.0.0").expect("requirement"),
            required: false,
            limits: ProviderLimits {
                startup_timeout: Duration::from_millis(500),
                operation_timeout: Duration::from_millis(100),
                max_response_bytes: 1_024,
                failure_threshold: 2,
                cooldown: Duration::from_mins(1),
            },
        }
    }

    #[test]
    fn version_validation_accepts_embedded_semver() {
        let requirement = VersionReq::parse(">=0.9.0, <1.0.0").expect("requirement");
        let version = validate_version("graphify 0.9.35", &requirement).expect("version");
        assert_eq!(version, Version::new(0, 9, 35));
    }

    #[test]
    fn version_validation_rejects_incompatible_semver() {
        let requirement = VersionReq::parse(">=1.0.0, <2.0.0").expect("requirement");
        assert!(matches!(
            validate_version("tool v2.0.0", &requirement),
            Err(ProviderError::Incompatible { .. })
        ));
    }

    #[test]
    fn malformed_probe_output_is_rejected_and_can_fallback() {
        let requirement = VersionReq::parse(">=1.0.0").expect("requirement");
        assert_eq!(
            validate_version("not a version", &requirement),
            Err(ProviderError::Malformed)
        );
        let spec = fake_spec("version");
        let outcome =
            resolve_with_fallback(&spec, Err(ProviderError::Malformed), "builtin", || "safe")
                .expect("fallback");
        assert_eq!(outcome.value, "safe");
    }

    #[test]
    fn mcp_parser_rejects_malformed_and_oversized_corpus() {
        assert!(parse_mcp_response(r#"{"jsonrpc":"2.0","id":1,"result":{}}"#, 128).is_ok());
        assert_eq!(
            parse_mcp_response("not-json", 128),
            Err(ProviderError::Malformed)
        );
        assert_eq!(
            parse_mcp_response(r#"{"jsonrpc":"2.0"}"#, 4),
            Err(ProviderError::ResponseTooLarge)
        );
    }

    #[test]
    fn checked_in_fuzz_corpus_never_panics() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus");
        let requirement = VersionReq::parse(">=1.0.0, <2.0.0").expect("requirement");

        for entry in fs::read_dir(root.join("provider_version")).expect("version corpus") {
            let bytes = fs::read(entry.expect("corpus entry").path()).expect("corpus bytes");
            let input = String::from_utf8_lossy(&bytes);
            let _ = validate_version(&input, &requirement);
        }
        for entry in fs::read_dir(root.join("provider_response")).expect("response corpus") {
            let bytes = fs::read(entry.expect("corpus entry").path()).expect("corpus bytes");
            let input = String::from_utf8_lossy(&bytes);
            let _ = parse_mcp_response(&input, 1_024);
        }
    }

    #[test]
    fn circuit_opens_and_recovers_after_cooldown() {
        let circuit = CircuitBreaker::new(2, Duration::ZERO);
        circuit.failure();
        assert_eq!(circuit.permit(), Ok(()));
        circuit.failure();
        assert_eq!(circuit.permit(), Ok(()));
    }

    #[test]
    fn error_codes_are_stable() {
        assert_eq!(ProviderError::Timeout.code(), "provider-timeout");
        assert_eq!(
            ProviderError::Malformed.code(),
            "provider-malformed-response"
        );
    }

    #[test]
    fn optional_failure_falls_back_but_required_failure_does_not() {
        let optional = fake_spec("crash");
        let outcome = resolve_with_fallback(
            &optional,
            Err(ProviderError::Crashed),
            "builtin",
            || "fallback",
        )
        .expect("optional fallback");
        assert_eq!(outcome.value, "fallback");
        assert_eq!(outcome.attribution.provider, "builtin");
        assert_eq!(outcome.attribution.warning_code, Some("provider-crashed"));

        let mut required = optional;
        required.required = true;
        assert_eq!(
            resolve_with_fallback(
                &required,
                Err(ProviderError::Timeout),
                "builtin",
                || "fallback"
            ),
            Err(ProviderError::Timeout)
        );
    }

    #[test]
    fn every_contract_failure_class_falls_back_when_optional() {
        let spec = fake_spec("version");
        let failures = [
            ProviderError::Unavailable,
            ProviderError::Timeout,
            ProviderError::Crashed,
            ProviderError::Malformed,
            ProviderError::Incompatible {
                found: "2.0.0".to_owned(),
                expected: "<2.0.0".to_owned(),
            },
            ProviderError::ResponseTooLarge,
            ProviderError::CircuitOpen,
            ProviderError::MissingCapability("search".to_owned()),
        ];
        for failure in failures {
            let expected_code = failure.code();
            let outcome = resolve_with_fallback(&spec, Err(failure), "builtin", || 7)
                .expect("optional fallback");
            assert_eq!(outcome.value, 7);
            assert_eq!(outcome.attribution.warning_code, Some(expected_code));
        }
    }

    #[test]
    fn quality_metrics_use_comparable_counts() {
        let quality = ProviderQuality {
            raw_tokens: 1_000,
            optimized_tokens: 400,
            relevant_total: 5,
            relevant_retained: 4,
        };
        assert_eq!(quality.reduction_basis_points(), 6_000);
        assert_eq!(quality.recall_basis_points(), 8_000);
    }

    #[test]
    fn fake_process_contract_covers_success_and_failure_classes() {
        assert_eq!(
            ManagedProcess::new(fake_spec("version"))
                .probe(&[])
                .expect("probe"),
            Version::new(1, 2, 3)
        );
        assert_eq!(
            ManagedProcess::new(fake_spec("echo"))
                .invoke(&[], b"hello")
                .expect("echo")
                .stdout,
            b"hello"
        );
        assert_eq!(
            ManagedProcess::new(fake_spec("sleep")).invoke(&[], &[]),
            Err(ProviderError::Timeout)
        );
        assert_eq!(
            ManagedProcess::new(fake_spec("crash")).invoke(&[], &[]),
            Err(ProviderError::Crashed)
        );
        assert_eq!(
            ManagedProcess::new(fake_spec("large")).invoke(&[], &[]),
            Err(ProviderError::ResponseTooLarge)
        );
        assert!(matches!(
            ManagedProcess::new(fake_spec("old-version")).probe(&[]),
            Err(ProviderError::Incompatible { .. })
        ));
    }

    #[test]
    fn fake_process_opens_circuit_after_repeated_failures() {
        let provider = ManagedProcess::new(fake_spec("crash"));
        assert_eq!(provider.invoke(&[], &[]), Err(ProviderError::Crashed));
        assert_eq!(provider.invoke(&[], &[]), Err(ProviderError::Crashed));
        assert_eq!(provider.invoke(&[], &[]), Err(ProviderError::CircuitOpen));
    }

    #[test]
    fn fake_mcp_contract_validates_capability_and_call() {
        let mut session =
            McpSession::connect(&fake_spec("mcp"), "fake_search").expect("MCP connection");
        let result = session
            .call_tool(
                "fake_search",
                &json!({"query":"safe"}),
                Duration::from_secs(1),
            )
            .expect("tool call");
        assert_eq!(result["content"][0]["type"], "text");
        drop(session);
        assert!(matches!(
            McpSession::connect(&fake_spec("mcp"), "missing"),
            Err(ProviderError::MissingCapability(tool)) if tool == "missing"
        ));
    }
}
