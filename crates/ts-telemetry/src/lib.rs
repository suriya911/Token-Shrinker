//! Content-free local token and event accounting.

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{Connection, TransactionBehavior, params};
use std::{fmt, path::Path, time::Duration};
use token_shrinker_types::{RequestId, RouteMode};

const SCHEMA_VERSION: i64 = 1;

/// Stable event status stored without diagnostic content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventStatus {
    /// Operation completed normally.
    Success,
    /// Operation completed using a documented fallback.
    Degraded,
    /// Operation failed.
    Failed,
    /// Operation was cancelled.
    Cancelled,
}

impl EventStatus {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Degraded => "degraded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Token flow direction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TokenDirection {
    /// Tokens moving toward a model or tool.
    Input,
    /// Tokens returned from a model or tool.
    Output,
}

impl TokenDirection {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Output => "output",
        }
    }
}

/// Content-free request lifecycle event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestEvent {
    /// Correlation identifier.
    pub request_id: RequestId,
    /// Validated stable session label.
    pub session_id: String,
    /// Validated stable agent label.
    pub agent: String,
    /// Resolved route mode.
    pub mode: RouteMode,
    /// Unix creation time in milliseconds.
    pub started_at_ms: i64,
    /// Monotonic duration in milliseconds.
    pub duration_ms: u64,
    /// Content-free result category.
    pub status: EventStatus,
}

/// Comparable raw/optimized token measurement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TokenEvent {
    /// Owning request.
    pub request_id: RequestId,
    /// Validated pipeline-stage label.
    pub stage: String,
    /// Input or output direction.
    pub direction: TokenDirection,
    /// Comparable unoptimized count.
    pub raw_tokens: u64,
    /// Comparable optimized count.
    pub optimized_tokens: u64,
    /// Validated tokenizer identifier.
    pub tokenizer: String,
    /// Whether the tokenizer reported exact counts.
    pub exact: bool,
    /// Unix event time in milliseconds.
    pub created_at_ms: i64,
}

/// Content-free optional-provider event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderEvent {
    /// Owning request.
    pub request_id: RequestId,
    /// Validated provider identifier.
    pub provider: String,
    /// Validated operation identifier.
    pub operation: String,
    /// Monotonic duration in milliseconds.
    pub duration_ms: u64,
    /// Stable result category.
    pub status: EventStatus,
    /// Optional stable warning code, never a warning message.
    pub warning_code: Option<String>,
}

/// Metadata for separately retained content.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactEvent {
    /// Owning request.
    pub request_id: RequestId,
    /// Stable artifact category.
    pub kind: String,
    /// SHA-256 digest; content itself is never stored here.
    pub content_hash: String,
    /// Raw artifact size.
    pub byte_count: u64,
    /// Unix expiration time in milliseconds.
    pub expires_at_ms: i64,
}

/// Savings aggregate for one comparable tokenizer/exactness group.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TokenSavings {
    /// Tokenizer identifier.
    pub tokenizer: String,
    /// Exact-versus-estimated label.
    pub exact: bool,
    /// Sum of raw comparable counts.
    pub raw_tokens: u64,
    /// Sum of optimized comparable counts.
    pub optimized_tokens: u64,
    /// Signed raw-minus-optimized savings.
    pub savings_tokens: i128,
    /// Number of token events.
    pub event_count: u64,
}

impl TokenSavings {
    /// Returns savings percentage when the comparable raw count is nonzero.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn savings_percent(&self) -> Option<f64> {
        (self.raw_tokens > 0).then(|| {
            let savings = self.savings_tokens as f64;
            savings / self.raw_tokens as f64 * 100.0
        })
    }
}

/// Counts removed by one retention transaction.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PruneReport {
    /// Requests removed; child token/provider/artifact rows cascade.
    pub requests: usize,
    /// Expired artifacts removed independently of request age.
    pub artifacts: usize,
}

/// Bounded, thread-safe `SQLite` telemetry store.
#[derive(Clone)]
pub struct TelemetryStore {
    pool: Pool<SqliteConnectionManager>,
}

