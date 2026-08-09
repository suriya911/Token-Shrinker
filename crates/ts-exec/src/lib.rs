//! Policy-aware process execution and bounded output capture.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fmt, fs,
    io::{self, Read},
    path::PathBuf,
    process::{Child, Command, ExitStatus, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};
use token_shrinker_compress::{RawArtifactHandle, TerminalInput};

/// Why an admitted process stopped.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminationReason {
    /// Process exited without enforcement.
    Completed,
    /// Configured deadline elapsed.
    TimedOut,
    /// Caller raised the cancellation flag.
    Cancelled,
}

/// Bounded first/tail bytes from one process stream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapturedStream {
    /// First retained bytes.
    pub head: Vec<u8>,
    /// Last retained bytes when truncation occurred.
    pub tail: Vec<u8>,
    /// Total bytes read before EOF.
    pub total_bytes: u64,
    /// Whether bytes between head and tail were discarded.
    pub truncated: bool,
}

impl CapturedStream {
    /// Returns retained head and tail with a stable omission marker between them.
    #[must_use]
    pub fn retained_bytes(&self) -> Vec<u8> {
        if !self.truncated {
            return self.head.clone();
        }
        let mut output = self.head.clone();
        output.extend_from_slice(b"\n...[output truncated]...\n");
        output.extend_from_slice(&self.tail);
        output
    }
}

/// One argument-array execution request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionRequest {
    /// Exact executable path; no shell parsing occurs.
    pub program: PathBuf,
    /// Arguments passed as distinct OS strings.
    pub args: Vec<String>,
    /// Requested working directory.
    pub working_directory: PathBuf,
    /// Explicit environment additions admitted by policy.
    pub environment: BTreeMap<String, String>,
    /// Requested deadline, capped by policy.
    pub timeout: Duration,
}

/// Immutable execution admission policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionPolicy {
    allowed_programs: BTreeSet<PathBuf>,
    denied_programs: BTreeSet<PathBuf>,
    allowed_roots: Vec<PathBuf>,
    inherited_environment: BTreeSet<String>,
    denied_environment: BTreeSet<String>,
    max_timeout: Duration,
    max_output_bytes_per_stream: usize,
}

impl ExecutionPolicy {
    /// Constructs a policy after canonicalizing every executable and working root.
    ///
    /// # Errors
    ///
    /// Returns [`ExecutionError`] for nonexistent paths or zero limits.
    pub fn new(
        allowed_programs: impl IntoIterator<Item = PathBuf>,
        denied_programs: impl IntoIterator<Item = PathBuf>,
        allowed_roots: impl IntoIterator<Item = PathBuf>,
        inherited_environment: impl IntoIterator<Item = String>,
        denied_environment: impl IntoIterator<Item = String>,
        max_timeout: Duration,
        max_output_bytes_per_stream: usize,
    ) -> Result<Self, ExecutionError> {
        if max_timeout.is_zero() || max_output_bytes_per_stream == 0 {
            return Err(ExecutionError::ZeroLimit);
        }
        let allowed_programs = canonical_set(allowed_programs, true)?;
        let denied_programs = canonical_set(denied_programs, true)?;
        let allowed_roots = canonical_roots(allowed_roots)?;
        if allowed_programs.is_empty() || allowed_roots.is_empty() {
            return Err(ExecutionError::EmptyPolicy);
        }
        Ok(Self {
            allowed_programs,
            denied_programs,
            allowed_roots,
            inherited_environment: inherited_environment
                .into_iter()
                .map(|key| normalize_environment_key(&key))
                .collect(),
            denied_environment: denied_environment
                .into_iter()
                .map(|key| normalize_environment_key(&key))
                .collect(),
            max_timeout,
            max_output_bytes_per_stream,
        })
    }

    fn admit(&self, request: &ExecutionRequest) -> Result<AdmittedExecution, ExecutionError> {
        let program = fs::canonicalize(&request.program).map_err(ExecutionError::ProgramIo)?;
        if self.denied_programs.contains(&program) || !self.allowed_programs.contains(&program) {
            return Err(ExecutionError::ProgramDenied(program));
        }
        let working_directory = fs::canonicalize(&request.working_directory)
            .map_err(ExecutionError::WorkingDirectoryIo)?;
        if !working_directory.is_dir()
            || !self
                .allowed_roots
                .iter()
                .any(|root| working_directory.starts_with(root))
        {
            return Err(ExecutionError::WorkingDirectoryDenied(working_directory));
        }

        let mut environment = BTreeMap::new();
        for key in &self.inherited_environment {
            if !self.denied_environment.contains(key)
                && let Some(value) = environment_value(key)
            {
                environment.insert(key.clone(), value);
            }
        }
        for (key, value) in &request.environment {
            let key = normalize_environment_key(key);
            if self.denied_environment.contains(&key) || !self.inherited_environment.contains(&key)
            {
                return Err(ExecutionError::EnvironmentDenied(key));
            }
            environment.insert(key, value.clone());
        }

        Ok(AdmittedExecution {
            program,
            args: request.args.clone(),
            working_directory,
            environment,
            timeout: request.timeout.min(self.max_timeout),
            output_cap: self.max_output_bytes_per_stream,
        })
    }
}

