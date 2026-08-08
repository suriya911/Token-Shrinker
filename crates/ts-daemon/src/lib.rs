//! Shared service composition for stdio, one-shot, and future daemon modes.

use std::path::Path;
use token_shrinker_context::{ConservativeEstimator, ContextBundle, build_context};
use token_shrinker_repo::{
    RepositoryError, RepositoryProvider, RepositoryQuery, RepositoryTrace, ScanWarning,
};
use token_shrinker_types::TokenBudget;

/// Observable result of the dependency-free native context baseline.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BaselineContext {
    /// Deterministically ranked and budgeted context sent to the next pipeline stage.
    pub bundle: ContextBundle,
    /// Non-fatal repository omissions, without source contents.
    pub warnings: Vec<ScanWarning>,
    /// Content-free repository counters.
    pub repository_trace: RepositoryTrace,
}

/// Builds context using only the native repository provider and conservative token estimator.
///
/// This path intentionally does not invoke optional symbol indexes, external search tools, or an
/// LLM. It is the deterministic fallback that later provider layers may enrich.
///
/// # Errors
///
/// Returns [`RepositoryError`] when the allowed root cannot be opened or scanned.
pub fn build_baseline_context(
    root: impl AsRef<Path>,
    query: &str,
    budget: TokenBudget,
) -> Result<BaselineContext, RepositoryError> {
    let terms = query_terms(query);
    let provider = RepositoryProvider::open(root)?;
    let scan = provider.scan(&RepositoryQuery {
        path_hints: terms.clone(),
        terms,
    })?;
    let bundle = build_context(&scan.candidates, budget, &ConservativeEstimator);
    Ok(BaselineContext {
        bundle,
        warnings: scan.warnings,
        repository_trace: scan.trace,
    })
}

fn query_terms(query: &str) -> Vec<String> {
    let mut terms = query
        .split(|character: char| !character.is_alphanumeric() && character != '_')
        .filter(|term| term.chars().count() >= 2)
        .map(str::to_lowercase)
        .collect::<Vec<_>>();
    terms.sort();
    terms.dedup();
    terms
}

use fs4::FileExt;
#[cfg(unix)]
use interprocess::local_socket::{GenericFilePath, ToFsName};
#[cfg(windows)]
use interprocess::local_socket::{GenericNamespaced, ToNsName};
use interprocess::local_socket::{
    Listener, ListenerNonblockingMode, ListenerOptions, Name, Stream, prelude::*,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fmt, fs,
    fs::{File, OpenOptions},
    io::{self, Write},
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use token_shrinker_protocol::{ProtocolError, RpcRequest, RpcResponse, read_frame, write_frame};
use token_shrinker_types::{ProtocolVersion, RequestId};

/// Health state for one required or optional capability.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthState {
    /// Capability is operating through the selected provider.
    Healthy,
    /// Capability is operating through a documented fallback.
    Degraded,
    /// Required capability is unavailable.
    Failed,
}

/// Content-free capability selection record.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capability {
    /// Stable capability identifier.
    pub id: String,
    /// Selected provider identifier.
    pub provider: String,
    /// Built-in fallback identifier, when applicable.
    pub fallback: Option<String>,
    /// Current health.
    pub health: HealthState,
    /// Stable warning code, never provider output.
    pub warning_code: Option<String>,
}

/// Deterministically ordered capability registry.
#[derive(Clone, Debug, Default)]
pub struct CapabilityRegistry {
    capabilities: BTreeMap<String, Capability>,
}

impl CapabilityRegistry {
    /// Registers a healthy provider or its built-in fallback.
    pub fn register(
        &mut self,
        id: impl Into<String>,
        preferred_provider: impl Into<String>,
        preferred_healthy: bool,
        fallback: impl Into<String>,
        required: bool,
    ) {
        let id = id.into();
        let preferred = preferred_provider.into();
        let fallback = fallback.into();
        let capability = if preferred_healthy {
            Capability {
                id: id.clone(),
                provider: preferred,
                fallback: Some(fallback),
                health: HealthState::Healthy,
                warning_code: None,
            }
        } else if required {
            Capability {
                id: id.clone(),
                provider: preferred,
                fallback: Some(fallback),
                health: HealthState::Failed,
                warning_code: Some("required-provider-unavailable".to_owned()),
            }
        } else {
            Capability {
                id: id.clone(),
                provider: fallback.clone(),
                fallback: Some(fallback),
                health: HealthState::Degraded,
                warning_code: Some("optional-provider-fallback".to_owned()),
            }
        };
        self.capabilities.insert(id, capability);
    }

    /// Returns capabilities in stable identifier order.
    #[must_use]
    pub fn list(&self) -> Vec<Capability> {
        self.capabilities.values().cloned().collect()
    }