impl fmt::Debug for TelemetryStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TelemetryStore")
            .field("max_size", &self.pool.max_size())
            .finish_non_exhaustive()
    }
}

impl TelemetryStore {
    /// Opens an isolated single-connection in-memory store.
    ///
    /// # Errors
    ///
    /// Returns [`TelemetryError`] when pool creation or migration fails.
    pub fn open_in_memory() -> Result<Self, TelemetryError> {
        let manager = SqliteConnectionManager::memory().with_init(|connection| {
            connection.busy_timeout(Duration::from_secs(30))?;
            connection.pragma_update(None, "foreign_keys", true)
        });
        let pool = Pool::builder()
            .max_size(1)
            .connection_timeout(Duration::from_secs(5))
            .build(manager)
            .map_err(TelemetryError::Pool)?;
        {
            let mut connection = pool.get().map_err(TelemetryError::Pool)?;
            migrate(&mut connection)?;
        }
        Ok(Self { pool })
    }

    /// Opens a file-backed pool and completes forward migration before returning.
    ///
    /// # Errors
    ///
    /// Returns [`TelemetryError`] for invalid size, pool, configuration, or migration failure.
    pub fn open(path: impl AsRef<Path>, max_size: u32) -> Result<Self, TelemetryError> {
        if max_size == 0 {
            return Err(TelemetryError::ZeroPoolSize);
        }
        let manager = SqliteConnectionManager::file(path).with_init(|connection| {
            connection.busy_timeout(Duration::from_secs(30))?;
            connection.pragma_update(None, "foreign_keys", true)
        });
        let pool = Pool::builder()
            .max_size(max_size)
            .connection_timeout(Duration::from_secs(5))
            .build(manager)
            .map_err(TelemetryError::Pool)?;
        {
            let mut connection = pool.get().map_err(TelemetryError::Pool)?;
            connection
                .pragma_update(None, "journal_mode", "WAL")
                .map_err(TelemetryError::Sqlite)?;
            migrate(&mut connection)?;
        }
        Ok(Self { pool })
    }

    /// Inserts or replaces one request event atomically.
    ///
    /// # Errors
    ///
    /// Returns [`TelemetryError`] for invalid labels, overflow, checkout, or `SQLite` failure.
    pub fn record_request(&self, event: &RequestEvent) -> Result<(), TelemetryError> {
        validate_label("session_id", &event.session_id)?;
        validate_label("agent", &event.agent)?;
        let duration = to_i64(event.duration_ms)?;
        let mut connection = self.pool.get().map_err(TelemetryError::Pool)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(TelemetryError::Sqlite)?;
        transaction
            .execute(
                "INSERT INTO requests
                 (id, session_id, agent, mode, started_at_ms, duration_ms, status)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(id) DO UPDATE SET
                   session_id=excluded.session_id, agent=excluded.agent, mode=excluded.mode,
                   started_at_ms=excluded.started_at_ms, duration_ms=excluded.duration_ms,
                   status=excluded.status",
                params![
                    event.request_id.as_str(),
                    event.session_id,
                    event.agent,
                    event.mode.as_str(),
                    event.started_at_ms,
                    duration,
                    event.status.as_str()
                ],
            )
            .map_err(TelemetryError::Sqlite)?;
        transaction.commit().map_err(TelemetryError::Sqlite)
    }

    /// Appends one token event.
    ///
    /// # Errors
    ///
    /// Returns [`TelemetryError`] for invalid labels, overflow, missing request, or `SQLite` failure.
    pub fn record_tokens(&self, event: &TokenEvent) -> Result<(), TelemetryError> {
        validate_label("stage", &event.stage)?;
        validate_label("tokenizer", &event.tokenizer)?;
        let connection = self.pool.get().map_err(TelemetryError::Pool)?;
        connection
            .execute(
                "INSERT INTO token_events
                 (request_id, stage, direction, raw_tokens, optimized_tokens,
                  tokenizer, exact, created_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    event.request_id.as_str(),
                    event.stage,
                    event.direction.as_str(),
                    to_i64(event.raw_tokens)?,
                    to_i64(event.optimized_tokens)?,
                    event.tokenizer,
                    event.exact,
                    event.created_at_ms
                ],
            )
            .map_err(TelemetryError::Sqlite)?;
        Ok(())
    }

