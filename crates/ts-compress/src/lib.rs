//! Built-in deterministic context and terminal compressors plus raw artifact retention.

use semver::VersionReq;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fmt, fs, io,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use token_shrinker_context::{ConservativeEstimator, ContextCandidate, SourceId, TokenCounter};
use token_shrinker_provider::{
    CircuitBreaker, ManagedProcess, McpSession, ProviderError, ProviderLimits, ProviderOutcome,
    ProviderSpec, resolve_with_fallback,
};
use token_shrinker_types::TokenBudget;

/// Headroom versions covered by adapter tests.
pub const HEADROOM_VERSION_REQUIREMENT: &str = ">=0.22.0, <1.0.0";
/// RTK versions covered by adapter tests.
pub const RTK_VERSION_REQUIREMENT: &str = ">=0.45.0, <1.0.0";

/// Validated result from an optional compressor.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCompression {
    /// Provider identifier for response attribution.
    pub provider: String,
    /// Compressed text.
    pub compressed: String,
    /// Provider-reported original token count when available.
    pub original_tokens: Option<u64>,
    /// Provider-reported compressed token count when available.
    pub compressed_tokens: Option<u64>,
}

/// Optional Headroom MCP compressor. Content crosses the configured subprocess boundary.
#[derive(Clone, Debug)]
pub struct HeadroomProvider {
    spec: ProviderSpec,
    version_process: ManagedProcess,
    circuit: CircuitBreaker,
}

impl HeadroomProvider {
    /// Configures `headroom mcp serve` or a contract-compatible executable.
    #[must_use]
    pub fn new(command: PathBuf, base_args: Vec<String>, required: bool) -> Self {
        let limits = ProviderLimits::default();
        let version_spec = ProviderSpec {
            id: "headroom".to_owned(),
            command: command.clone(),
            base_args: Vec::new(),
            environment: BTreeMap::new(),
            version_requirement: VersionReq::parse(HEADROOM_VERSION_REQUIREMENT)
                .unwrap_or(VersionReq::STAR),
            required,
            limits,
        };
        let spec = ProviderSpec {
            id: "headroom".to_owned(),
            command,
            base_args,
            environment: BTreeMap::new(),
            version_requirement: VersionReq::parse(HEADROOM_VERSION_REQUIREMENT)
                .unwrap_or(VersionReq::STAR),
            required,
            limits,
        };
        Self {
            spec,
            version_process: ManagedProcess::new(version_spec),
            circuit: CircuitBreaker::new(limits.failure_threshold, limits.cooldown),
        }
    }

    /// Probes MCP initialization, version, and the compression capability.
    ///
    /// # Errors
    ///
    /// Returns a normalized provider, protocol, or compatibility error.
    pub fn probe(&self) -> Result<(), ProviderError> {
        self.version_process.probe(&["--version".to_owned()])?;
        McpSession::connect_capability(&self.spec, "headroom_compress").map(drop)
    }

    /// Compresses content and validates Headroom's JSON result schema.
    ///
    /// # Errors
    ///
    /// Returns a normalized provider, deadline, or schema error.
    pub fn compress(&self, content: &str) -> Result<ProviderCompression, ProviderError> {
        self.circuit.permit()?;
        let result = self.compress_inner(content);
        if result.is_ok() {
            self.circuit.success();
        } else {
            self.circuit.failure();
        }
        result
    }

    /// Compresses with Headroom or returns the built-in extractive result for this request.
    ///
    /// # Errors
    ///
    /// Returns the classified Headroom failure when the provider is required.
    pub fn compress_or_fallback(
        &self,
        content: &str,
        fallback: impl FnOnce() -> ProviderCompression,
    ) -> Result<ProviderOutcome<ProviderCompression>, ProviderError> {
        resolve_with_fallback(
            &self.spec,
            self.compress(content),
            "builtin-extractive",
            fallback,
        )
    }