#[derive(Debug)]
struct AdmittedExecution {
    program: PathBuf,
    args: Vec<String>,
    working_directory: PathBuf,
    environment: BTreeMap<String, String>,
    timeout: Duration,
    output_cap: usize,
}

/// Completed process evidence, including nonzero exits and enforced termination.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionResult {
    /// Canonical executable followed by exact arguments.
    pub command: Vec<String>,
    /// Platform exit code, when available.
    pub exit_code: Option<i32>,
    /// Reason the process stopped.
    pub termination: TerminationReason,
    /// Monotonic execution duration.
    pub duration: Duration,
    /// Bounded standard output.
    pub stdout: CapturedStream,
    /// Bounded standard error.
    pub stderr: CapturedStream,
}

impl ExecutionResult {
    /// Converts bounded process evidence into the terminal-compressor contract.
    #[must_use]
    pub fn terminal_input(&self, raw_handle: Option<RawArtifactHandle>) -> TerminalInput {
        TerminalInput {
            command: self.command.clone(),
            exit_code: self.exit_code,
            duration: self.duration,
            stdout: String::from_utf8_lossy(&self.stdout.retained_bytes()).into_owned(),
            stderr: String::from_utf8_lossy(&self.stderr.retained_bytes()).into_owned(),
            truncated: self.stdout.truncated || self.stderr.truncated,
            raw_handle,
        }
    }

    /// Returns a bounded raw artifact body with explicit stream boundaries.
    #[must_use]
    pub fn retained_raw_output(&self) -> Vec<u8> {
        let mut output = b"--- stdout ---\n".to_vec();
        output.extend(self.stdout.retained_bytes());
        output.extend_from_slice(b"\n--- stderr ---\n");
        output.extend(self.stderr.retained_bytes());
        output
    }
}

/// Synchronous, policy-enforcing execution engine.
#[derive(Clone, Debug)]
pub struct ExecutionEngine {
    policy: ExecutionPolicy,
}

impl ExecutionEngine {
    /// Creates an engine with an immutable admission policy.
    #[must_use]
    pub const fn new(policy: ExecutionPolicy) -> Self {
        Self { policy }
    }

    /// Admits, spawns, observes, and if necessary terminates one process tree.
    ///
    /// # Errors
    ///
    /// Returns [`ExecutionError`] for policy denial, spawn/wait failure, or capture failure.
    pub fn execute(
        &self,
        request: &ExecutionRequest,
        cancelled: &AtomicBool,
    ) -> Result<ExecutionResult, ExecutionError> {
        let admitted = self.policy.admit(request)?;
        if admitted.timeout.is_zero() {
            return Err(ExecutionError::ZeroLimit);
        }
        let mut command = Command::new(&admitted.program);
        command
            .args(&admitted.args)
            .current_dir(&admitted.working_directory)
            .env_clear()
            .envs(&admitted.environment)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        configure_process_group(&mut command);

        let started = Instant::now();
        let mut child = command.spawn().map_err(ExecutionError::Spawn)?;
        let stdout = child.stdout.take().ok_or(ExecutionError::MissingPipe)?;
        let stderr = child.stderr.take().ok_or(ExecutionError::MissingPipe)?;
        let cap = admitted.output_cap;
        let stdout_reader = thread::spawn(move || read_bounded(stdout, cap));
        let stderr_reader = thread::spawn(move || read_bounded(stderr, cap));

        let (status, termination) =
            wait_for_exit(&mut child, started, admitted.timeout, cancelled)?;
        let stdout = join_capture(stdout_reader)?;
        let stderr = join_capture(stderr_reader)?;
        let mut recorded_command = vec![admitted.program.to_string_lossy().into_owned()];
        recorded_command.extend(admitted.args);
        Ok(ExecutionResult {
            command: recorded_command,
            exit_code: status.code(),
            termination,
            duration: started.elapsed(),
            stdout,
            stderr,
        })
    }
}