    /// Appends one provider event.
    ///
    /// # Errors
    ///
    /// Returns [`TelemetryError`] for invalid codes, overflow, missing request, or `SQLite` failure.
    pub fn record_provider(&self, event: &ProviderEvent) -> Result<(), TelemetryError> {
        validate_label("provider", &event.provider)?;
        validate_label("operation", &event.operation)?;
        if let Some(code) = &event.warning_code {
            validate_label("warning_code", code)?;
        }
        let connection = self.pool.get().map_err(TelemetryError::Pool)?;
        connection
            .execute(
                "INSERT INTO provider_events
                 (request_id, provider, operation, duration_ms, status, warning_code)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    event.request_id.as_str(),
                    event.provider,
                    event.operation,
                    to_i64(event.duration_ms)?,
                    event.status.as_str(),
                    event.warning_code
                ],
            )
            .map_err(TelemetryError::Sqlite)?;
        Ok(())
    }

    /// Appends content-free raw artifact metadata.
    ///
    /// # Errors
    ///
    /// Returns [`TelemetryError`] for invalid metadata, overflow, missing request, or `SQLite` failure.
    pub fn record_artifact(&self, event: &ArtifactEvent) -> Result<(), TelemetryError> {
        validate_label("artifact_kind", &event.kind)?;
        validate_hash(&event.content_hash)?;
        let connection = self.pool.get().map_err(TelemetryError::Pool)?;
        connection
            .execute(
                "INSERT INTO artifacts
                 (request_id, kind, content_hash, byte_count, expires_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    event.request_id.as_str(),
                    event.kind,
                    event.content_hash,
                    to_i64(event.byte_count)?,
                    event.expires_at_ms
                ],
            )
            .map_err(TelemetryError::Sqlite)?;
        Ok(())
    }

    /// Returns separate savings rows for every tokenizer and exactness label.
    ///
    /// # Errors
    ///
    /// Returns [`TelemetryError`] when checkout or query decoding fails.
    pub fn token_savings(&self) -> Result<Vec<TokenSavings>, TelemetryError> {
        let connection = self.pool.get().map_err(TelemetryError::Pool)?;
        let mut statement = connection
            .prepare(
                "SELECT tokenizer, exact, SUM(raw_tokens), SUM(optimized_tokens), COUNT(*)
                 FROM token_events GROUP BY tokenizer, exact ORDER BY tokenizer, exact",
            )
            .map_err(TelemetryError::Sqlite)?;
        let rows = statement
            .query_map([], |row| {
                let raw: i64 = row.get(2)?;
                let optimized: i64 = row.get(3)?;
                let count: i64 = row.get(4)?;
                Ok(TokenSavings {
                    tokenizer: row.get(0)?,
                    exact: row.get(1)?,
                    raw_tokens: u64::try_from(raw).unwrap_or_default(),
                    optimized_tokens: u64::try_from(optimized).unwrap_or_default(),
                    savings_tokens: i128::from(raw) - i128::from(optimized),
                    event_count: u64::try_from(count).unwrap_or_default(),
                })
            })
            .map_err(TelemetryError::Sqlite)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(TelemetryError::Sqlite)
    }

    /// Transactionally removes old requests and independently expired artifact metadata.
    ///
    /// # Errors
    ///
    /// Returns [`TelemetryError`] when checkout, deletion, or commit fails.
    pub fn prune(
        &self,
        request_cutoff_ms: i64,
        now_ms: i64,
    ) -> Result<PruneReport, TelemetryError> {
        let mut connection = self.pool.get().map_err(TelemetryError::Pool)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(TelemetryError::Sqlite)?;
        let artifacts = transaction
            .execute("DELETE FROM artifacts WHERE expires_at_ms <= ?1", [now_ms])
            .map_err(TelemetryError::Sqlite)?;
        let requests = transaction
            .execute(
                "DELETE FROM requests WHERE started_at_ms < ?1",
                [request_cutoff_ms],
            )
            .map_err(TelemetryError::Sqlite)?;
        transaction.commit().map_err(TelemetryError::Sqlite)?;
        Ok(PruneReport {
            requests,
            artifacts,
        })
    }

    /// Transactionally deletes one request and all child telemetry.
    ///
    /// # Errors
    ///
    /// Returns [`TelemetryError`] when checkout, deletion, or commit fails.
    pub fn forget_request(&self, request_id: &RequestId) -> Result<bool, TelemetryError> {
        let mut connection = self.pool.get().map_err(TelemetryError::Pool)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(TelemetryError::Sqlite)?;
        let removed = transaction
            .execute("DELETE FROM requests WHERE id = ?1", [request_id.as_str()])
            .map_err(TelemetryError::Sqlite)?;
        transaction.commit().map_err(TelemetryError::Sqlite)?;
        Ok(removed > 0)
    }

    /// Runs `SQLite`'s quick integrity check.
    ///
    /// # Errors
    ///
    /// Returns [`TelemetryError`] for checkout, query, or integrity failure.
    pub fn integrity_check(&self) -> Result<(), TelemetryError> {
        let connection = self.pool.get().map_err(TelemetryError::Pool)?;
        let result: String = connection
            .query_row("PRAGMA quick_check", [], |row| row.get(0))
            .map_err(TelemetryError::Sqlite)?;
        if result == "ok" {
            Ok(())
        } else {
            Err(TelemetryError::Integrity(result))
        }
    }
}