    fn compress_inner(&self, content: &str) -> Result<ProviderCompression, ProviderError> {
        let mut session = McpSession::connect_capability(&self.spec, "headroom_compress")?;
        let result = session.call_tool(
            "headroom_compress",
            &serde_json::json!({"content": content}),
            self.spec.limits.operation_timeout,
        )?;
        let text = result
            .get("content")
            .and_then(serde_json::Value::as_array)
            .and_then(|items| items.first())
            .and_then(|item| item.get("text"))
            .and_then(serde_json::Value::as_str)
            .ok_or(ProviderError::Malformed)?;
        let value: serde_json::Value =
            serde_json::from_str(text).map_err(|_| ProviderError::Malformed)?;
        let compressed = value
            .get("compressed")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or(ProviderError::Malformed)?
            .to_owned();
        Ok(ProviderCompression {
            provider: "headroom".to_owned(),
            compressed,
            original_tokens: value
                .get("original_tokens")
                .and_then(serde_json::Value::as_u64),
            compressed_tokens: value
                .get("compressed_tokens")
                .and_then(serde_json::Value::as_u64),
        })
    }
}

/// Optional RTK terminal-output compressor. Raw command output crosses the subprocess boundary.
#[derive(Clone, Debug)]
pub struct RtkProvider {
    process: ManagedProcess,
}

impl RtkProvider {
    /// Creates an adapter for the RTK executable.
    #[must_use]
    pub fn new(command: PathBuf, required: bool) -> Self {
        Self::from_spec(ProviderSpec {
            id: "rtk".to_owned(),
            command,
            base_args: Vec::new(),
            environment: BTreeMap::new(),
            version_requirement: VersionReq::parse(RTK_VERSION_REQUIREMENT)
                .unwrap_or(VersionReq::STAR),
            required,
            limits: ProviderLimits::default(),
        })
    }

    /// Creates an adapter from an advanced provider specification.
    #[must_use]
    pub fn from_spec(spec: ProviderSpec) -> Self {
        Self {
            process: ManagedProcess::new(spec),
        }
    }

    /// Verifies executable availability and supported version.
    ///
    /// # Errors
    ///
    /// Returns a normalized provider probe or compatibility error.
    pub fn probe(&self) -> Result<String, ProviderError> {
        self.process
            .probe(&["--version".to_owned()])
            .map(|version| version.to_string())
    }

    /// Applies RTK's log filter to already-captured terminal output.
    ///
    /// # Errors
    ///
    /// Returns a normalized provider, deadline, size, or UTF-8 schema error.
    pub fn compress_terminal(&self, content: &str) -> Result<ProviderCompression, ProviderError> {
        let output = self
            .process
            .invoke(&["log".to_owned()], content.as_bytes())?;
        let compressed = String::from_utf8(output.stdout).map_err(|_| ProviderError::Malformed)?;
        if compressed.trim().is_empty() && !content.trim().is_empty() {
            return Err(ProviderError::Malformed);
        }
        Ok(ProviderCompression {
            provider: "rtk".to_owned(),
            compressed,
            original_tokens: None,
            compressed_tokens: None,
        })
    }

    /// Compresses with RTK or returns the built-in terminal result for this request.
    ///
    /// # Errors
    ///
    /// Returns the classified RTK failure when the provider is required.
    pub fn compress_terminal_or_fallback(
        &self,
        content: &str,
        fallback: impl FnOnce() -> ProviderCompression,
    ) -> Result<ProviderOutcome<ProviderCompression>, ProviderError> {
        resolve_with_fallback(
            self.process.spec(),
            self.compress_terminal(content),
            "builtin-terminal",
            fallback,
        )
    }
}

/// One retained extractive passage with exact source lines.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtractedPassage {
    /// Opaque parent source handle.
    pub source_id: SourceId,
    /// One-based inclusive first line.
    pub start_line: u32,
    /// One-based inclusive final line.
    pub end_line: u32,
    /// Verbatim selected lines.
    pub content: String,
}

/// One omitted source range available for follow-up retrieval.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OmittedRange {
    /// One-based inclusive first line.
    pub start_line: u32,
    /// One-based inclusive final line.
    pub end_line: u32,
}

/// Deterministic extractive compression result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompressedContext {
    /// Selected verbatim passages in source order.
    pub passages: Vec<ExtractedPassage>,
    /// Unselected ranges in source order.
    pub omitted_ranges: Vec<OmittedRange>,
    /// Estimated tokens retained under the supplied budget.
    pub used_tokens: u64,
}

