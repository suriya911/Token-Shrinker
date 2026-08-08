//! Native repository scanning and optional graph providers.

use ignore::WalkBuilder;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fmt, fs, io,
    path::{Component, Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::SystemTime,
};
use token_shrinker_context::{
    ContentHash, ContextCandidate, ContextProvider, ContextQuery, RelevanceSignals, Sensitivity,
    SourceId, SourceKind, SourceLocation,
};

/// Default maximum size of one repository text source.
pub const DEFAULT_MAX_FILE_BYTES: u64 = 1_048_576;
/// Default maximum number of files inspected during one scan.
pub const DEFAULT_MAX_FILES: usize = 10_000;
/// Default maximum retained text across one scan.
pub const DEFAULT_MAX_TOTAL_BYTES: u64 = 67_108_864;

/// Query hints used to add deterministic relevance signals during discovery.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RepositoryQuery {
    /// Case-insensitive terms to find in file content.
    pub terms: Vec<String>,
    /// Case-insensitive fragments to find in repository-relative paths.
    pub path_hints: Vec<String>,
}

/// Why a repository entry was not emitted as a context candidate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScanWarningKind {
    /// File exceeded the configured byte cap.
    TooLarge,
    /// File contained a NUL byte and was treated as binary.
    Binary,
    /// Path matched a built-in generated or vendored directory exclusion.
    Generated,
    /// Path could not be represented by the source-handle format.
    UnsupportedPath,
    /// File disappeared or became unreadable during scanning.
    Unreadable,
    /// Canonical path escaped the allowed repository root.
    OutsideRoot,
    /// Scan reached the configured file-count ceiling.
    FileLimitReached,
    /// Scan reached the configured retained-byte ceiling.
    TotalBytesLimitReached,
    /// Caller cancelled the scan.
    Cancelled,
}

/// Non-fatal repository discovery warning.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScanWarning {
    /// Display path associated with the warning.
    pub path: String,
    /// Stable warning category.
    pub kind: ScanWarningKind,
}

/// Deterministic repository discovery result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScanResult {
    /// Text candidates sorted by repository-relative URI.
    pub candidates: Vec<ContextCandidate>,
    /// Non-fatal skips sorted by display path.
    pub warnings: Vec<ScanWarning>,
    /// Content-free provider counters.
    pub trace: RepositoryTrace,
}

/// Content-free native repository trace.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RepositoryTrace {
    /// Number of file entries inspected.
    pub visited_files: u64,
    /// Number of unchanged files served from the content-hash cache.
    pub cache_hits: u64,
    /// Number of sensitive values removed before candidate admission.
    pub redactions: u64,
    /// Number of optional symbol snippets emitted after native discovery.
    pub symbol_candidates: u64,
    /// Whether an explicit cancellation stopped the scan.
    pub cancelled: bool,
}

/// Result returned by a repository redaction policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedactionResult {
    /// Content safe to hash, cache, and rank.
    pub content: String,
    /// Number of values removed.
    pub redactions: u64,
}

/// Pre-cache content policy for repository candidates.
pub trait RedactionPolicy: Send + Sync {
    /// Removes or denies sensitive values before candidate admission.
    fn redact(&self, content: &str) -> RedactionResult;
}

/// One-based inclusive line range reported by an optional symbol extractor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SymbolSpan {
    /// First line of the definition.
    pub start_line: u32,
    /// Last line of the definition.
    pub end_line: u32,
}

/// Optional local symbol extraction hook applied only to admitted native text candidates.
pub trait SymbolExtractor: Send + Sync {
    /// Returns candidate definition spans for one already root-confined source.
    fn spans(&self, source: &ContextCandidate) -> Vec<SymbolSpan>;
}

/// Lightweight definition-line extractor for common source languages.
#[derive(Clone, Copy, Debug, Default)]
pub struct BuiltInDefinitionExtractor;

impl SymbolExtractor for BuiltInDefinitionExtractor {
    fn spans(&self, source: &ContextCandidate) -> Vec<SymbolSpan> {
        source
            .content
            .lines()
            .enumerate()
            .filter_map(|(index, line)| {
                let line = line.trim_start();
                let is_definition = [
                    "class ",
                    "def ",
                    "fn ",
                    "function ",
                    "interface ",
                    "pub fn ",
                    "pub struct ",
                    "struct ",
                ]
                .iter()
                .any(|prefix| line.starts_with(prefix));
                is_definition.then(|| {
                    let line = u32::try_from(index + 1).unwrap_or(u32::MAX);
                    SymbolSpan {
                        start_line: line,
                        end_line: line,
                    }
                })
            })
            .collect()
    }
}

/// Local deterministic redactor for common credential assignments.
#[derive(Clone, Copy, Debug, Default)]
pub struct BuiltInSecretRedactor;