fn wait_for_exit(
    child: &mut Child,
    started: Instant,
    timeout: Duration,
    cancelled: &AtomicBool,
) -> Result<(ExitStatus, TerminationReason), ExecutionError> {
    loop {
        if let Some(status) = child.try_wait().map_err(ExecutionError::Wait)? {
            return Ok((status, TerminationReason::Completed));
        }
        let termination = if cancelled.load(Ordering::Acquire) {
            Some(TerminationReason::Cancelled)
        } else if started.elapsed() >= timeout {
            Some(TerminationReason::TimedOut)
        } else {
            None
        };
        if let Some(termination) = termination {
            terminate_process_tree(child);
            let status = child.wait().map_err(ExecutionError::Wait)?;
            return Ok((status, termination));
        }
        thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(unix)]
fn configure_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(windows)]
fn configure_process_group(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    command.creation_flags(CREATE_NEW_PROCESS_GROUP);
}

#[cfg(unix)]
fn terminate_process_tree(child: &mut Child) {
    let _ = Command::new("kill")
        .args(["-KILL", "--", &format!("-{}", child.id())])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = child.kill();
}

#[cfg(windows)]
fn terminate_process_tree(child: &mut Child) {
    let taskkill = Command::new("taskkill")
        .args(["/PID", &child.id().to_string(), "/T", "/F"])
        .output();
    if !matches!(taskkill, Ok(output) if output.status.success()) {
        let _ = child.kill();
    }
}

fn read_bounded(mut reader: impl Read, cap: usize) -> io::Result<CapturedStream> {
    let head_cap = cap.div_ceil(2);
    let tail_cap = cap / 2;
    let mut head = Vec::with_capacity(head_cap);
    let mut tail = VecDeque::with_capacity(tail_cap);
    let mut total_bytes = 0_u64;
    let mut buffer = [0_u8; 8192];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        total_bytes = total_bytes.saturating_add(u64::try_from(read).unwrap_or(u64::MAX));
        for &byte in &buffer[..read] {
            if head.len() < head_cap {
                head.push(byte);
            } else if tail_cap > 0 {
                if tail.len() == tail_cap {
                    tail.pop_front();
                }
                tail.push_back(byte);
            }
        }
    }
    let truncated = total_bytes > u64::try_from(cap).unwrap_or(u64::MAX);
    if !truncated {
        head.extend(tail);
        tail = VecDeque::new();
    }
    Ok(CapturedStream {
        head,
        tail: tail.into(),
        total_bytes,
        truncated,
    })
}

fn join_capture(
    reader: thread::JoinHandle<io::Result<CapturedStream>>,
) -> Result<CapturedStream, ExecutionError> {
    reader
        .join()
        .map_err(|_| ExecutionError::CaptureThread)?
        .map_err(ExecutionError::Capture)
}

fn canonical_set(
    paths: impl IntoIterator<Item = PathBuf>,
    require_file: bool,
) -> Result<BTreeSet<PathBuf>, ExecutionError> {
    paths
        .into_iter()
        .map(|path| {
            let canonical = fs::canonicalize(path).map_err(ExecutionError::ProgramIo)?;
            if require_file && !canonical.is_file() {
                return Err(ExecutionError::ProgramDenied(canonical));
            }
            Ok(canonical)
        })
        .collect()
}

fn canonical_roots(
    paths: impl IntoIterator<Item = PathBuf>,
) -> Result<Vec<PathBuf>, ExecutionError> {
    paths
        .into_iter()
        .map(|path| {
            let canonical = fs::canonicalize(path).map_err(ExecutionError::WorkingDirectoryIo)?;
            if !canonical.is_dir() {
                return Err(ExecutionError::WorkingDirectoryDenied(canonical));
            }
            Ok(canonical)
        })
        .collect()
}

#[cfg(windows)]
fn normalize_environment_key(key: &str) -> String {
    key.to_ascii_uppercase()
}

#[cfg(not(windows))]
fn normalize_environment_key(key: &str) -> String {
    key.to_owned()
}

#[cfg(windows)]
fn environment_value(key: &str) -> Option<String> {
    std::env::vars()
        .find_map(|(candidate, value)| candidate.eq_ignore_ascii_case(key).then_some(value))
}

#[cfg(not(windows))]
fn environment_value(key: &str) -> Option<String> {
    std::env::var(key).ok()
}