fn migrate(connection: &mut Connection) -> Result<(), TelemetryError> {
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(TelemetryError::Sqlite)?;
    if version > SCHEMA_VERSION {
        return Err(TelemetryError::NewerSchema(version));
    }
    if version == 0 {
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(TelemetryError::Sqlite)?;
        transaction.execute_batch(
            "CREATE TABLE requests (
               id TEXT PRIMARY KEY, session_id TEXT NOT NULL, agent TEXT NOT NULL,
               mode TEXT NOT NULL, started_at_ms INTEGER NOT NULL,
               duration_ms INTEGER NOT NULL, status TEXT NOT NULL
             );
             CREATE TABLE token_events (
               id INTEGER PRIMARY KEY, request_id TEXT NOT NULL REFERENCES requests(id) ON DELETE CASCADE,
               stage TEXT NOT NULL, direction TEXT NOT NULL, raw_tokens INTEGER NOT NULL,
               optimized_tokens INTEGER NOT NULL, tokenizer TEXT NOT NULL,
               exact INTEGER NOT NULL, created_at_ms INTEGER NOT NULL
             );
             CREATE TABLE provider_events (
               id INTEGER PRIMARY KEY, request_id TEXT NOT NULL REFERENCES requests(id) ON DELETE CASCADE,
               provider TEXT NOT NULL, operation TEXT NOT NULL, duration_ms INTEGER NOT NULL,
               status TEXT NOT NULL, warning_code TEXT
             );
             CREATE TABLE artifacts (
               id INTEGER PRIMARY KEY, request_id TEXT NOT NULL REFERENCES requests(id) ON DELETE CASCADE,
               kind TEXT NOT NULL, content_hash TEXT NOT NULL, byte_count INTEGER NOT NULL,
               expires_at_ms INTEGER NOT NULL
             );
             CREATE INDEX requests_started ON requests(started_at_ms);
             CREATE INDEX token_events_grouping ON token_events(tokenizer, exact);
             CREATE INDEX artifacts_expiry ON artifacts(expires_at_ms);"
        ).map_err(TelemetryError::Sqlite)?;
        transaction
            .pragma_update(None, "user_version", SCHEMA_VERSION)
            .map_err(TelemetryError::Sqlite)?;
        transaction.commit().map_err(TelemetryError::Sqlite)?;
    }
    Ok(())
}