/// Selects matched lines, headings, diagnostics, and definition signatures with bounded context.
#[must_use]
pub fn compress_context(
    source: &ContextCandidate,
    query_terms: &[String],
    surrounding_lines: usize,
    budget: TokenBudget,
) -> CompressedContext {
    let lines = source.content.lines().collect::<Vec<_>>();
    let terms = query_terms
        .iter()
        .map(|term| term.to_lowercase())
        .collect::<Vec<_>>();
    let mut wanted = BTreeMap::<usize, u8>::new();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        let lower = trimmed.to_lowercase();
        let matched = terms
            .iter()
            .any(|term| !term.is_empty() && lower.contains(term));
        let diagnostic = ["error", "warning", "panicked", "assertion failed"]
            .iter()
            .any(|marker| lower.contains(marker));
        let definition = [
            "class ",
            "def ",
            "fn ",
            "function ",
            "interface ",
            "pub fn ",
            "struct ",
        ]
        .iter()
        .any(|prefix| trimmed.starts_with(prefix));
        let important = matched || diagnostic || definition || trimmed.starts_with('#');
        if important {
            let priority = if matched || diagnostic {
                0
            } else if definition {
                1
            } else {
                2
            };
            wanted
                .entry(index)
                .and_modify(|current| *current = (*current).min(priority))
                .or_insert(priority);
            let start = index.saturating_sub(surrounding_lines);
            let end = index
                .saturating_add(surrounding_lines)
                .min(lines.len().saturating_sub(1));
            for context_index in start..=end {
                wanted.entry(context_index).or_insert(3);
            }
        }
    }

    let counter = ConservativeEstimator;
    let limit = u64::from(budget.get());
    let mut selected = BTreeSet::new();
    let mut used_tokens = 0_u64;
    let mut wanted = wanted.into_iter().collect::<Vec<_>>();
    wanted.sort_by_key(|(index, priority)| (*priority, *index));
    for (index, _) in wanted {
        // Include a conservative separator so joined passages remain under budget.
        let cost = counter.count(lines[index]).tokens().saturating_add(4);
        if used_tokens.saturating_add(cost) <= limit {
            selected.insert(index);
            used_tokens += cost;
        }
    }
    CompressedContext {
        passages: ranges(&selected)
            .into_iter()
            .map(|(start, end)| ExtractedPassage {
                source_id: source.source_id.clone(),
                start_line: u32::try_from(start + 1).unwrap_or(u32::MAX),
                end_line: u32::try_from(end + 1).unwrap_or(u32::MAX),
                content: lines[start..=end].join("\n"),
            })
            .collect(),
        omitted_ranges: omitted_ranges(lines.len(), &selected),
        used_tokens,
    }
}

fn ranges(indices: &BTreeSet<usize>) -> Vec<(usize, usize)> {
    let mut output: Vec<(usize, usize)> = Vec::new();
    for &index in indices {
        if let Some((_, end)) = output.last_mut()
            && index == end.saturating_add(1)
        {
            *end = index;
        } else {
            output.push((index, index));
        }
    }
    output
}

fn omitted_ranges(line_count: usize, selected: &BTreeSet<usize>) -> Vec<OmittedRange> {
    let omitted = (0..line_count)
        .filter(|index| !selected.contains(index))
        .collect::<BTreeSet<_>>();
    ranges(&omitted)
        .into_iter()
        .map(|(start, end)| OmittedRange {
            start_line: u32::try_from(start + 1).unwrap_or(u32::MAX),
            end_line: u32::try_from(end + 1).unwrap_or(u32::MAX),
        })
        .collect()
}

/// Normalized process output supplied to the terminal compressor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalInput {
    /// Exact executable and arguments.
    pub command: Vec<String>,
    /// Platform exit code when available.
    pub exit_code: Option<i32>,
    /// Monotonic duration.
    pub duration: Duration,
    /// Retained standard output.
    pub stdout: String,
    /// Retained standard error.
    pub stderr: String,
    /// Whether upstream capture discarded bytes.
    pub truncated: bool,
    /// Optional raw artifact handle.
    pub raw_handle: Option<RawArtifactHandle>,
}

/// Unique actionable terminal line and repetition count.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalEvidence {
    /// First exact occurrence.
    pub line: String,
    /// Number of exact occurrences across both streams.
    pub count: u64,
}