impl RedactionPolicy for BuiltInSecretRedactor {
    fn redact(&self, content: &str) -> RedactionResult {
        redact_assignments(content)
    }
}

/// Root-confined native repository provider.
pub struct RepositoryProvider {
    root: PathBuf,
    max_file_bytes: u64,
    max_files: usize,
    max_total_bytes: u64,
    redaction_policy: Arc<dyn RedactionPolicy>,
    cache: Arc<Mutex<BTreeMap<PathBuf, CachedText>>>,
}

impl fmt::Debug for RepositoryProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RepositoryProvider")
            .field("root", &self.root)
            .field("max_file_bytes", &self.max_file_bytes)
            .field("max_files", &self.max_files)
            .field("max_total_bytes", &self.max_total_bytes)
            .finish_non_exhaustive()
    }
}

impl Clone for RepositoryProvider {
    fn clone(&self) -> Self {
        Self {
            root: self.root.clone(),
            max_file_bytes: self.max_file_bytes,
            max_files: self.max_files,
            max_total_bytes: self.max_total_bytes,
            redaction_policy: Arc::clone(&self.redaction_policy),
            cache: Arc::clone(&self.cache),
        }
    }
}

#[derive(Clone, Debug)]
struct CachedText {
    length: u64,
    modified: Option<SystemTime>,
    content: String,
    content_hash: ContentHash,
    sensitivity: Sensitivity,
    redactions: u64,
}