    /// Returns overall health, where failed dominates degraded.
    #[must_use]
    pub fn overall_health(&self) -> HealthState {
        if self
            .capabilities
            .values()
            .any(|item| item.health == HealthState::Failed)
        {
            HealthState::Failed
        } else if self
            .capabilities
            .values()
            .any(|item| item.health == HealthState::Degraded)
        {
            HealthState::Degraded
        } else {
            HealthState::Healthy
        }
    }
}

/// In-process request handler used by one-shot and IPC modes.
pub trait RpcHandler: Send + Sync + 'static {
    /// Handles one validated request.
    ///
    /// # Errors
    ///
    /// Returns [`ServiceError`] for unsupported methods or internal service failure.
    fn handle(&self, request: &RpcRequest, cancelled: &AtomicBool) -> Result<Value, ServiceError>;
}

/// Minimal built-in service handler used until M4 registers CLI/MCP methods.
#[derive(Clone, Debug)]
pub struct BuiltInHandler {
    registry: CapabilityRegistry,
}

impl BuiltInHandler {
    /// Creates the baseline service handler.
    #[must_use]
    pub const fn new(registry: CapabilityRegistry) -> Self {
        Self { registry }
    }
}

impl RpcHandler for BuiltInHandler {
    fn handle(&self, request: &RpcRequest, _cancelled: &AtomicBool) -> Result<Value, ServiceError> {
        match request.method.as_str() {
            "health" => {
                serde_json::to_value(self.registry.overall_health()).map_err(ServiceError::Json)
            }
            "capabilities" => {
                serde_json::to_value(self.registry.list()).map_err(ServiceError::Json)
            }
            "echo" => Ok(request.params.clone()),
            _ => Err(ServiceError::MethodNotFound),
        }
    }
}

#[derive(Debug)]
struct RequestGate {
    active: Mutex<usize>,
    changed: Condvar,
    maximum: usize,
}

impl RequestGate {
    fn enter(self: &Arc<Self>, shutting_down: &AtomicBool) -> Result<RequestPermit, ServiceError> {
        let mut active = self.active.lock().map_err(|_| ServiceError::Poisoned)?;
        while *active >= self.maximum && !shutting_down.load(Ordering::Acquire) {
            active = self
                .changed
                .wait(active)
                .map_err(|_| ServiceError::Poisoned)?;
        }
        if shutting_down.load(Ordering::Acquire) {
            return Err(ServiceError::ShuttingDown);
        }
        *active += 1;
        Ok(RequestPermit {
            gate: Arc::clone(self),
        })
    }
}

struct RequestPermit {
    gate: Arc<RequestGate>,
}
impl Drop for RequestPermit {
    fn drop(&mut self) {
        if let Ok(mut active) = self.gate.active.lock() {
            *active = active.saturating_sub(1);
            self.gate.changed.notify_one();
        }
    }
}

/// Capability-aware bounded service graph shared by runtime modes.
pub struct ServiceGraph {
    handler: Arc<dyn RpcHandler>,
    gate: Arc<RequestGate>,
    shutting_down: AtomicBool,
    cancellations: Mutex<BTreeMap<String, Arc<AtomicBool>>>,
}