/// Content-preserving bounded terminal summary.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalSummary {
    /// Exact command array.
    pub command: Vec<String>,
    /// Original exit code.
    pub exit_code: Option<i32>,
    /// Original duration.
    pub duration: Duration,
    /// Upstream truncation state.
    pub truncated: bool,
    /// First output lines.
    pub first_lines: Vec<String>,
    /// Last output lines not already retained first.
    pub tail_lines: Vec<String>,
    /// Errors, warnings, failures, assertions, and file/line evidence.
    pub evidence: Vec<TerminalEvidence>,
    /// Opaque raw artifact handle.
    pub raw_handle: Option<RawArtifactHandle>,
}

/// Incremental terminal parser retaining bounded display lines and unique evidence.
pub struct TerminalCompressor {
    input: TerminalInput,
    first_limit: usize,
    tail_limit: usize,
    first_lines: Vec<String>,
    tail_lines: VecDeque<String>,
    evidence: BTreeMap<String, u64>,
    stdout_partial: Vec<u8>,
    stderr_partial: Vec<u8>,
}

impl TerminalCompressor {
    /// Creates an incremental parser for one completed command.
    #[must_use]
    pub fn new(input: &TerminalInput, first_limit: usize, tail_limit: usize) -> Self {
        Self {
            input: input.clone(),
            first_limit,
            tail_limit,
            first_lines: Vec::new(),
            tail_lines: VecDeque::with_capacity(tail_limit),
            evidence: BTreeMap::new(),
            stdout_partial: Vec::new(),
            stderr_partial: Vec::new(),
        }
    }

    /// Pushes one standard-output chunk; lines may cross chunk boundaries.
    pub fn push_stdout(&mut self, chunk: &[u8]) {
        let mut partial = std::mem::take(&mut self.stdout_partial);
        self.push_chunk(&mut partial, chunk);
        self.stdout_partial = partial;
    }

    /// Pushes one standard-error chunk; lines may cross chunk boundaries.
    pub fn push_stderr(&mut self, chunk: &[u8]) {
        let mut partial = std::mem::take(&mut self.stderr_partial);
        self.push_chunk(&mut partial, chunk);
        self.stderr_partial = partial;
    }

    /// Flushes partial lines and returns the stable summary.
    #[must_use]
    pub fn finish(mut self) -> TerminalSummary {
        if !self.stdout_partial.is_empty() {
            self.record_line(String::from_utf8_lossy(&self.stdout_partial).into_owned());
        }
        if !self.stderr_partial.is_empty() {
            self.record_line(String::from_utf8_lossy(&self.stderr_partial).into_owned());
        }
        TerminalSummary {
            command: self.input.command,
            exit_code: self.input.exit_code,
            duration: self.input.duration,
            truncated: self.input.truncated,
            first_lines: self.first_lines,
            tail_lines: self.tail_lines.into(),
            evidence: self
                .evidence
                .into_iter()
                .map(|(line, count)| TerminalEvidence { line, count })
                .collect(),
            raw_handle: self.input.raw_handle,
        }
    }

    fn push_chunk(&mut self, partial: &mut Vec<u8>, chunk: &[u8]) {
        partial.extend_from_slice(chunk);
        while let Some(newline) = partial.iter().position(|byte| *byte == b'\n') {
            let mut line = partial.drain(..=newline).collect::<Vec<_>>();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            self.record_line(String::from_utf8_lossy(&line).into_owned());
        }
    }

    fn record_line(&mut self, line: String) {
        if self.first_lines.len() < self.first_limit {
            self.first_lines.push(line.clone());
        } else if self.tail_limit > 0 {
            if self.tail_lines.len() == self.tail_limit {
                self.tail_lines.pop_front();
            }
            self.tail_lines.push_back(line.clone());
        }
        let lower = line.to_lowercase();
        if [
            "error", "warning", "failed", "failure", "panicked", "assert",
        ]
        .iter()
        .any(|marker| lower.contains(marker))
            || has_file_line_reference(&line)
        {
            *self.evidence.entry(line).or_default() += 1;
        }
    }
}

/// Compresses normalized terminal output without rewriting evidence lines.
#[must_use]
pub fn compress_terminal(
    input: &TerminalInput,
    first_limit: usize,
    tail_limit: usize,
) -> TerminalSummary {
    let mut compressor = TerminalCompressor::new(input, first_limit, tail_limit);
    compressor.push_stdout(input.stdout.as_bytes());
    compressor.push_stderr(input.stderr.as_bytes());
    compressor.finish()
}