impl RepositoryProvider {
    /// Opens an existing repository root using the default one-megabyte file cap.
    ///
    /// # Errors
    ///
    /// Returns [`RepositoryError`] when the root cannot be canonicalized or is not a directory.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, RepositoryError> {
        Self::with_limits(
            root,
            DEFAULT_MAX_FILE_BYTES,
            DEFAULT_MAX_FILES,
            DEFAULT_MAX_TOTAL_BYTES,
        )
    }

    /// Opens an existing repository root with an explicit positive file-size cap.
    ///
    /// # Errors
    ///
    /// Returns [`RepositoryError`] for an invalid root or zero byte cap.
    pub fn with_max_file_bytes(
        root: impl AsRef<Path>,
        max_file_bytes: u64,
    ) -> Result<Self, RepositoryError> {
        Self::with_limits(
            root,
            max_file_bytes,
            DEFAULT_MAX_FILES,
            DEFAULT_MAX_TOTAL_BYTES,
        )
    }

    /// Opens a root with explicit positive per-file, file-count, and total-byte ceilings.
    ///
    /// # Errors
    ///
    /// Returns [`RepositoryError`] for an invalid root or any zero limit.
    pub fn with_limits(
        root: impl AsRef<Path>,
        max_file_bytes: u64,
        max_files: usize,
        max_total_bytes: u64,
    ) -> Result<Self, RepositoryError> {
        Self::with_policy_and_limits(
            root,
            max_file_bytes,
            max_files,
            max_total_bytes,
            Arc::new(BuiltInSecretRedactor),
        )
    }

    /// Opens a root with a custom pre-cache redaction policy and default bounds.
    ///
    /// # Errors
    ///
    /// Returns [`RepositoryError`] for an invalid root.
    pub fn with_policy(
        root: impl AsRef<Path>,
        redaction_policy: Arc<dyn RedactionPolicy>,
    ) -> Result<Self, RepositoryError> {
        Self::with_policy_and_limits(
            root,
            DEFAULT_MAX_FILE_BYTES,
            DEFAULT_MAX_FILES,
            DEFAULT_MAX_TOTAL_BYTES,
            redaction_policy,
        )
    }

    fn with_policy_and_limits(
        root: impl AsRef<Path>,
        max_file_bytes: u64,
        max_files: usize,
        max_total_bytes: u64,
        redaction_policy: Arc<dyn RedactionPolicy>,
    ) -> Result<Self, RepositoryError> {
        if max_file_bytes == 0 || max_files == 0 || max_total_bytes == 0 {
            return Err(RepositoryError::ZeroFileLimit);
        }
        let root = fs::canonicalize(root.as_ref()).map_err(RepositoryError::RootIo)?;
        if !root.is_dir() {
            return Err(RepositoryError::RootNotDirectory(root));
        }
        Ok(Self {
            root,
            max_file_bytes,
            max_files,
            max_total_bytes,
            redaction_policy,
            cache: Arc::new(Mutex::new(BTreeMap::new())),
        })
    }

    /// Returns the canonical allowed root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Discovers text files without following symlinks or escaping the allowed root.
    ///
    /// # Errors
    ///
    /// Returns [`RepositoryError`] only when the root walk itself cannot be constructed.
    pub fn scan(&self, query: &RepositoryQuery) -> Result<ScanResult, RepositoryError> {
        self.scan_cancellable(query, &AtomicBool::new(false))
    }

    /// Runs native text/path discovery first, then optionally adds bounded symbol snippets.
    ///
    /// # Errors
    ///
    /// Returns [`RepositoryError`] when native discovery cannot complete.
    pub fn scan_with_symbols(
        &self,
        query: &RepositoryQuery,
        extractor: &dyn SymbolExtractor,
    ) -> Result<ScanResult, RepositoryError> {
        let mut result = self.scan(query)?;
        let native_candidates = result.candidates.clone();
        for source in &native_candidates {
            for span in extractor.spans(source) {
                if let Some(candidate) = symbol_candidate(source, span) {
                    result.candidates.push(candidate);
                    result.trace.symbol_candidates =
                        result.trace.symbol_candidates.saturating_add(1);
                }
            }
        }
        result
            .candidates
            .sort_by(|left, right| left.source_id.cmp(&right.source_id));
        Ok(result)
    }

    /// Discovers repository text while observing an explicit cancellation flag.
    ///
    /// # Errors
    ///
    /// Returns [`RepositoryError`] only when the root walk cannot be constructed.
    pub fn scan_cancellable(
        &self,
        query: &RepositoryQuery,
        cancelled: &AtomicBool,
    ) -> Result<ScanResult, RepositoryError> {
        let walker = WalkBuilder::new(&self.root)
            .hidden(false)
            .follow_links(false)
            .require_git(false)
            .sort_by_file_path(std::cmp::Ord::cmp)
            .build();
        let mut candidates = Vec::new();
        let mut warnings = Vec::new();
        let mut visited_files = 0_usize;
        let mut retained_bytes = 0_u64;
        let mut cache_hits = 0_u64;
        let mut redactions = 0_u64;
        let mut was_cancelled = false;

        for entry in walker {
            if cancelled.load(Ordering::Relaxed) {
                warnings.push(ScanWarning {
                    path: display_path(&self.root),
                    kind: ScanWarningKind::Cancelled,
                });
                was_cancelled = true;
                break;
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    warnings.push(ScanWarning {
                        path: error.to_string(),
                        kind: ScanWarningKind::Unreadable,
                    });
                    continue;
                }
            };
            if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                continue;
            }
            if visited_files == self.max_files {
                warnings.push(ScanWarning {
                    path: display_path(&self.root),
                    kind: ScanWarningKind::FileLimitReached,
                });
                break;
            }
            visited_files += 1;
            if is_likely_generated(&self.root, entry.path()) {
                warnings.push(ScanWarning {
                    path: display_path(entry.path()),
                    kind: ScanWarningKind::Generated,
                });
                continue;
            }
            match self.candidate_from_path(entry.path(), query) {
                Ok(loaded) => {
                    let candidate = loaded.candidate;
                    cache_hits += u64::from(loaded.cache_hit);
                    redactions = redactions.saturating_add(loaded.redactions);
                    let candidate_bytes =
                        u64::try_from(candidate.content.len()).unwrap_or(u64::MAX);
                    if retained_bytes.saturating_add(candidate_bytes) > self.max_total_bytes {
                        warnings.push(ScanWarning {
                            path: candidate.location.uri,
                            kind: ScanWarningKind::TotalBytesLimitReached,
                        });
                        break;
                    }
                    retained_bytes += candidate_bytes;
                    candidates.push(candidate);
                }
                Err(FileSkip { path, kind }) => warnings.push(ScanWarning { path, kind }),
            }
        }

        candidates.sort_by(|left, right| left.location.uri.cmp(&right.location.uri));
        warnings.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(ScanResult {
            candidates,
            warnings,
            trace: RepositoryTrace {
                visited_files: u64::try_from(visited_files).unwrap_or(u64::MAX),
                cache_hits,
                redactions,
                symbol_candidates: 0,
                cancelled: was_cancelled,
            },
        })
    }

    /// Retrieves a previously discovered repository source by opaque handle.
    ///
    /// # Errors
    ///
    /// Returns [`RepositoryError`] for malformed handles, missing files, binary/oversized
    /// content, or any path that does not remain inside the canonical root.
    pub fn fetch(&self, source_id: &SourceId) -> Result<ContextCandidate, RepositoryError> {
        let encoded = source_id
            .as_str()
            .strip_prefix("repo:")
            .ok_or(RepositoryError::InvalidSourceHandle)?;
        let (encoded, symbol_span) = parse_symbol_suffix(encoded)?;
        let relative = decode_relative_path(encoded)?;
        let requested = self.root.join(relative);
        let canonical = fs::canonicalize(&requested).map_err(RepositoryError::SourceIo)?;
        if !canonical.starts_with(&self.root) {
            return Err(RepositoryError::OutsideRoot(canonical));
        }
        let candidate = self
            .candidate_from_path(&canonical, &RepositoryQuery::default())
            .map(|loaded| loaded.candidate)
            .map_err(|skip| RepositoryError::SourceRejected(skip.kind))?;
        if let Some(span) = symbol_span {
            symbol_candidate(&candidate, span).ok_or(RepositoryError::InvalidSourceHandle)
        } else {
            Ok(candidate)
        }
    }

    fn candidate_from_path(
        &self,
        path: &Path,
        query: &RepositoryQuery,
    ) -> Result<LoadedCandidate, FileSkip> {
        let canonical =
            fs::canonicalize(path).map_err(|_| FileSkip::new(path, ScanWarningKind::Unreadable))?;
        if !canonical.starts_with(&self.root) {
            return Err(FileSkip::new(path, ScanWarningKind::OutsideRoot));
        }
        let relative = canonical
            .strip_prefix(&self.root)
            .map_err(|_| FileSkip::new(path, ScanWarningKind::OutsideRoot))?;
        let relative_text = normalized_relative(relative)
            .ok_or_else(|| FileSkip::new(path, ScanWarningKind::UnsupportedPath))?;
        let (cached, cache_hit) = self.load_text(&canonical)?;
        let content = cached.content.clone();
        let path_lower = relative_text.to_lowercase();
        let content_lower = content.to_lowercase();
        let relevance = RelevanceSignals {
            exact_match: query
                .terms
                .iter()
                .filter(|term| !term.is_empty())
                .any(|term| content_lower.contains(&term.to_lowercase())),
            path_match: query
                .path_hints
                .iter()
                .filter(|hint| !hint.is_empty())
                .any(|hint| path_lower.contains(&hint.to_lowercase())),
            diagnostic: false,
            freshness: 0,
        };
        let modified_unix_ms = cached
            .modified
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .and_then(|duration| i64::try_from(duration.as_millis()).ok());

        Ok(LoadedCandidate {
            candidate: ContextCandidate {
                source_id: SourceId::new(format!("repo:{}", encode_path(&relative_text)))
                    .map_err(|_| FileSkip::new(path, ScanWarningKind::UnsupportedPath))?,
                source_kind: SourceKind::RepositoryFile,
                location: SourceLocation {
                    uri: relative_text,
                    start_line: None,
                    end_line: None,
                },
                content_hash: cached.content_hash,
                sensitivity: cached.sensitivity,
                content,
                modified_unix_ms,
                relevance,
            },
            cache_hit,
            redactions: cached.redactions,
        })
    }

    fn load_text(&self, path: &Path) -> Result<(CachedText, bool), FileSkip> {
        for attempt in 0..2 {
            let before =
                fs::metadata(path).map_err(|_| FileSkip::new(path, ScanWarningKind::Unreadable))?;
            if before.len() > self.max_file_bytes {
                return Err(FileSkip::new(path, ScanWarningKind::TooLarge));
            }
            let modified = before.modified().ok();
            if let Ok(cache) = self.cache.lock()
                && let Some(cached) = cache.get(path)
                && cached.length == before.len()
                && cached.modified == modified
            {
                return Ok((cached.clone(), true));
            }

            let bytes =
                fs::read(path).map_err(|_| FileSkip::new(path, ScanWarningKind::Unreadable))?;
            if bytes.contains(&0) {
                return Err(FileSkip::new(path, ScanWarningKind::Binary));
            }
            let raw = String::from_utf8(bytes)
                .map_err(|_| FileSkip::new(path, ScanWarningKind::Binary))?;
            let after =
                fs::metadata(path).map_err(|_| FileSkip::new(path, ScanWarningKind::Unreadable))?;
            if before.len() != after.len() || modified != after.modified().ok() {
                if attempt == 0 {
                    continue;
                }
                return Err(FileSkip::new(path, ScanWarningKind::Unreadable));
            }

            let redacted = self.redaction_policy.redact(&raw);
            let final_metadata =
                fs::metadata(path).map_err(|_| FileSkip::new(path, ScanWarningKind::Unreadable))?;
            let final_modified = final_metadata.modified().ok();
            if after.len() != final_metadata.len() || after.modified().ok() != final_modified {
                if attempt == 0 {
                    continue;
                }
                return Err(FileSkip::new(path, ScanWarningKind::Unreadable));
            }
            let cached = CachedText {
                length: final_metadata.len(),
                modified: final_modified,
                content_hash: hash_content(redacted.content.as_bytes()),
                sensitivity: if redacted.redactions > 0 {
                    Sensitivity::Redacted
                } else {
                    Sensitivity::Public
                },
                content: redacted.content,
                redactions: redacted.redactions,
            };
            if let Ok(mut cache) = self.cache.lock() {
                cache.insert(path.to_owned(), cached.clone());
            }
            return Ok((cached, false));
        }
        Err(FileSkip::new(path, ScanWarningKind::Unreadable))
    }
}