impl fmt::Debug for ServiceGraph {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ServiceGraph")
            .field("maximum_concurrency", &self.gate.maximum)
            .field("shutting_down", &self.shutting_down.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl ServiceGraph {
    /// Creates a bounded graph.
    ///
    /// # Errors
    ///
    /// Returns [`ServiceError`] when maximum concurrency is zero.
    pub fn new(
        handler: Arc<dyn RpcHandler>,
        maximum_concurrency: usize,
    ) -> Result<Self, ServiceError> {
        if maximum_concurrency == 0 {
            return Err(ServiceError::ZeroConcurrency);
        }
        Ok(Self {
            handler,
            gate: Arc::new(RequestGate {
                active: Mutex::new(0),
                changed: Condvar::new(),
                maximum: maximum_concurrency,
            }),
            shutting_down: AtomicBool::new(false),
            cancellations: Mutex::new(BTreeMap::new()),
        })
    }

    fn dispatch(&self, request: &RpcRequest) -> Result<Value, ServiceError> {
        let _permit = self.gate.enter(&self.shutting_down)?;
        let token = Arc::new(AtomicBool::new(false));
        {
            let mut cancellations = self
                .cancellations
                .lock()
                .map_err(|_| ServiceError::Poisoned)?;
            if cancellations
                .insert(request.id.as_str().to_owned(), Arc::clone(&token))
                .is_some()
            {
                return Err(ServiceError::DuplicateRequest);
            }
        }
        let deadline_guard = if let Some(deadline) = request.deadline_unix_ms {
            let now = unix_millis().map_err(|_| ServiceError::Internal("clock"))?;
            if deadline <= now {
                token.store(true, Ordering::Release);
                None
            } else {
                let wait = Duration::from_millis(u64::try_from(deadline - now).unwrap_or(u64::MAX));
                let deadline_token = Arc::clone(&token);
                let (sender, receiver) = std::sync::mpsc::channel::<()>();
                thread::spawn(move || {
                    if matches!(
                        receiver.recv_timeout(wait),
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                    ) {
                        deadline_token.store(true, Ordering::Release);
                    }
                });
                Some(sender)
            }
        } else {
            None
        };
        let result = self.handler.handle(request, &token);
        drop(deadline_guard);
        if let Ok(mut cancellations) = self.cancellations.lock() {
            cancellations.remove(request.id.as_str());
        }
        result
    }

    fn cancel(&self, request_id: &str) -> Result<bool, ServiceError> {
        let cancellations = self
            .cancellations
            .lock()
            .map_err(|_| ServiceError::Poisoned)?;
        Ok(cancellations.get(request_id).is_some_and(|token| {
            token.store(true, Ordering::Release);
            true
        }))
    }

    fn begin_shutdown(&self) {
        self.shutting_down.store(true, Ordering::Release);
        self.gate.changed.notify_all();
    }
}

/// Local daemon configuration.
#[derive(Clone, Debug)]
pub struct DaemonConfig {
    /// User-only runtime directory.
    pub runtime_directory: PathBuf,
    /// Maximum JSON frame bytes.
    pub max_frame_bytes: u32,
    /// Maximum concurrent service requests.
    pub max_concurrency: usize,
    /// Maximum time to wait for worker drain.
    pub graceful_shutdown_timeout: Duration,
    /// Content-free log rotation threshold.
    pub max_log_bytes: u64,
}

/// User-only discovery record.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiscoveryState {
    /// Platform local-socket or named-pipe name.
    pub endpoint: String,
    /// Owning daemon process.
    pub pid: u32,
    /// Unix start time in milliseconds.
    pub started_at_ms: i64,
    /// Server protocol version.
    pub protocol_version: ProtocolVersion,
    /// Ephemeral 256-bit authentication token.
    pub auth_token: String,
}

impl fmt::Debug for DiscoveryState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DiscoveryState")
            .field("endpoint", &self.endpoint)
            .field("pid", &self.pid)
            .field("started_at_ms", &self.started_at_ms)
            .field("protocol_version", &self.protocol_version)
            .field("auth_token", &"[REDACTED]")
            .finish()
    }
}

/// Running daemon handle retaining the single-instance lock.
pub struct DaemonHandle {
    discovery: DiscoveryState,
    discovery_path: PathBuf,
    graph: Arc<ServiceGraph>,
    thread: Option<thread::JoinHandle<()>>,
    _lock: File,
    max_frame_bytes: u32,
}

impl fmt::Debug for DaemonHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DaemonHandle")
            .field("discovery", &self.discovery)
            .finish_non_exhaustive()
    }
}

impl DaemonHandle {
    /// Starts one user-local daemon and writes discovery only after readiness.
    ///
    /// # Errors
    ///
    /// Returns [`DaemonError`] for unsafe configuration, an active instance, or startup failure.
    pub fn start(config: &DaemonConfig, handler: Arc<dyn RpcHandler>) -> Result<Self, DaemonError> {
        validate_daemon_config(config)?;
        secure_runtime_directory(&config.runtime_directory)?;
        let lock_path = config
            .runtime_directory
            .join(format!("daemon-v{}.lock", ProtocolVersion::CURRENT.major));
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(DaemonError::Io)?;
        FileExt::try_lock(&lock).map_err(|_| DaemonError::AlreadyRunning)?;
        let discovery_path = config.runtime_directory.join("daemon.json");
        if discovery_path.exists() {
            fs::remove_file(&discovery_path).map_err(DaemonError::Io)?;
        }
        rotate_log(
            &config.runtime_directory.join("daemon.log"),
            config.max_log_bytes,
        )?;

        let started_at_ms = unix_millis()?;
        let auth_token = auth_token()?;
        let endpoint = endpoint_text(&config.runtime_directory, started_at_ms, &auth_token[..16]);
        let name = endpoint_name(&endpoint)?;
        let listener = create_listener(name)?;
        secure_socket_file(&endpoint)?;
        let discovery = DiscoveryState {
            endpoint,
            pid: std::process::id(),
            started_at_ms,
            protocol_version: ProtocolVersion::CURRENT,
            auth_token,
        };
        write_discovery(&discovery_path, &discovery)?;
        append_log(&config.runtime_directory.join("daemon.log"), "ready")?;

        let graph = Arc::new(
            ServiceGraph::new(handler, config.max_concurrency).map_err(DaemonError::Service)?,
        );
        let thread_graph = Arc::clone(&graph);
        let thread_discovery = discovery.clone();
        let max_frame_bytes = config.max_frame_bytes;
        let shutdown_timeout = config.graceful_shutdown_timeout;
        let server_thread = thread::spawn(move || {
            server_loop(
                listener,
                thread_graph,
                thread_discovery,
                max_frame_bytes,
                shutdown_timeout,
            );
        });
        Ok(Self {
            discovery,
            discovery_path,
            graph,
            thread: Some(server_thread),
            _lock: lock,
            max_frame_bytes,
        })
    }