fn validate_label(field: &'static str, value: &str) -> Result<(), TelemetryError> {
    let lower = value.to_ascii_lowercase();
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
        || lower.starts_with("sk-")
        || lower.starts_with("ghp_")
        || lower.starts_with("github_pat_")
        || lower.contains("api_key")
    {
        Err(TelemetryError::InvalidLabel(field))
    } else {
        Ok(())
    }
}

fn validate_hash(value: &str) -> Result<(), TelemetryError> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        Ok(())
    } else {
        Err(TelemetryError::InvalidHash)
    }
}

fn to_i64(value: u64) -> Result<i64, TelemetryError> {
    i64::try_from(value).map_err(|_| TelemetryError::IntegerOverflow)
}

/// Telemetry validation, persistence, or integrity failure.
#[derive(Debug)]
pub enum TelemetryError {
    /// Pool size was zero.
    ZeroPoolSize,
    /// Stable identifier contained content-like or unsafe characters.
    InvalidLabel(&'static str),
    /// Artifact digest was not lowercase SHA-256 hexadecimal.
    InvalidHash,
    /// Unsigned value exceeded `SQLite`'s signed integer range.
    IntegerOverflow,
    /// Database schema is newer than this build.
    NewerSchema(i64),
    /// Integrity check returned a non-ok message.
    Integrity(String),
    /// Pool creation or checkout failed.
    Pool(r2d2::Error),
    /// `SQLite` operation failed.
    Sqlite(rusqlite::Error),
}

impl fmt::Display for TelemetryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroPoolSize => formatter.write_str("telemetry pool size must be positive"),
            Self::InvalidLabel(field) => {
                write!(formatter, "invalid content-free telemetry label: {field}")
            }
            Self::InvalidHash => {
                formatter.write_str("artifact hash must be lowercase SHA-256 hexadecimal")
            }
            Self::IntegerOverflow => formatter.write_str("telemetry integer exceeds SQLite range"),
            Self::NewerSchema(version) => write!(
                formatter,
                "telemetry schema {version} is newer than supported schema {SCHEMA_VERSION}"
            ),
            Self::Integrity(result) => {
                write!(formatter, "telemetry integrity check failed: {result}")
            }
            Self::Pool(error) => write!(formatter, "telemetry pool failed: {error}"),
            Self::Sqlite(error) => write!(formatter, "telemetry SQLite operation failed: {error}"),
        }
    }
}