impl ContextProvider for RepositoryProvider {
    type Error = RepositoryError;

    fn candidates(&self, query: &ContextQuery) -> Result<Vec<ContextCandidate>, Self::Error> {
        self.scan(&RepositoryQuery {
            terms: query.terms.clone(),
            path_hints: query.path_hints.clone(),
        })
        .map(|result| result.candidates)
    }

    fn fetch(&self, source_id: &SourceId) -> Result<ContextCandidate, Self::Error> {
        RepositoryProvider::fetch(self, source_id)
    }
}

#[derive(Debug)]
struct LoadedCandidate {
    candidate: ContextCandidate,
    cache_hit: bool,
    redactions: u64,
}

#[derive(Debug)]
struct FileSkip {
    path: String,
    kind: ScanWarningKind,
}

impl FileSkip {
    fn new(path: &Path, kind: ScanWarningKind) -> Self {
        Self {
            path: display_path(path),
            kind,
        }
    }
}

/// Fatal repository-provider error.
#[derive(Debug)]
pub enum RepositoryError {
    /// One or more scan limits were zero.
    ZeroFileLimit,
    /// Root could not be canonicalized.
    RootIo(io::Error),
    /// Canonical root was not a directory.
    RootNotDirectory(PathBuf),
    /// Source handle did not belong to the repository provider.
    InvalidSourceHandle,
    /// Percent-encoded source handle was malformed or unsafe.
    InvalidEncodedPath,
    /// Source could not be canonicalized or read.
    SourceIo(io::Error),
    /// Canonical source escaped the allowed root.
    OutsideRoot(PathBuf),
    /// Source exists but violates the provider's text policy.
    SourceRejected(ScanWarningKind),
}