    /// Returns the ready discovery record.
    #[must_use]
    pub const fn discovery(&self) -> &DiscoveryState {
        &self.discovery
    }

    /// Sends one request through the actual local transport.
    ///
    /// # Errors
    ///
    /// Returns [`DaemonError`] for connection, framing, or structured server failure.
    pub fn call(&self, method: &str, params: Value) -> Result<Value, DaemonError> {
        daemon_call(&self.discovery, self.max_frame_bytes, method, params)
    }

    /// Requests graceful shutdown and waits for the listener thread.
    ///
    /// # Errors
    ///
    /// Returns [`DaemonError`] when shutdown transport or joining fails.
    pub fn shutdown(mut self) -> Result<(), DaemonError> {
        let _ = self.call("daemon.shutdown", Value::Null)?;
        self.graph.begin_shutdown();
        if let Some(thread) = self.thread.take() {
            thread.join().map_err(|_| DaemonError::ServerThread)?;
        }
        if self.discovery_path.exists() {
            fs::remove_file(&self.discovery_path).map_err(DaemonError::Io)?;
        }
        Ok(())
    }

    /// Waits for a remote shutdown request and then removes discovery state.
    ///
    /// This is used by foreground daemon processes whose lifecycle is controlled through the
    /// authenticated local transport.
    ///
    /// # Errors
    ///
    /// Returns [`DaemonError`] if the listener thread panics or discovery cleanup fails.
    pub fn wait(mut self) -> Result<(), DaemonError> {
        if let Some(thread) = self.thread.take() {
            thread.join().map_err(|_| DaemonError::ServerThread)?;
        }
        if self.discovery_path.exists() {
            fs::remove_file(&self.discovery_path).map_err(DaemonError::Io)?;
        }
        Ok(())
    }
}

impl Drop for DaemonHandle {
    fn drop(&mut self) {
        if self.thread.is_none() {
            return;
        }
        let _ = daemon_call(
            &self.discovery,
            self.max_frame_bytes,
            "daemon.shutdown",
            Value::Null,
        );
        self.graph.begin_shutdown();
        if let Some(server_thread) = self.thread.take() {
            let _ = server_thread.join();
        }
        if self.discovery_path.exists() {
            let _ = fs::remove_file(&self.discovery_path);
        }
    }
}

#[allow(clippy::needless_pass_by_value)]
fn server_loop(
    listener: Listener,
    graph: Arc<ServiceGraph>,
    discovery: DiscoveryState,
    max_frame_bytes: u32,
    shutdown_timeout: Duration,
) {
    let workers = Arc::new(Mutex::new(Vec::<thread::JoinHandle<()>>::new()));
    while !graph.shutting_down.load(Ordering::Acquire) {
        match listener.accept() {
            Ok(stream) => {
                let graph = Arc::clone(&graph);
                let auth = discovery.auth_token.clone();
                let worker =
                    thread::spawn(move || handle_connection(stream, graph, &auth, max_frame_bytes));
                if let Ok(mut worker_list) = workers.lock() {
                    worker_list.push(worker);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5));
            }
            Err(_) => break,
        }
    }
    graph.begin_shutdown();
    let deadline = std::time::Instant::now() + shutdown_timeout;
    if let Ok(mut worker_list) = workers.lock() {
        while !worker_list.is_empty() && std::time::Instant::now() < deadline {
            let mut index = 0;
            while index < worker_list.len() {
                if worker_list[index].is_finished() {
                    let worker = worker_list.swap_remove(index);
                    let _ = worker.join();
                } else {
                    index += 1;
                }
            }
            if !worker_list.is_empty() {
                thread::sleep(Duration::from_millis(2));
            }
        }
    }
}