/// Execution admission or lifecycle failure.
#[derive(Debug)]
pub enum ExecutionError {
    /// A configured limit was zero.
    ZeroLimit,
    /// No executable or working root was configured.
    EmptyPolicy,
    /// Executable could not be canonicalized.
    ProgramIo(io::Error),
    /// Working directory could not be canonicalized.
    WorkingDirectoryIo(io::Error),
    /// Executable was absent from the allowlist or explicitly denied.
    ProgramDenied(PathBuf),
    /// Working directory escaped every allowed root.
    WorkingDirectoryDenied(PathBuf),
    /// Environment key was not allowlisted or was explicitly denied.
    EnvironmentDenied(String),
    /// Process could not be spawned.
    Spawn(io::Error),
    /// Child pipe was unexpectedly unavailable.
    MissingPipe,
    /// Process status could not be observed.
    Wait(io::Error),
    /// Output reader failed.
    Capture(io::Error),
    /// Output reader panicked.
    CaptureThread,
}

impl fmt::Display for ExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroLimit => formatter.write_str("execution limits must be positive"),
            Self::EmptyPolicy => {
                formatter.write_str("execution policy requires programs and roots")
            }
            Self::ProgramIo(error) => write!(formatter, "cannot resolve executable: {error}"),
            Self::WorkingDirectoryIo(error) => {
                write!(formatter, "cannot resolve working directory: {error}")
            }
            Self::ProgramDenied(path) => {
                write!(formatter, "executable is denied: {}", path.display())
            }
            Self::WorkingDirectoryDenied(path) => {
                write!(formatter, "working directory is denied: {}", path.display())
            }
            Self::EnvironmentDenied(key) => write!(formatter, "environment key is denied: {key}"),
            Self::Spawn(error) => write!(formatter, "process spawn failed: {error}"),
            Self::MissingPipe => formatter.write_str("process output pipe is unavailable"),
            Self::Wait(error) => write!(formatter, "process wait failed: {error}"),
            Self::Capture(error) => write!(formatter, "process output capture failed: {error}"),
            Self::CaptureThread => formatter.write_str("process output reader panicked"),
        }
    }
}