fn has_file_line_reference(line: &str) -> bool {
    line.rsplit_once(':').is_some_and(|(prefix, suffix)| {
        suffix.trim().parse::<u32>().is_ok() && prefix.contains(['/', '\\'])
    })
}

/// Opaque identifier for one retained raw artifact.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct RawArtifactHandle(String);

impl RawArtifactHandle {
    /// Returns the opaque printable identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// File-backed, bounded raw-output retention separate from telemetry.
#[derive(Debug)]
pub struct RawArtifactStore {
    root: PathBuf,
    max_artifacts: usize,
    ttl: Duration,
}

impl RawArtifactStore {
    /// Opens a dedicated retention directory.
    ///
    /// # Errors
    ///
    /// Returns [`ArtifactError`] when the directory cannot be created or limits are zero.
    pub fn open(
        root: impl AsRef<Path>,
        max_artifacts: usize,
        ttl: Duration,
    ) -> Result<Self, ArtifactError> {
        if max_artifacts == 0 || ttl.is_zero() {
            return Err(ArtifactError::ZeroLimit);
        }
        fs::create_dir_all(root.as_ref()).map_err(ArtifactError::Io)?;
        let root = fs::canonicalize(root.as_ref()).map_err(ArtifactError::Io)?;
        Ok(Self {
            root,
            max_artifacts,
            ttl,
        })
    }

    /// Retains bytes and returns a content-addressed opaque handle.
    ///
    /// # Errors
    ///
    /// Returns [`ArtifactError`] when pruning or persistence fails.
    pub fn retain(&self, content: &[u8]) -> Result<RawArtifactHandle, ArtifactError> {
        self.prune()?;
        self.make_room()?;
        let now = unix_millis()?;
        let mut hasher = Sha256::new();
        hasher.update(now.to_be_bytes());
        hasher.update(content);
        let handle = RawArtifactHandle(hex_digest(hasher.finalize()));
        let path = self.root.join(format!("{now}-{}.raw", handle.as_str()));
        fs::write(path, content).map_err(ArtifactError::Io)?;
        self.enforce_count()?;
        Ok(handle)
    }

    /// Retrieves retained bytes by opaque handle.
    ///
    /// # Errors
    ///
    /// Returns [`ArtifactError`] for malformed, expired, missing, or unreadable artifacts.
    pub fn fetch(&self, handle: &RawArtifactHandle) -> Result<Vec<u8>, ArtifactError> {
        validate_handle(handle)?;
        let entry = self.find(handle)?.ok_or(ArtifactError::NotFound)?;
        if is_expired(&entry, self.ttl)? {
            return Err(ArtifactError::Expired);
        }
        fs::read(entry.path()).map_err(ArtifactError::Io)
    }