#[allow(clippy::needless_pass_by_value)]
fn handle_connection(
    mut stream: Stream,
    graph: Arc<ServiceGraph>,
    auth: &str,
    max_frame_bytes: u32,
) {
    let Ok(request) = read_frame::<RpcRequest>(&mut stream, max_frame_bytes) else {
        return;
    };
    let response = if let Err(error) = request.validate(auth) {
        RpcResponse::failure(
            request.id.clone(),
            -32001,
            "request rejected",
            Some(protocol_error_code(&error).to_owned()),
        )
    } else if request.method == "daemon.shutdown" {
        graph.begin_shutdown();
        RpcResponse::success(request.id.clone(), json!({"status":"shutting_down"}))
    } else if request.method == "daemon.cancel" {
        let target = request.params.get("requestId").and_then(Value::as_str);
        match target {
            Some(target) => match graph.cancel(target) {
                Ok(cancelled) => {
                    RpcResponse::success(request.id.clone(), json!({"cancelled":cancelled}))
                }
                Err(error) => RpcResponse::failure(
                    request.id.clone(),
                    error.code(),
                    error.to_string(),
                    Some(error.data_code().to_owned()),
                ),
            },
            None => RpcResponse::failure(
                request.id.clone(),
                -32602,
                "invalid cancellation request",
                Some("invalid-params".to_owned()),
            ),
        }
    } else if request
        .deadline_unix_ms
        .is_some_and(|deadline| unix_millis().is_ok_and(|now| now > deadline))
    {
        RpcResponse::failure(
            request.id.clone(),
            -32002,
            "request deadline exceeded",
            Some("deadline-exceeded".to_owned()),
        )
    } else {
        match graph.dispatch(&request) {
            Ok(result) => RpcResponse::success(request.id.clone(), result),
            Err(error) => RpcResponse::failure(
                request.id.clone(),
                error.code(),
                error.to_string(),
                Some(error.data_code().to_owned()),
            ),
        }
    };
    let _ = write_frame(&mut stream, &response, max_frame_bytes);
}

/// Calls a daemon using only a discovery record.
///
/// # Errors
///
/// Returns [`DaemonError`] for endpoint, framing, validation, or remote error failure.
pub fn daemon_call(
    discovery: &DiscoveryState,
    max_frame_bytes: u32,
    method: &str,
    params: Value,
) -> Result<Value, DaemonError> {
    let name = endpoint_name(&discovery.endpoint)?;
    let mut stream = Stream::connect(name).map_err(DaemonError::Io)?;
    let request = RpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: RequestId::new(random_request_id()?).map_err(|_| DaemonError::RequestId)?,
        protocol_version: ProtocolVersion::CURRENT,
        auth_token: discovery.auth_token.clone(),
        method: method.to_owned(),
        params,
        deadline_unix_ms: None,
    };
    write_frame(&mut stream, &request, max_frame_bytes).map_err(DaemonError::Protocol)?;
    let response: RpcResponse =
        read_frame(&mut stream, max_frame_bytes).map_err(DaemonError::Protocol)?;
    if !response.is_valid() {
        return Err(DaemonError::InvalidResponse);
    }
    match (response.result, response.error) {
        (Some(result), None) => Ok(result),
        (None, Some(error)) => Err(DaemonError::Remote(error.code, error.data_code)),
        _ => Err(DaemonError::InvalidResponse),
    }
}

fn validate_daemon_config(config: &DaemonConfig) -> Result<(), DaemonError> {
    if config.max_frame_bytes == 0
        || config.max_concurrency == 0
        || config.graceful_shutdown_timeout.is_zero()
        || config.max_log_bytes == 0
    {
        Err(DaemonError::ZeroLimit)
    } else {
        Ok(())
    }
}

fn endpoint_text(runtime: &Path, started_at_ms: i64, nonce: &str) -> String {
    #[cfg(windows)]
    {
        let _ = runtime;
        format!(
            "token-shrinker-{}-{started_at_ms}-{nonce}",
            std::process::id()
        )
    }
    #[cfg(unix)]
    {
        let _ = (started_at_ms, nonce);
        runtime.join("daemon.sock").to_string_lossy().into_owned()
    }
}

fn endpoint_name(endpoint: &str) -> Result<Name<'static>, DaemonError> {
    #[cfg(windows)]
    {
        endpoint
            .to_owned()
            .to_ns_name::<GenericNamespaced>()
            .map_err(DaemonError::Io)
    }
    #[cfg(unix)]
    {
        PathBuf::from(endpoint)
            .to_fs_name::<GenericFilePath>()
            .map_err(DaemonError::Io)
    }
}

fn create_listener(name: Name<'static>) -> Result<Listener, DaemonError> {
    let options = ListenerOptions::new()
        .name(name)
        .nonblocking(ListenerNonblockingMode::Accept);
    #[cfg(windows)]
    let options = {
        use interprocess::os::windows::{
            local_socket::ListenerOptionsExt, security_descriptor::SecurityDescriptor,
        };
        let sddl = widestring::U16CString::from_str("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;OW)")
            .map_err(|error| DaemonError::Security(error.to_string()))?;
        let descriptor =
            SecurityDescriptor::deserialize(sddl.as_ucstr()).map_err(DaemonError::Io)?;
        options.security_descriptor(descriptor)
    };
    options.create_sync().map_err(DaemonError::Io)
}

fn secure_runtime_directory(path: &Path) -> Result<(), DaemonError> {
    fs::create_dir_all(path).map_err(DaemonError::Io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(DaemonError::Io)?;
    }
    Ok(())
}