impl fmt::Display for RepositoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroFileLimit => formatter.write_str("repository scan limits must be positive"),
            Self::RootIo(error) => write!(formatter, "failed to open repository root: {error}"),
            Self::RootNotDirectory(path) => write!(
                formatter,
                "repository root is not a directory: {}",
                path.display()
            ),
            Self::InvalidSourceHandle => {
                formatter.write_str("source handle does not belong to repository provider")
            }
            Self::InvalidEncodedPath => {
                formatter.write_str("source handle contains an invalid encoded path")
            }
            Self::SourceIo(error) => write!(formatter, "failed to open repository source: {error}"),
            Self::OutsideRoot(path) => write!(
                formatter,
                "repository source escaped allowed root: {}",
                path.display()
            ),
            Self::SourceRejected(kind) => {
                write!(formatter, "repository source was rejected: {kind:?}")
            }
        }
    }
}

impl std::error::Error for RepositoryError {}

fn normalized_relative(path: &Path) -> Option<String> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str()?.to_owned()),
            _ => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn encode_path(path: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(path.len());
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    encoded
}

fn decode_relative_path(encoded: &str) -> Result<PathBuf, RepositoryError> {
    let bytes = encoded.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err(RepositoryError::InvalidEncodedPath);
            }
            let high = hex_value(bytes[index + 1]).ok_or(RepositoryError::InvalidEncodedPath)?;
            let low = hex_value(bytes[index + 2]).ok_or(RepositoryError::InvalidEncodedPath)?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    let decoded = String::from_utf8(decoded).map_err(|_| RepositoryError::InvalidEncodedPath)?;
    let mut path = PathBuf::new();
    for part in decoded.split('/') {
        if part.is_empty() || matches!(part, "." | "..") {
            return Err(RepositoryError::InvalidEncodedPath);
        }
        path.push(part);
    }
    Ok(path)
}

const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn hash_content(content: &[u8]) -> ContentHash {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(content);
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    ContentHash::new(encoded).expect("SHA-256 formatter emits lowercase hexadecimal")
}

fn redact_assignments(content: &str) -> RedactionResult {
    let mut output = String::with_capacity(content.len());
    let mut redactions = 0_u64;
    for segment in content.split_inclusive('\n') {
        let (line, newline) = segment
            .strip_suffix('\n')
            .map_or((segment, ""), |line| (line, "\n"));
        if let Some(separator) = line.find(['=', ':']) {
            let key = line[..separator]
                .chars()
                .filter(char::is_ascii_alphanumeric)
                .flat_map(char::to_lowercase)
                .collect::<String>();
            if is_sensitive_key(&key) && !line[separator + 1..].trim().is_empty() {
                let value = line[separator + 1..].trim();
                output.push_str(&line[..=separator]);
                if value.starts_with(['"', '\'']) {
                    output.push_str(" \"[REDACTED]\"");
                    if value.ends_with(',') {
                        output.push(',');
                    }
                } else {
                    output.push_str("[REDACTED]");
                }
                output.push_str(newline);
                redactions += 1;
                continue;
            }
        }
        output.push_str(line);
        output.push_str(newline);
    }
    RedactionResult {
        content: output,
        redactions,
    }
}