    /// Deletes expired artifacts and returns the number removed.
    ///
    /// # Errors
    ///
    /// Returns [`ArtifactError`] when enumeration or deletion fails.
    pub fn prune(&self) -> Result<usize, ArtifactError> {
        let mut removed = 0;
        for entry in self.entries()? {
            if is_expired(&entry, self.ttl)? {
                fs::remove_file(entry.path()).map_err(ArtifactError::Io)?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    fn enforce_count(&self) -> Result<(), ArtifactError> {
        let mut entries = self.entries()?;
        entries.sort_by_key(fs::DirEntry::file_name);
        let excess = entries.len().saturating_sub(self.max_artifacts);
        for entry in entries.into_iter().take(excess) {
            fs::remove_file(entry.path()).map_err(ArtifactError::Io)?;
        }
        Ok(())
    }

    fn make_room(&self) -> Result<(), ArtifactError> {
        let mut entries = self.entries()?;
        entries.sort_by_key(fs::DirEntry::file_name);
        let keep_before_insert = self.max_artifacts.saturating_sub(1);
        let excess = entries.len().saturating_sub(keep_before_insert);
        for entry in entries.into_iter().take(excess) {
            fs::remove_file(entry.path()).map_err(ArtifactError::Io)?;
        }
        Ok(())
    }

    fn entries(&self) -> Result<Vec<fs::DirEntry>, ArtifactError> {
        fs::read_dir(&self.root)
            .map_err(ArtifactError::Io)?
            .filter_map(|entry| match entry {
                Ok(entry) if entry.path().extension().is_some_and(|ext| ext == "raw") => {
                    Some(Ok(entry))
                }
                Ok(_) => None,
                Err(error) => Some(Err(ArtifactError::Io(error))),
            })
            .collect()
    }

    fn find(&self, handle: &RawArtifactHandle) -> Result<Option<fs::DirEntry>, ArtifactError> {
        let suffix = format!("-{}.raw", handle.as_str());
        Ok(self
            .entries()?
            .into_iter()
            .find(|entry| entry.file_name().to_string_lossy().ends_with(&suffix)))
    }
}

fn validate_handle(handle: &RawArtifactHandle) -> Result<(), ArtifactError> {
    if handle.0.len() == 64
        && handle
            .0
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        Ok(())
    } else {
        Err(ArtifactError::InvalidHandle)
    }
}

fn is_expired(entry: &fs::DirEntry, ttl: Duration) -> Result<bool, ArtifactError> {
    let modified = entry
        .metadata()
        .map_err(ArtifactError::Io)?
        .modified()
        .map_err(ArtifactError::Io)?;
    Ok(SystemTime::now()
        .duration_since(modified)
        .unwrap_or_default()
        >= ttl)
}

fn unix_millis() -> Result<u128, ArtifactError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .map_err(|_| ArtifactError::Clock)
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

/// Raw artifact retention failure.
#[derive(Debug)]
pub enum ArtifactError {
    ZeroLimit,
    InvalidHandle,
    NotFound,
    Expired,
    Clock,
    Io(io::Error),
}

impl fmt::Display for ArtifactError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroLimit => formatter.write_str("artifact retention limits must be positive"),
            Self::InvalidHandle => formatter.write_str("invalid raw artifact handle"),
            Self::NotFound => formatter.write_str("raw artifact not found"),
            Self::Expired => formatter.write_str("raw artifact expired"),
            Self::Clock => formatter.write_str("system clock precedes Unix epoch"),
            Self::Io(error) => write!(formatter, "raw artifact operation failed: {error}"),
        }
    }
}