#[allow(clippy::unnecessary_wraps)]
fn secure_socket_file(endpoint: &str) -> Result<(), DaemonError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(endpoint, fs::Permissions::from_mode(0o600))
            .map_err(DaemonError::Io)?;
    }
    let _ = endpoint;
    Ok(())
}

fn write_discovery(path: &Path, state: &DiscoveryState) -> Result<(), DaemonError> {
    let bytes = serde_json::to_vec(state).map_err(DaemonError::Json)?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, bytes).map_err(DaemonError::Io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))
            .map_err(DaemonError::Io)?;
    }
    fs::rename(temporary, path).map_err(DaemonError::Io)?;
    Ok(())
}

fn auth_token() -> Result<String, DaemonError> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(|error| DaemonError::Security(error.to_string()))?;
    Ok(hex_digest(bytes))
}

fn random_request_id() -> Result<String, DaemonError> {
    let mut bytes = [0_u8; 12];
    getrandom::fill(&mut bytes).map_err(|error| DaemonError::Security(error.to_string()))?;
    Ok(format!("client-{}", hex_digest(bytes)))
}

fn hex_digest(digest: impl AsRef<[u8]>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(digest.as_ref().len() * 2);
    for &byte in digest.as_ref() {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn unix_millis() -> Result<i64, DaemonError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| DaemonError::Clock)?
        .as_millis();
    i64::try_from(millis).map_err(|_| DaemonError::Clock)
}

fn rotate_log(path: &Path, max_bytes: u64) -> Result<(), DaemonError> {
    if path
        .metadata()
        .is_ok_and(|metadata| metadata.len() >= max_bytes)
    {
        let rotated = path.with_extension("log.1");
        if rotated.exists() {
            fs::remove_file(&rotated).map_err(DaemonError::Io)?;
        }
        fs::rename(path, rotated).map_err(DaemonError::Io)?;
    }
    Ok(())
}

fn append_log(path: &Path, event: &str) -> Result<(), DaemonError> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(DaemonError::Io)?;
    writeln!(file, "{} {event}", unix_millis()?).map_err(DaemonError::Io)
}

fn protocol_error_code(error: &ProtocolError) -> &'static str {
    match error {
        ProtocolError::InvalidJsonRpc => "invalid-jsonrpc",
        ProtocolError::IncompatibleVersion(_) => "incompatible-version",
        ProtocolError::Authentication => "authentication",
        ProtocolError::InvalidMethod => "invalid-method",
        ProtocolError::FrameTooLarge(_) => "frame-too-large",
        ProtocolError::Json(_) => "invalid-json",
        ProtocolError::Io(_) => "io",
    }
}

/// Service dispatch failure.
#[derive(Debug)]
pub enum ServiceError {
    ZeroConcurrency,
    MethodNotFound,
    ShuttingDown,
    Poisoned,
    DuplicateRequest,
    Json(serde_json::Error),
    Internal(&'static str),
}
impl ServiceError {
    const fn code(&self) -> i32 {
        match self {
            Self::MethodNotFound => -32601,
            Self::ShuttingDown => -32003,
            Self::DuplicateRequest => -32004,
            _ => -32603,
        }
    }
    const fn data_code(&self) -> &'static str {
        match self {
            Self::ZeroConcurrency => "zero-concurrency",
            Self::MethodNotFound => "method-not-found",
            Self::ShuttingDown => "shutting-down",
            Self::Poisoned => "lock-poisoned",
            Self::DuplicateRequest => "duplicate-request",
            Self::Json(_) => "json",
            Self::Internal(code) => code,
        }
    }
}
impl fmt::Display for ServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ZeroConcurrency => "service concurrency must be positive",
            Self::MethodNotFound => "method not found",
            Self::ShuttingDown => "service is shutting down",
            Self::Poisoned => "service synchronization failed",
            Self::DuplicateRequest => "duplicate request ID",
            Self::Json(_) => "service serialization failed",
            Self::Internal(_) => "service failed",
        })
    }
}
impl std::error::Error for ServiceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            _ => None,
        }
    }
}