impl std::error::Error for TelemetryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Pool(error) => Some(error),
            Self::Sqlite(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::Arc, thread};

    fn request(id: &str, started_at_ms: i64) -> RequestEvent {
        RequestEvent {
            request_id: RequestId::new(id).expect("request ID"),
            session_id: "session-1".to_owned(),
            agent: "codex".to_owned(),
            mode: RouteMode::Build,
            started_at_ms,
            duration_ms: 12,
            status: EventStatus::Success,
        }
    }

    fn store() -> (tempfile::TempDir, std::path::PathBuf, TelemetryStore) {
        let directory = tempfile::tempdir().expect("telemetry directory");
        let path = directory.path().join("telemetry.db");
        let store = TelemetryStore::open(&path, 4).expect("telemetry store");
        (directory, path, store)
    }

    #[test]
    fn savings_keep_tokenizers_and_precision_separate() {
        let (_directory, _path, store) = store();
        store
            .record_request(&request("request-1", 1))
            .expect("request");
        for (tokenizer, exact, raw, optimized) in [
            ("byte-v1", false, 100, 60),
            ("byte-v1", false, 50, 25),
            ("model-v1", true, 80, 70),
        ] {
            store
                .record_tokens(&TokenEvent {
                    request_id: RequestId::new("request-1").expect("request ID"),
                    stage: "context".to_owned(),
                    direction: TokenDirection::Input,
                    raw_tokens: raw,
                    optimized_tokens: optimized,
                    tokenizer: tokenizer.to_owned(),
                    exact,
                    created_at_ms: 2,
                })
                .expect("token event");
        }

        let savings = store.token_savings().expect("token savings");
        assert_eq!(savings.len(), 2);
        assert_eq!(savings[0].raw_tokens, 150);
        assert_eq!(savings[0].optimized_tokens, 85);
        assert_eq!(savings[0].savings_tokens, 65);
        assert_eq!(savings[1].tokenizer, "model-v1");
        assert!(savings[1].exact);
    }

    #[test]
    fn retention_and_forget_cascade_child_events() {
        let (_directory, _path, store) = store();
        store
            .record_request(&request("old", 1))
            .expect("old request");
        store
            .record_request(&request("new", 100))
            .expect("new request");
        store
            .record_artifact(&ArtifactEvent {
                request_id: RequestId::new("new").expect("request ID"),
                kind: "raw-output".to_owned(),
                content_hash: "a".repeat(64),
                byte_count: 10,
                expires_at_ms: 50,
            })
            .expect("artifact event");

        assert_eq!(
            store.prune(50, 50).expect("prune"),
            PruneReport {
                requests: 1,
                artifacts: 1
            }
        );
        assert!(
            store
                .forget_request(&RequestId::new("new").expect("request ID"))
                .expect("forget")
        );
        assert!(
            !store
                .forget_request(&RequestId::new("missing").expect("request ID"))
                .expect("forget missing")
        );
        store.integrity_check().expect("healthy telemetry");
    }

    #[test]
    fn content_and_secret_shaped_labels_never_reach_database() {
        let (_directory, path, store) = store();
        let mut event = request("request-safe", 1);
        event.agent = "fixture source contains private canary".to_owned();
        assert!(matches!(
            store.record_request(&event),
            Err(TelemetryError::InvalidLabel("agent"))
        ));
        event.agent = "sk-super-secret-token".to_owned();
        assert!(matches!(
            store.record_request(&event),
            Err(TelemetryError::InvalidLabel("agent"))
        ));
        drop(store);

        let database = std::fs::read(path).expect("read telemetry database");
        let database = String::from_utf8_lossy(&database);
        assert!(!database.contains("fixture source contains private canary"));
        assert!(!database.contains("sk-super-secret-token"));
    }

    #[test]
    fn pool_accepts_concurrent_bounded_request_writes() {
        let (_directory, _path, store) = store();
        let store = Arc::new(store);
        let writers = (0..8)
            .map(|index| {
                let store = Arc::clone(&store);
                thread::spawn(move || {
                    store.record_request(&request(&format!("request-{index}"), index))
                })
            })
            .collect::<Vec<_>>();
        for writer in writers {
            writer
                .join()
                .expect("writer thread")
                .expect("request write");
        }
        store
            .integrity_check()
            .expect("healthy concurrent telemetry");
    }

    #[test]
    fn provider_warning_codes_are_codes_not_messages() {
        let (_directory, _path, store) = store();
        store
            .record_request(&request("provider-request", 1))
            .expect("request");
        store
            .record_provider(&ProviderEvent {
                request_id: RequestId::new("provider-request").expect("request ID"),
                provider: "graphify".to_owned(),
                operation: "candidates".to_owned(),
                duration_ms: 4,
                status: EventStatus::Degraded,
                warning_code: Some("provider-timeout".to_owned()),
            })
            .expect("provider event");
        let invalid = ProviderEvent {
            request_id: RequestId::new("provider-request").expect("request ID"),
            provider: "graphify".to_owned(),
            operation: "candidates".to_owned(),
            duration_ms: 4,
            status: EventStatus::Failed,
            warning_code: Some("timed out while reading secret source".to_owned()),
        };
        assert!(matches!(
            store.record_provider(&invalid),
            Err(TelemetryError::InvalidLabel("warning_code"))
        ));
    }
}