fn symbol_candidate(source: &ContextCandidate, span: SymbolSpan) -> Option<ContextCandidate> {
    if span.start_line == 0 || span.end_line < span.start_line {
        return None;
    }
    let start = usize::try_from(span.start_line - 1).ok()?;
    let count = usize::try_from(span.end_line - span.start_line + 1).ok()?;
    let content = source
        .content
        .lines()
        .skip(start)
        .take(count)
        .collect::<Vec<_>>()
        .join("\n");
    if content.is_empty() {
        return None;
    }
    let source_id = SourceId::new(format!(
        "{}#L{}-L{}",
        source.source_id.as_str(),
        span.start_line,
        span.end_line
    ))
    .ok()?;
    Some(ContextCandidate {
        source_id,
        source_kind: SourceKind::RepositorySymbol,
        location: SourceLocation {
            uri: source.location.uri.clone(),
            start_line: Some(span.start_line),
            end_line: Some(span.end_line),
        },
        content_hash: hash_content(content.as_bytes()),
        sensitivity: source.sensitivity,
        content,
        modified_unix_ms: source.modified_unix_ms,
        relevance: source.relevance,
    })
}

fn parse_symbol_suffix(encoded: &str) -> Result<(&str, Option<SymbolSpan>), RepositoryError> {
    let Some((path, range)) = encoded.rsplit_once("#L") else {
        return Ok((encoded, None));
    };
    let (start, end) = range
        .split_once("-L")
        .ok_or(RepositoryError::InvalidSourceHandle)?;
    let start_line = start
        .parse::<u32>()
        .map_err(|_| RepositoryError::InvalidSourceHandle)?;
    let end_line = end
        .parse::<u32>()
        .map_err(|_| RepositoryError::InvalidSourceHandle)?;
    if start_line == 0 || end_line < start_line {
        return Err(RepositoryError::InvalidSourceHandle);
    }
    Ok((
        path,
        Some(SymbolSpan {
            start_line,
            end_line,
        }),
    ))
}

fn is_sensitive_key(key: &str) -> bool {
    [
        "password",
        "secret",
        "token",
        "apikey",
        "accesstoken",
        "privatekey",
    ]
    .iter()
    .any(|suffix| key.ends_with(suffix))
}