/// Daemon lifecycle, transport, or remote failure.
#[derive(Debug)]
pub enum DaemonError {
    ZeroLimit,
    AlreadyRunning,
    Clock,
    RequestId,
    InvalidResponse,
    ServerThread,
    Security(String),
    Remote(i32, Option<String>),
    Io(io::Error),
    Json(serde_json::Error),
    Protocol(ProtocolError),
    Service(ServiceError),
}
impl fmt::Display for DaemonError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroLimit => formatter.write_str("daemon limits must be positive"),
            Self::AlreadyRunning => {
                formatter.write_str("daemon already running for this user and protocol major")
            }
            Self::Clock => formatter.write_str("system clock is invalid"),
            Self::RequestId => formatter.write_str("cannot create request ID"),
            Self::InvalidResponse => formatter.write_str("daemon returned invalid response"),
            Self::ServerThread => formatter.write_str("daemon server thread panicked"),
            Self::Security(error) => {
                write!(formatter, "daemon security configuration failed: {error}")
            }
            Self::Remote(code, data) => {
                write!(formatter, "daemon request failed ({code}, {data:?})")
            }
            Self::Io(error) => write!(formatter, "daemon I/O failed: {error}"),
            Self::Json(error) => write!(formatter, "daemon JSON failed: {error}"),
            Self::Protocol(error) => write!(formatter, "daemon protocol failed: {error}"),
            Self::Service(error) => write!(formatter, "daemon service failed: {error}"),
        }
    }
}
impl std::error::Error for DaemonError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Protocol(error) => Some(error),
            Self::Service(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use token_shrinker_context::Sensitivity;

    fn fixture_root() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("fixtures/demo-repo")
    }

    #[test]
    fn baseline_builds_ranked_context_without_optional_tools() {
        let budget = TokenBudget::from_u32(300).expect("positive test budget");
        let first =
            build_baseline_context(fixture_root(), "fix session authorization policy", budget)
                .expect("build native baseline");
        let second =
            build_baseline_context(fixture_root(), "fix session authorization policy", budget)
                .expect("rebuild native baseline");

        assert!(!first.bundle.items.is_empty());
        assert!(first.bundle.trace.used_tokens <= u64::from(budget.get()));
        assert_eq!(first.bundle, second.bundle);
        assert!(
            first.bundle.items.iter().any(|item| item
                .location
                .uri
                .to_ascii_lowercase()
                .contains("session")),
            "mandatory session evidence must fit the baseline budget"
        );
        assert!(first.bundle.items.iter().all(|item| {
            item.sensitivity != Sensitivity::Redacted || !item.content.contains("canary-secret")
        }));
        assert!(!first.repository_trace.cancelled);
    }

    #[test]
    fn query_terms_are_normalized_deduplicated_and_stable() {
        assert_eq!(
            query_terms("Session, AUTH session x"),
            vec!["auth", "session"]
        );
    }

    fn daemon_config(path: &Path) -> DaemonConfig {
        DaemonConfig {
            runtime_directory: path.to_owned(),
            max_frame_bytes: 64 * 1024,
            max_concurrency: 2,
            graceful_shutdown_timeout: Duration::from_secs(2),
            max_log_bytes: 1024,
        }
    }

    #[test]
    fn capability_registry_uses_explicit_fallback_health() {
        let mut registry = CapabilityRegistry::default();
        registry.register("context", "graphify", false, "native-repo", false);
        registry.register("memory", "sqlite", true, "sqlite", true);
        assert_eq!(registry.overall_health(), HealthState::Degraded);
        assert_eq!(registry.list()[0].provider, "native-repo");

        registry.register("required", "missing", false, "none", true);
        assert_eq!(registry.overall_health(), HealthState::Failed);
    }

    #[test]
    fn local_daemon_is_authenticated_single_instance_and_graceful() {
        let directory = tempfile::tempdir().expect("runtime directory");
        let mut registry = CapabilityRegistry::default();
        registry.register("context", "native-repo", true, "native-repo", true);
        let handler = Arc::new(BuiltInHandler::new(registry));
        let daemon = DaemonHandle::start(&daemon_config(directory.path()), handler.clone())
            .expect("start daemon");

        assert_eq!(
            daemon.call("echo", json!({"value": 42})).expect("echo"),
            json!({"value": 42})
        );
        assert!(matches!(
            DaemonHandle::start(&daemon_config(directory.path()), handler),
            Err(DaemonError::AlreadyRunning)
        ));

        let mut unauthorized = daemon.discovery().clone();
        unauthorized.auth_token = "b".repeat(64);
        assert!(matches!(
            daemon_call(&unauthorized, 64 * 1024, "health", Value::Null),
            Err(DaemonError::Remote(-32001, _))
        ));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let socket_mode = fs::metadata(&daemon.discovery.endpoint)
                .expect("socket metadata")
                .permissions()
                .mode()
                & 0o777;
            let state_mode = fs::metadata(directory.path().join("daemon.json"))
                .expect("state metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(socket_mode, 0o600);
            assert_eq!(state_mode, 0o600);
        }

        daemon.shutdown().expect("graceful shutdown");
        assert!(!directory.path().join("daemon.json").exists());
    }

    #[test]
    fn daemon_bounds_concurrent_requests() {
        use std::sync::atomic::AtomicUsize;

        #[derive(Debug)]
        struct TrackingHandler {
            active: AtomicUsize,
            maximum_seen: AtomicUsize,
        }
        impl RpcHandler for TrackingHandler {
            fn handle(
                &self,
                _request: &RpcRequest,
                _cancelled: &AtomicBool,
            ) -> Result<Value, ServiceError> {
                let active = self.active.fetch_add(1, Ordering::AcqRel) + 1;
                self.maximum_seen.fetch_max(active, Ordering::AcqRel);
                thread::sleep(Duration::from_millis(25));
                self.active.fetch_sub(1, Ordering::AcqRel);
                Ok(json!({"ok": true}))
            }
        }

        let directory = tempfile::tempdir().expect("runtime directory");
        let handler = Arc::new(TrackingHandler {
            active: AtomicUsize::new(0),
            maximum_seen: AtomicUsize::new(0),
        });
        let daemon = DaemonHandle::start(&daemon_config(directory.path()), handler.clone())
            .expect("start daemon");
        let discovery = daemon.discovery().clone();
        let clients = (0..8)
            .map(|_| {
                let discovery = discovery.clone();
                thread::spawn(move || daemon_call(&discovery, 64 * 1024, "work", Value::Null))
            })
            .collect::<Vec<_>>();
        for client in clients {
            assert_eq!(
                client.join().expect("client thread").expect("request"),
                json!({"ok": true})
            );
        }
        assert!(handler.maximum_seen.load(Ordering::Acquire) <= 2);
        daemon.shutdown().expect("graceful shutdown");
    }

    #[test]
    fn each_request_has_an_addressable_cancellation_token() {
        #[derive(Debug)]
        struct CancellableHandler;
        impl RpcHandler for CancellableHandler {
            fn handle(
                &self,
                _request: &RpcRequest,
                cancelled: &AtomicBool,
            ) -> Result<Value, ServiceError> {
                while !cancelled.load(Ordering::Acquire) {
                    thread::sleep(Duration::from_millis(2));
                }
                Ok(json!({"cancelled": true}))
            }
        }
        let graph =
            Arc::new(ServiceGraph::new(Arc::new(CancellableHandler), 1).expect("service graph"));
        let request = RpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: RequestId::new("cancel-target").expect("ID"),
            protocol_version: ProtocolVersion::CURRENT,
            auth_token: "a".repeat(64),
            method: "work".to_owned(),
            params: Value::Null,
            deadline_unix_ms: None,
        };
        let worker_graph = Arc::clone(&graph);
        let worker = thread::spawn(move || worker_graph.dispatch(&request));
        for _ in 0..100 {
            if graph.cancel("cancel-target").expect("cancel lookup") {
                break;
            }
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            worker.join().expect("worker").expect("dispatch"),
            json!({"cancelled": true})
        );
    }

    #[test]
    fn request_deadline_raises_its_cancellation_token() {
        #[derive(Debug)]
        struct DeadlineHandler;
        impl RpcHandler for DeadlineHandler {
            fn handle(
                &self,
                _request: &RpcRequest,
                cancelled: &AtomicBool,
            ) -> Result<Value, ServiceError> {
                while !cancelled.load(Ordering::Acquire) {
                    thread::sleep(Duration::from_millis(1));
                }
                Ok(json!({"deadline": true}))
            }
        }
        let graph = ServiceGraph::new(Arc::new(DeadlineHandler), 1).expect("service graph");
        let request = RpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: RequestId::new("deadline-target").expect("ID"),
            protocol_version: ProtocolVersion::CURRENT,
            auth_token: "a".repeat(64),
            method: "work".to_owned(),
            params: Value::Null,
            deadline_unix_ms: Some(unix_millis().expect("clock") + 20),
        };
        assert_eq!(
            graph.dispatch(&request).expect("deadline dispatch"),
            json!({"deadline": true})
        );
    }

    #[test]
    fn dropping_handle_cleans_state_and_releases_instance_lock() {
        let directory = tempfile::tempdir().expect("runtime directory");
        let config = daemon_config(directory.path());
        {
            let _daemon = DaemonHandle::start(
                &config,
                Arc::new(BuiltInHandler::new(CapabilityRegistry::default())),
            )
            .expect("first daemon");
        }
        assert!(!directory.path().join("daemon.json").exists());
        let daemon = DaemonHandle::start(
            &config,
            Arc::new(BuiltInHandler::new(CapabilityRegistry::default())),
        )
        .expect("restarted daemon");
        daemon.shutdown().expect("shutdown restarted daemon");
    }

    #[test]
    fn rotating_log_never_contains_request_content() {
        let directory = tempfile::tempdir().expect("runtime directory");
        let daemon = DaemonHandle::start(
            &daemon_config(directory.path()),
            Arc::new(BuiltInHandler::new(CapabilityRegistry::default())),
        )
        .expect("start daemon");
        daemon
            .call("echo", json!({"secret":"fixture-canary-source"}))
            .expect("echo");
        daemon.shutdown().expect("shutdown");
        let log = fs::read_to_string(directory.path().join("daemon.log")).expect("daemon log");
        assert!(!log.contains("fixture-canary-source"));
    }
}