impl std::error::Error for ArtifactError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use token_shrinker_context::{
        ContentHash, RelevanceSignals, Sensitivity, SourceKind, SourceLocation,
    };

    fn fake_script() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("ts-provider")
            .join("tests/fixtures/fake_provider.py")
    }

    #[test]
    fn headroom_contract_validates_compression_schema() {
        let provider = HeadroomProvider::new(
            PathBuf::from("python"),
            vec![
                fake_script().to_string_lossy().into_owned(),
                "mcp-headroom".to_owned(),
            ],
            false,
        );
        let result = provider.compress("long context").expect("compression");
        assert_eq!(result.provider, "headroom");
        assert_eq!(result.compressed, "short");
        assert_eq!(result.compressed_tokens, Some(2));
    }

    #[test]
    fn rtk_contract_accepts_bounded_utf8_terminal_output() {
        let provider = RtkProvider::from_spec(ProviderSpec {
            id: "rtk".to_owned(),
            command: PathBuf::from("python"),
            base_args: vec![
                fake_script().to_string_lossy().into_owned(),
                "echo".to_owned(),
            ],
            environment: BTreeMap::new(),
            version_requirement: VersionReq::STAR,
            required: false,
            limits: ProviderLimits::default(),
        });
        let result = provider.compress_terminal("line\n").expect("compression");
        assert_eq!(result.compressed.lines().collect::<Vec<_>>(), vec!["line"]);
    }

    #[test]
    #[ignore = "requires locally installed Headroom"]
    fn live_headroom_probe() {
        HeadroomProvider::new(
            PathBuf::from("headroom"),
            vec!["mcp".to_owned(), "serve".to_owned()],
            false,
        )
        .probe()
        .expect("compatible Headroom MCP server");
    }

    #[test]
    #[ignore = "requires locally installed RTK"]
    fn live_rtk_probe() {
        assert!(
            RtkProvider::new(PathBuf::from("rtk"), false)
                .probe()
                .is_ok()
        );
    }

    fn candidate(content: &str) -> ContextCandidate {
        ContextCandidate {
            source_id: SourceId::new("repo:source.rs").expect("source ID"),
            source_kind: SourceKind::RepositoryFile,
            location: SourceLocation {
                uri: "source.rs".to_owned(),
                start_line: None,
                end_line: None,
            },
            content: content.to_owned(),
            content_hash: ContentHash::new("a".repeat(64)).expect("content hash"),
            sensitivity: Sensitivity::Public,
            modified_unix_ms: None,
            relevance: RelevanceSignals::default(),
        }
    }

    #[test]
    fn context_compression_is_verbatim_bounded_and_addressable() {
        let source = candidate("# Heading\nnoise\npub fn target() {\n value\n}\ntail");
        let compressed = compress_context(
            &source,
            &["target".to_owned()],
            1,
            TokenBudget::from_u32(100).expect("budget"),
        );

        assert!(compressed.used_tokens <= 100);
        assert!(
            compressed
                .passages
                .iter()
                .any(|passage| passage.content.contains("pub fn target() {"))
        );
        assert!(!compressed.omitted_ranges.is_empty());
        assert!(
            compressed
                .passages
                .iter()
                .all(|passage| passage.source_id == source.source_id)
        );
    }

    #[test]
    fn terminal_summary_preserves_failure_and_repetition_count() {
        let input = TerminalInput {
            command: vec!["cargo".to_owned(), "test".to_owned()],
            exit_code: Some(1),
            duration: Duration::from_millis(12),
            stdout: "start\nFAILED test_a\nFAILED test_a\ntail".to_owned(),
            stderr: "src/lib.rs:42\nerror: assertion failed".to_owned(),
            truncated: true,
            raw_handle: None,
        };
        let summary = compress_terminal(&input, 1, 1);

        assert_eq!(summary.exit_code, Some(1));
        assert!(summary.truncated);
        assert!(
            summary
                .evidence
                .iter()
                .any(|item| item.line == "FAILED test_a" && item.count == 2)
        );
        assert!(
            summary
                .evidence
                .iter()
                .any(|item| item.line == "error: assertion failed")
        );

        let mut streaming = TerminalCompressor::new(&input, 1, 1);
        streaming.push_stdout(b"FAI");
        streaming.push_stdout(b"LED chunked_test\n");
        streaming.push_stderr(b"src/main.rs:");
        streaming.push_stderr(b"9");
        let streaming = streaming.finish();
        assert!(
            streaming
                .evidence
                .iter()
                .any(|item| item.line == "FAILED chunked_test")
        );
        assert!(
            streaming
                .evidence
                .iter()
                .any(|item| item.line == "src/main.rs:9")
        );
    }

    #[test]
    fn raw_artifacts_are_opaque_bounded_and_fetchable() {
        let directory = tempfile::tempdir().expect("artifact directory");
        let store = RawArtifactStore::open(directory.path(), 1, Duration::from_mins(1))
            .expect("artifact store");
        let first = store.retain(b"first raw output").expect("retain first");
        assert_eq!(
            store.fetch(&first).expect("fetch first"),
            b"first raw output"
        );
        let second = store.retain(b"second raw output").expect("retain second");

        assert!(matches!(store.fetch(&first), Err(ArtifactError::NotFound)));
        assert_eq!(
            store.fetch(&second).expect("fetch second"),
            b"second raw output"
        );
        assert!(!second.as_str().contains("second"));
    }

    #[test]
    fn arbitrary_terminal_and_context_inputs_are_deterministic() {
        let mut state = 0xA5A5_1234_u32;
        for length in 0..512_usize {
            let text = (0..length)
                .map(|_| {
                    state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    char::from_u32(32 + state % 95).expect("printable scalar")
                })
                .collect::<String>();
            let input = TerminalInput {
                command: vec!["fuzz".to_owned()],
                exit_code: Some(1),
                duration: Duration::ZERO,
                stdout: text.clone(),
                stderr: text.clone(),
                truncated: length > 100,
                raw_handle: None,
            };
            assert_eq!(
                compress_terminal(&input, 4, 4),
                compress_terminal(&input, 4, 4)
            );

            let source = candidate(&text);
            let budget = TokenBudget::from_u32(64).expect("budget");
            assert_eq!(
                compress_context(&source, &["error".to_owned()], 2, budget),
                compress_context(&source, &["error".to_owned()], 2, budget)
            );
        }
    }
}