fn is_likely_generated(root: &Path, path: &Path) -> bool {
    const EXCLUDED_DIRECTORIES: &[&str] = &[
        "build",
        "dist",
        "generated",
        "node_modules",
        "target",
        "vendor",
    ];
    path.strip_prefix(root)
        .ok()
        .and_then(Path::parent)
        .is_some_and(|parent| {
            parent.components().any(|component| {
                let name = component.as_os_str().to_string_lossy().to_ascii_lowercase();
                EXCLUDED_DIRECTORIES.contains(&name.as_str())
            })
        })
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_respects_ignores_limits_binary_policy_and_order() {
        let directory = tempfile::tempdir().expect("temporary repository");
        fs::write(directory.path().join(".gitignore"), "ignored.txt\n").expect("write ignore file");
        fs::write(directory.path().join("b.rs"), "fn beta() {}").expect("write source");
        fs::write(directory.path().join("a.rs"), "fn alpha() {}").expect("write source");
        fs::write(directory.path().join("ignored.txt"), "ignored").expect("write ignored source");
        fs::write(directory.path().join("binary.bin"), b"a\0b").expect("write binary fixture");
        fs::write(
            directory.path().join("large.txt"),
            "012345678901234567890123456789",
        )
        .expect("write large fixture");
        let provider =
            RepositoryProvider::with_max_file_bytes(directory.path(), 20).expect("open repository");

        let result = provider
            .scan(&RepositoryQuery {
                terms: vec!["alpha".to_owned()],
                path_hints: vec!["a.rs".to_owned()],
            })
            .expect("scan repository");

        assert_eq!(
            result
                .candidates
                .iter()
                .map(|candidate| candidate.location.uri.as_str())
                .collect::<Vec<_>>(),
            vec![".gitignore", "a.rs", "b.rs"]
        );
        assert_eq!(result.warnings.len(), 2);
    }

    #[test]
    fn discovered_source_can_be_fetched_by_handle() {
        let directory = tempfile::tempdir().expect("temporary repository");
        fs::create_dir(directory.path().join("src")).expect("create source directory");
        fs::write(directory.path().join("src/main.rs"), "fn main() {}").expect("write source");
        let provider = RepositoryProvider::open(directory.path()).expect("open repository");
        let scan = provider
            .scan(&RepositoryQuery::default())
            .expect("scan repository");
        let source = scan
            .candidates
            .iter()
            .find(|candidate| candidate.location.uri == "src/main.rs")
            .expect("discovered source");

        let fetched = provider.fetch(&source.source_id).expect("fetch source");

        assert_eq!(fetched.content, "fn main() {}");
        assert_eq!(fetched.content_hash, source.content_hash);
    }

    #[test]
    fn malicious_source_handle_cannot_escape_root() {
        let directory = tempfile::tempdir().expect("temporary repository");
        let provider = RepositoryProvider::open(directory.path()).expect("open repository");
        let source = SourceId::new("repo:../outside.txt").expect("syntactically valid handle");

        assert!(matches!(
            provider.fetch(&source),
            Err(RepositoryError::InvalidEncodedPath)
        ));
    }

    #[test]
    fn scan_stops_at_deterministic_file_limit() {
        let directory = tempfile::tempdir().expect("temporary repository");
        fs::write(directory.path().join("a.txt"), "a").expect("write source");
        fs::write(directory.path().join("b.txt"), "b").expect("write source");
        let provider =
            RepositoryProvider::with_limits(directory.path(), 10, 1, 10).expect("open repository");

        let result = provider
            .scan(&RepositoryQuery::default())
            .expect("scan repository");

        assert_eq!(result.candidates[0].location.uri, "a.txt");
        assert_eq!(result.warnings[0].kind, ScanWarningKind::FileLimitReached);
    }

    #[test]
    fn secrets_are_redacted_before_hash_cache_and_candidate_admission() {
        let directory = tempfile::tempdir().expect("temporary repository");
        let path = directory.path().join("secrets.env");
        fs::write(&path, "API_KEY=super-secret\nname=public\n").expect("write secret fixture");
        let provider = RepositoryProvider::open(directory.path()).expect("open repository");

        let first = provider
            .scan(&RepositoryQuery::default())
            .expect("first scan");
        let candidate = &first.candidates[0];
        let first_hash = candidate.content_hash.clone();
        assert!(!candidate.content.contains("super-secret"));
        assert!(candidate.content.contains("[REDACTED]"));
        assert_eq!(candidate.sensitivity, Sensitivity::Redacted);
        assert_eq!(first.trace.redactions, 1);
        assert_eq!(first.trace.cache_hits, 0);

        let second = provider
            .scan(&RepositoryQuery::default())
            .expect("cached scan");
        assert_eq!(second.trace.cache_hits, 1);

        fs::write(&path, "API_KEY=replaced-longer-secret\nname=public\n")
            .expect("replace secret fixture");
        let changed = provider
            .scan(&RepositoryQuery::default())
            .expect("changed scan");
        assert_eq!(changed.trace.cache_hits, 0);
        assert_eq!(changed.candidates[0].content_hash, first_hash);
    }

    #[test]
    fn cancellation_stops_before_candidate_admission() {
        let directory = tempfile::tempdir().expect("temporary repository");
        fs::write(directory.path().join("source.txt"), "content").expect("write source");
        let provider = RepositoryProvider::open(directory.path()).expect("open repository");
        let cancelled = AtomicBool::new(true);

        let result = provider
            .scan_cancellable(&RepositoryQuery::default(), &cancelled)
            .expect("cancelled scan");

        assert!(result.candidates.is_empty());
        assert!(result.trace.cancelled);
        assert_eq!(result.warnings[0].kind, ScanWarningKind::Cancelled);
    }

    #[test]
    fn unicode_paths_and_case_insensitive_hints_are_fetchable() {
        let directory = tempfile::tempdir().expect("temporary repository");
        fs::write(directory.path().join("café.rs"), "Needle value").expect("write source");
        let provider = RepositoryProvider::open(directory.path()).expect("open repository");

        let result = provider
            .scan(&RepositoryQuery {
                terms: vec!["needle".to_owned()],
                path_hints: vec!["CAFÉ".to_owned()],
            })
            .expect("scan repository");
        let candidate = &result.candidates[0];

        assert!(candidate.relevance.exact_match);
        assert!(candidate.relevance.path_match);
        assert!(candidate.source_id.as_str().contains('%'));
        assert_eq!(
            provider
                .fetch(&candidate.source_id)
                .expect("fetch Unicode source")
                .content,
            "Needle value"
        );
    }

    #[test]
    fn generated_and_vendored_directories_are_excluded_by_default() {
        let directory = tempfile::tempdir().expect("temporary repository");
        fs::create_dir(directory.path().join("generated")).expect("create generated directory");
        fs::create_dir(directory.path().join("vendor")).expect("create vendor directory");
        fs::write(directory.path().join("source.rs"), "fn source() {}").expect("write source");
        fs::write(directory.path().join("generated/code.rs"), "generated")
            .expect("write generated source");
        fs::write(directory.path().join("vendor/dependency.rs"), "vendored")
            .expect("write vendored source");
        let provider = RepositoryProvider::open(directory.path()).expect("open repository");

        let result = provider
            .scan(&RepositoryQuery::default())
            .expect("scan repository");

        assert_eq!(result.candidates.len(), 1);
        assert_eq!(result.candidates[0].location.uri, "source.rs");
        assert_eq!(
            result
                .warnings
                .iter()
                .filter(|warning| warning.kind == ScanWarningKind::Generated)
                .count(),
            2
        );
    }

    #[test]
    fn optional_symbols_are_derived_only_from_native_candidates_and_fetchable() {
        let directory = tempfile::tempdir().expect("temporary repository");
        fs::write(
            directory.path().join("source.rs"),
            "// header\npub fn admitted() {}\n",
        )
        .expect("write source");
        let provider = RepositoryProvider::open(directory.path()).expect("open repository");

        let result = provider
            .scan_with_symbols(
                &RepositoryQuery {
                    terms: vec!["admitted".to_owned()],
                    path_hints: Vec::new(),
                },
                &BuiltInDefinitionExtractor,
            )
            .expect("scan with optional symbols");
        let symbol = result
            .candidates
            .iter()
            .find(|candidate| candidate.source_kind == SourceKind::RepositorySymbol)
            .expect("derived symbol candidate");

        assert_eq!(result.trace.symbol_candidates, 1);
        assert_eq!(symbol.location.start_line, Some(2));
        assert_eq!(symbol.content, "pub fn admitted() {}");
        assert_eq!(
            provider
                .fetch(&symbol.source_id)
                .expect("fetch symbol by source ref")
                .content,
            symbol.content
        );
    }

    #[test]
    fn file_changed_during_read_is_retried_before_cache_admission() {
        #[derive(Debug)]
        struct ChangeOnce {
            path: PathBuf,
            changed: AtomicBool,
        }

        impl RedactionPolicy for ChangeOnce {
            fn redact(&self, content: &str) -> RedactionResult {
                if !self.changed.swap(true, Ordering::SeqCst) {
                    fs::write(&self.path, "new content after concurrent change")
                        .expect("replace source during scan");
                }
                RedactionResult {
                    content: content.to_owned(),
                    redactions: 0,
                }
            }
        }

        let directory = tempfile::tempdir().expect("temporary repository");
        let path = directory.path().join("changing.txt");
        fs::write(&path, "old").expect("write initial source");
        let policy = Arc::new(ChangeOnce {
            path,
            changed: AtomicBool::new(false),
        });
        let provider = RepositoryProvider::with_policy(directory.path(), policy)
            .expect("open repository with changing policy");

        let result = provider
            .scan(&RepositoryQuery::default())
            .expect("scan changing repository");

        assert_eq!(
            result.candidates[0].content,
            "new content after concurrent change"
        );
        assert_eq!(result.trace.cache_hits, 0);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_to_outside_root_is_never_admitted() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().expect("temporary repository");
        let outside = tempfile::NamedTempFile::new().expect("outside source");
        symlink(outside.path(), directory.path().join("escape.txt")).expect("create symlink");
        let provider = RepositoryProvider::open(directory.path()).expect("open repository");

        let result = provider
            .scan(&RepositoryQuery::default())
            .expect("scan repository");
        assert!(result.candidates.is_empty());
        let source = SourceId::new("repo:escape.txt").expect("source handle");
        assert!(matches!(
            provider.fetch(&source),
            Err(RepositoryError::OutsideRoot(_))
        ));
    }

    #[cfg(windows)]
    #[test]
    fn symlink_to_outside_root_is_never_admitted_when_supported() {
        use std::os::windows::fs::symlink_file;

        let directory = tempfile::tempdir().expect("temporary repository");
        let outside = tempfile::NamedTempFile::new().expect("outside source");
        if symlink_file(outside.path(), directory.path().join("escape.txt")).is_err() {
            return;
        }
        let provider = RepositoryProvider::open(directory.path()).expect("open repository");
        let result = provider
            .scan(&RepositoryQuery::default())
            .expect("scan repository");
        assert!(result.candidates.is_empty());
        let source = SourceId::new("repo:escape.txt").expect("source handle");
        assert!(matches!(
            provider.fetch(&source),
            Err(RepositoryError::OutsideRoot(_))
        ));
    }
}