impl std::error::Error for ExecutionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ProgramIo(error)
            | Self::WorkingDirectoryIo(error)
            | Self::Spawn(error)
            | Self::Wait(error)
            | Self::Capture(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn policy(root: &Path, executable: &Path, cap: usize) -> ExecutionPolicy {
        ExecutionPolicy::new(
            [executable.to_owned()],
            [],
            [root.to_owned()],
            ["TS_EXEC_HELPER".to_owned(), "TS_EXEC_MARKER".to_owned()],
            ["TOKEN".to_owned()],
            Duration::from_secs(5),
            cap,
        )
        .expect("test execution policy")
    }

    fn request(root: &Path, helper: &str) -> ExecutionRequest {
        ExecutionRequest {
            program: std::env::current_exe().expect("test executable"),
            args: vec![
                "--exact".to_owned(),
                "tests::subprocess_helper".to_owned(),
                "--nocapture".to_owned(),
            ],
            working_directory: root.to_owned(),
            environment: BTreeMap::from([("TS_EXEC_HELPER".to_owned(), helper.to_owned())]),
            timeout: Duration::from_secs(2),
        }
    }

    #[test]
    fn subprocess_helper() {
        match std::env::var("TS_EXEC_HELPER").as_deref() {
            Ok("nonzero") => panic!("preserved helper failure"),
            Ok("large") => {
                println!("HEAD{}TAIL", "x".repeat(4_096));
                eprintln!("error evidence");
            }
            Ok("wait") => thread::sleep(Duration::from_secs(30)),
            Ok("tree") => {
                let marker = std::env::var("TS_EXEC_MARKER").expect("tree marker path");
                let mut grandchild =
                    Command::new(std::env::current_exe().expect("test executable"))
                        .args(["--exact", "tests::subprocess_helper", "--nocapture"])
                        .env("TS_EXEC_HELPER", "grandchild")
                        .env("TS_EXEC_MARKER", marker)
                        .spawn()
                        .expect("spawn grandchild");
                thread::sleep(Duration::from_secs(30));
                let _ = grandchild.wait();
            }
            Ok("grandchild") => {
                let marker = std::env::var("TS_EXEC_MARKER").expect("grandchild marker path");
                fs::write(format!("{marker}.ready"), "ready")
                    .expect("write grandchild ready marker");
                // Leave enough time for Windows taskkill startup under a loaded CI host.
                thread::sleep(Duration::from_secs(2));
                fs::write(marker, "survived").expect("write survival marker");
            }
            _ => {}
        }
    }

    #[test]
    fn blocked_command_never_spawns() {
        let directory = tempfile::tempdir().expect("temporary working root");
        let executable = std::env::current_exe().expect("test executable");
        let denied = ExecutionPolicy::new(
            [executable.clone()],
            [executable],
            [directory.path().to_owned()],
            ["TS_EXEC_HELPER".to_owned()],
            [],
            Duration::from_secs(1),
            100,
        )
        .expect("denying policy");
        let result = ExecutionEngine::new(denied)
            .execute(&request(directory.path(), "wait"), &AtomicBool::new(false));
        assert!(matches!(result, Err(ExecutionError::ProgramDenied(_))));
    }

    #[test]
    fn nonzero_exit_and_error_evidence_are_preserved() {
        let directory = tempfile::tempdir().expect("temporary working root");
        let executable = std::env::current_exe().expect("test executable");
        let engine = ExecutionEngine::new(policy(directory.path(), &executable, 4_096));

        let result = engine
            .execute(
                &request(directory.path(), "nonzero"),
                &AtomicBool::new(false),
            )
            .expect("execute failing helper");

        assert_eq!(result.termination, TerminationReason::Completed);
        assert_ne!(result.exit_code, Some(0));
        let evidence = [
            result.stdout.retained_bytes(),
            result.stderr.retained_bytes(),
        ]
        .concat();
        assert!(String::from_utf8_lossy(&evidence).contains("preserved helper failure"));
    }

    #[test]
    fn output_cap_preserves_head_tail_and_stderr() {
        let directory = tempfile::tempdir().expect("temporary working root");
        let executable = std::env::current_exe().expect("test executable");
        let engine = ExecutionEngine::new(policy(directory.path(), &executable, 128));

        let result = engine
            .execute(&request(directory.path(), "large"), &AtomicBool::new(false))
            .expect("execute noisy helper");

        assert!(result.stdout.truncated);
        assert!(
            String::from_utf8_lossy(&result.stderr.retained_bytes()).contains("error evidence")
        );

        let bounded = read_bounded(std::io::Cursor::new(b"HEADxxxxxxxxTAIL"), 8)
            .expect("bounded in-memory capture");
        let retained = bounded.retained_bytes();
        let retained = String::from_utf8_lossy(&retained);
        assert!(bounded.truncated);
        assert!(retained.starts_with("HEAD"));
        assert!(retained.ends_with("TAIL"));
    }

    #[test]
    fn cancellation_terminates_entire_process_tree() {
        use std::sync::Arc;

        let directory = tempfile::tempdir().expect("temporary working root");
        let executable = std::env::current_exe().expect("test executable");
        let engine = ExecutionEngine::new(policy(directory.path(), &executable, 128));
        let marker = directory.path().join("grandchild-survived.txt");
        let ready = directory.path().join("grandchild-survived.txt.ready");
        let mut request = request(directory.path(), "tree");
        request.environment.insert(
            "TS_EXEC_MARKER".to_owned(),
            marker.to_string_lossy().into_owned(),
        );
        let cancelled = Arc::new(AtomicBool::new(false));
        let trigger = Arc::clone(&cancelled);
        let cancellation_thread = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            while !ready.exists() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(5));
            }
            assert!(ready.exists(), "grandchild never reached ready state");
            trigger.store(true, Ordering::Release);
        });

        let result = engine
            .execute(&request, &cancelled)
            .expect("execute cancelled helper");
        cancellation_thread.join().expect("cancellation thread");

        assert_eq!(result.termination, TerminationReason::Cancelled);
        assert!(result.duration < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(2_200));
        assert!(
            !marker.exists(),
            "grandchild survived process-tree cancellation"
        );
    }

    #[test]
    fn timeout_and_environment_denial_are_explicit() {
        let directory = tempfile::tempdir().expect("temporary working root");
        let executable = std::env::current_exe().expect("test executable");
        let engine = ExecutionEngine::new(policy(directory.path(), &executable, 128));
        let mut timed = request(directory.path(), "wait");
        timed.timeout = Duration::from_millis(30);
        let result = engine
            .execute(&timed, &AtomicBool::new(false))
            .expect("timed execution");
        assert_eq!(result.termination, TerminationReason::TimedOut);

        let mut denied = request(directory.path(), "wait");
        denied
            .environment
            .insert("TOKEN".to_owned(), "secret".to_owned());
        assert!(
            matches!(engine.execute(&denied, &AtomicBool::new(false)), Err(ExecutionError::EnvironmentDenied(key)) if key == "TOKEN")
        );
    }
}
