//! Memory providers and built-in `SQLite` persistence.

use r2d2::{Pool, PooledConnection};
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{Connection, Transaction, TransactionBehavior, params};
use sha2::{Digest, Sha256};
use std::{fmt, path::Path, time::Duration};
use token_shrinker_context::{
    ContentHash, ContextCandidate, RelevanceSignals, Sensitivity, SourceId, SourceKind,
    SourceLocation,
};

const SCHEMA_VERSION: i64 = 1;

/// Isolation boundary for one memory record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MemoryScope {
    /// Available across repositories for the current user.
    User,
    /// Available only inside one stable repository identity.
    Repository(String),
}

impl MemoryScope {
    fn database_parts(&self) -> Result<(&'static str, &str), MemoryError> {
        match self {
            Self::User => Ok(("user", "")),
            Self::Repository(key) if !key.is_empty() => Ok(("repository", key)),
            Self::Repository(_) => Err(MemoryError::EmptyRepositoryScope),
        }
    }
}

/// Record accepted by the built-in memory provider.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewMemory {
    /// Caller-supplied stable printable ASCII identifier.
    pub id: String,
    /// User or repository isolation boundary.
    pub scope: MemoryScope,
    /// Memory body stored locally in `SQLite`.
    pub content: String,
    /// Creation time in Unix milliseconds.
    pub created_at_unix_ms: i64,
    /// Optional expiration time in Unix milliseconds.
    pub expires_at_unix_ms: Option<i64>,
}

/// Stored memory returned by deterministic queries.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryRecord {
    /// Stable memory identifier.
    pub id: String,
    /// Isolation boundary.
    pub scope: MemoryScope,
    /// Stored body.
    pub content: String,
    /// Creation time in Unix milliseconds.
    pub created_at_unix_ms: i64,
    /// Optional expiration time in Unix milliseconds.
    pub expires_at_unix_ms: Option<i64>,
}

impl MemoryRecord {
    /// Converts this record into an addressable context candidate.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError`] when the stored identifier cannot form a source handle.
    pub fn into_candidate(self) -> Result<ContextCandidate, MemoryError> {
        let source_id = SourceId::new(format!("memory:{}", self.id))
            .map_err(|_| MemoryError::InvalidMemoryId)?;
        Ok(ContextCandidate {
            source_id,
            source_kind: SourceKind::Memory,
            location: SourceLocation {
                uri: format!("memory:{}", self.id),
                start_line: None,
                end_line: None,
            },
            content_hash: hash_content(self.content.as_bytes()),
            sensitivity: Sensitivity::Private,
            content: self.content,
            modified_unix_ms: Some(self.created_at_unix_ms),
            relevance: RelevanceSignals {
                exact_match: true,
                ..RelevanceSignals::default()
            },
        })
    }
}

/// SQLite-backed local memory store.
pub struct MemoryStore {
    connection: Connection,
    fts5_available: bool,
}

/// Bounded, thread-safe pool for file-backed memory access.
#[derive(Clone)]
pub struct MemoryPool {
    pool: Pool<SqliteConnectionManager>,
    fts5_available: bool,
}

impl fmt::Debug for MemoryPool {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryPool")
            .field("max_size", &self.pool.max_size())
            .field("fts5_available", &self.fts5_available)
            .finish_non_exhaustive()
    }
}

impl MemoryPool {
    /// Opens a bounded pool and completes all migrations before returning it.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError`] for a zero-sized pool, connection failure, or migration failure.
    pub fn open(path: impl AsRef<Path>, max_size: u32) -> Result<Self, MemoryError> {
        if max_size == 0 {
            return Err(MemoryError::ZeroPoolSize);
        }
        let manager = SqliteConnectionManager::file(path).with_init(|connection| {
            connection.busy_timeout(Duration::from_secs(30))?;
            connection.pragma_update(None, "foreign_keys", true)
        });
        let pool = Pool::builder()
            .max_size(max_size)
            .connection_timeout(Duration::from_secs(5))
            .build(manager)
            .map_err(MemoryError::Pool)?;

        let fts5_available = {
            let mut connection = pool.get().map_err(MemoryError::Pool)?;
            connection
                .pragma_update(None, "journal_mode", "WAL")
                .map_err(MemoryError::Sqlite)?;
            migrate(&mut connection)?;
            let available = detect_fts5(&connection);
            if available {
                create_fts5(&connection).map_err(MemoryError::Sqlite)?;
            }
            available
        };
        Ok(Self {
            pool,
            fts5_available,
        })
    }

    /// Returns whether pooled connections expose FTS5.
    #[must_use]
    pub const fn fts5_available(&self) -> bool {
        self.fts5_available
    }

    /// Atomically inserts or replaces one memory using a pooled connection.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError`] for invalid input, checkout failure, or a `SQLite` failure.
    pub fn remember(&self, memory: &NewMemory) -> Result<(), MemoryError> {
        let mut connection = self.connection()?;
        remember_on(&mut connection, memory)
    }

    /// Searches one scope using a pooled connection.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError`] for an invalid scope, checkout failure, or a query failure.
    pub fn search(
        &self,
        scope: &MemoryScope,
        query: &str,
        now_unix_ms: i64,
        limit: u32,
    ) -> Result<Vec<MemoryRecord>, MemoryError> {
        let connection = self.connection()?;
        search_on(
            &connection,
            self.fts5_available,
            scope,
            query,
            now_unix_ms,
            limit,
        )
    }

    /// Deletes one memory transactionally using a pooled connection.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError`] when checkout, deletion, or commit fails.
    pub fn forget(&self, id: &str) -> Result<bool, MemoryError> {
        let mut connection = self.connection()?;
        forget_on(&mut connection, id)
    }

    /// Transactionally removes expired memories using a pooled connection.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError`] when checkout, deletion, or commit fails.
    pub fn prune_expired(&self, now_unix_ms: i64) -> Result<usize, MemoryError> {
        let mut connection = self.connection()?;
        prune_expired_on(&mut connection, now_unix_ms)
    }

    /// Runs `SQLite`'s quick integrity check using a pooled connection.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError`] when checkout or the integrity query fails.
    pub fn integrity_check(&self) -> Result<(), MemoryError> {
        let connection = self.connection()?;
        integrity_check_on(&connection)
    }

    fn connection(&self) -> Result<PooledConnection<SqliteConnectionManager>, MemoryError> {
        self.pool.get().map_err(MemoryError::Pool)
    }
}

impl fmt::Debug for MemoryStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryStore")
            .field("fts5_available", &self.fts5_available)
            .finish_non_exhaustive()
    }
}

impl MemoryStore {
    /// Opens or creates a file-backed database and applies forward migrations.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError`] when `SQLite` cannot open, configure, or migrate the database.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, MemoryError> {
        let connection = Connection::open(path).map_err(MemoryError::Sqlite)?;
        Self::from_connection(connection)
    }

    /// Creates an isolated in-memory database.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError`] when `SQLite` configuration or migration fails.
    pub fn open_in_memory() -> Result<Self, MemoryError> {
        let connection = Connection::open_in_memory().map_err(MemoryError::Sqlite)?;
        Self::from_connection(connection)
    }

    fn from_connection(mut connection: Connection) -> Result<Self, MemoryError> {
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(MemoryError::Sqlite)?;
        connection
            .pragma_update(None, "foreign_keys", true)
            .map_err(MemoryError::Sqlite)?;
        migrate(&mut connection)?;
        let fts5_available = detect_fts5(&connection);
        if fts5_available {
            create_fts5(&connection).map_err(MemoryError::Sqlite)?;
        }
        Ok(Self {
            connection,
            fts5_available,
        })
    }

    /// Returns whether this `SQLite` build exposes FTS5.
    #[must_use]
    pub const fn fts5_available(&self) -> bool {
        self.fts5_available
    }

    /// Inserts or replaces one scoped memory atomically.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError`] for invalid input or a `SQLite` failure.
    pub fn remember(&mut self, memory: &NewMemory) -> Result<(), MemoryError> {
        validate_memory(memory)?;
        let (scope_kind, scope_key) = memory.scope.database_parts()?;
        let transaction = self.connection.transaction().map_err(MemoryError::Sqlite)?;
        transaction
            .execute(
                "INSERT INTO memories (
                    id, scope_kind, scope_key, content, created_at_ms, expires_at_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(id) DO UPDATE SET
                    scope_kind = excluded.scope_kind,
                    scope_key = excluded.scope_key,
                    content = excluded.content,
                    created_at_ms = excluded.created_at_ms,
                    expires_at_ms = excluded.expires_at_ms",
                params![
                    memory.id,
                    scope_kind,
                    scope_key,
                    memory.content,
                    memory.created_at_unix_ms,
                    memory.expires_at_unix_ms
                ],
            )
            .map_err(MemoryError::Sqlite)?;
        transaction.commit().map_err(MemoryError::Sqlite)
    }

    /// Searches one scope in deterministic relevance/time/ID order.
    ///
    /// FTS5 is used when available. Otherwise a deterministic substring fallback is used.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError`] for an invalid scope or `SQLite` query failure.
    pub fn search(
        &self,
        scope: &MemoryScope,
        query: &str,
        now_unix_ms: i64,
        limit: u32,
    ) -> Result<Vec<MemoryRecord>, MemoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let (scope_kind, scope_key) = scope.database_parts()?;
        if self.fts5_available && !query.trim().is_empty() {
            let fts_query = format!("\"{}\"", query.replace('"', "\"\""));
            let mut statement = self
                .connection
                .prepare(
                    "SELECT m.id, m.scope_kind, m.scope_key, m.content,
                            m.created_at_ms, m.expires_at_ms
                     FROM memory_fts
                     JOIN memories AS m ON m.rowid = memory_fts.rowid
                     WHERE memory_fts MATCH ?1
                       AND m.scope_kind = ?2
                       AND m.scope_key = ?3
                       AND (m.expires_at_ms IS NULL OR m.expires_at_ms > ?4)
                     ORDER BY bm25(memory_fts), m.created_at_ms DESC, m.id ASC
                     LIMIT ?5",
                )
                .map_err(MemoryError::Sqlite)?;
            return collect_records(
                &mut statement,
                params![fts_query, scope_kind, scope_key, now_unix_ms, limit],
            );
        }

        let mut statement = self
            .connection
            .prepare(
                "SELECT id, scope_kind, scope_key, content, created_at_ms, expires_at_ms
                 FROM memories
                 WHERE scope_kind = ?1
                   AND scope_key = ?2
                   AND (?3 = '' OR instr(lower(content), lower(?3)) > 0)
                   AND (expires_at_ms IS NULL OR expires_at_ms > ?4)
                 ORDER BY created_at_ms DESC, id ASC
                 LIMIT ?5",
            )
            .map_err(MemoryError::Sqlite)?;
        collect_records(
            &mut statement,
            params![scope_kind, scope_key, query, now_unix_ms, limit],
        )
    }

    /// Deletes one memory inside an explicit transaction.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError`] when `SQLite` cannot delete or commit.
    pub fn forget(&mut self, id: &str) -> Result<bool, MemoryError> {
        let transaction = self.connection.transaction().map_err(MemoryError::Sqlite)?;
        let deleted = transaction
            .execute("DELETE FROM memories WHERE id = ?1", [id])
            .map_err(MemoryError::Sqlite)?;
        transaction.commit().map_err(MemoryError::Sqlite)?;
        Ok(deleted > 0)
    }

    /// Transactionally removes records whose expiration is not later than now.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError`] when `SQLite` cannot delete or commit.
    pub fn prune_expired(&mut self, now_unix_ms: i64) -> Result<usize, MemoryError> {
        let transaction = self.connection.transaction().map_err(MemoryError::Sqlite)?;
        let deleted = transaction
            .execute(
                "DELETE FROM memories WHERE expires_at_ms IS NOT NULL AND expires_at_ms <= ?1",
                [now_unix_ms],
            )
            .map_err(MemoryError::Sqlite)?;
        transaction.commit().map_err(MemoryError::Sqlite)?;
        Ok(deleted)
    }

    /// Runs `SQLite`'s quick integrity check.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError`] for a query failure or non-`ok` result.
    pub fn integrity_check(&self) -> Result<(), MemoryError> {
        let result: String = self
            .connection
            .query_row("PRAGMA quick_check", [], |row| row.get(0))
            .map_err(MemoryError::Sqlite)?;
        if result == "ok" {
            Ok(())
        } else {
            Err(MemoryError::Integrity(result))
        }
    }
}

fn remember_on(connection: &mut Connection, memory: &NewMemory) -> Result<(), MemoryError> {
    validate_memory(memory)?;
    let (scope_kind, scope_key) = memory.scope.database_parts()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(MemoryError::Sqlite)?;
    transaction
        .execute(
            "INSERT INTO memories (
                id, scope_kind, scope_key, content, created_at_ms, expires_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                scope_kind = excluded.scope_kind,
                scope_key = excluded.scope_key,
                content = excluded.content,
                created_at_ms = excluded.created_at_ms,
                expires_at_ms = excluded.expires_at_ms",
            params![
                memory.id,
                scope_kind,
                scope_key,
                memory.content,
                memory.created_at_unix_ms,
                memory.expires_at_unix_ms
            ],
        )
        .map_err(MemoryError::Sqlite)?;
    transaction.commit().map_err(MemoryError::Sqlite)
}

fn search_on(
    connection: &Connection,
    fts5_available: bool,
    scope: &MemoryScope,
    query: &str,
    now_unix_ms: i64,
    limit: u32,
) -> Result<Vec<MemoryRecord>, MemoryError> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let (scope_kind, scope_key) = scope.database_parts()?;
    if fts5_available && !query.trim().is_empty() {
        let fts_query = format!("\"{}\"", query.replace('"', "\"\""));
        let mut statement = connection
            .prepare(
                "SELECT m.id, m.scope_kind, m.scope_key, m.content,
                        m.created_at_ms, m.expires_at_ms
                 FROM memory_fts
                 JOIN memories AS m ON m.rowid = memory_fts.rowid
                 WHERE memory_fts MATCH ?1
                   AND m.scope_kind = ?2
                   AND m.scope_key = ?3
                   AND (m.expires_at_ms IS NULL OR m.expires_at_ms > ?4)
                 ORDER BY bm25(memory_fts), m.created_at_ms DESC, m.id ASC
                 LIMIT ?5",
            )
            .map_err(MemoryError::Sqlite)?;
        return collect_records(
            &mut statement,
            params![fts_query, scope_kind, scope_key, now_unix_ms, limit],
        );
    }

    let mut statement = connection
        .prepare(
            "SELECT id, scope_kind, scope_key, content, created_at_ms, expires_at_ms
             FROM memories
             WHERE scope_kind = ?1
               AND scope_key = ?2
               AND (?3 = '' OR instr(lower(content), lower(?3)) > 0)
               AND (expires_at_ms IS NULL OR expires_at_ms > ?4)
             ORDER BY created_at_ms DESC, id ASC
             LIMIT ?5",
        )
        .map_err(MemoryError::Sqlite)?;
    collect_records(
        &mut statement,
        params![scope_kind, scope_key, query, now_unix_ms, limit],
    )
}

fn forget_on(connection: &mut Connection, id: &str) -> Result<bool, MemoryError> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(MemoryError::Sqlite)?;
    let deleted = transaction
        .execute("DELETE FROM memories WHERE id = ?1", [id])
        .map_err(MemoryError::Sqlite)?;
    transaction.commit().map_err(MemoryError::Sqlite)?;
    Ok(deleted > 0)
}

fn prune_expired_on(connection: &mut Connection, now_unix_ms: i64) -> Result<usize, MemoryError> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(MemoryError::Sqlite)?;
    let deleted = transaction
        .execute(
            "DELETE FROM memories WHERE expires_at_ms IS NOT NULL AND expires_at_ms <= ?1",
            [now_unix_ms],
        )
        .map_err(MemoryError::Sqlite)?;
    transaction.commit().map_err(MemoryError::Sqlite)?;
    Ok(deleted)
}

fn integrity_check_on(connection: &Connection) -> Result<(), MemoryError> {
    let result: String = connection
        .query_row("PRAGMA quick_check", [], |row| row.get(0))
        .map_err(MemoryError::Sqlite)?;
    if result == "ok" {
        Ok(())
    } else {
        Err(MemoryError::Integrity(result))
    }
}

fn validate_memory(memory: &NewMemory) -> Result<(), MemoryError> {
    SourceId::new(format!("memory:{}", memory.id)).map_err(|_| MemoryError::InvalidMemoryId)?;
    memory.scope.database_parts()?;
    if memory
        .expires_at_unix_ms
        .is_some_and(|expiration| expiration <= memory.created_at_unix_ms)
    {
        return Err(MemoryError::InvalidExpiration);
    }
    Ok(())
}

fn migrate(connection: &mut Connection) -> Result<(), MemoryError> {
    let current: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(MemoryError::Sqlite)?;
    if current > SCHEMA_VERSION {
        return Err(MemoryError::NewerSchema(current));
    }
    if current == 0 {
        let transaction = connection.transaction().map_err(MemoryError::Sqlite)?;
        migration_v1(&transaction).map_err(MemoryError::Sqlite)?;
        transaction
            .pragma_update(None, "user_version", SCHEMA_VERSION)
            .map_err(MemoryError::Sqlite)?;
        transaction.commit().map_err(MemoryError::Sqlite)?;
    }
    Ok(())
}

fn migration_v1(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE memories (
            id TEXT PRIMARY KEY NOT NULL,
            scope_kind TEXT NOT NULL CHECK (scope_kind IN ('user', 'repository')),
            scope_key TEXT NOT NULL,
            content TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            expires_at_ms INTEGER
         );
         CREATE INDEX memories_scope_time
             ON memories(scope_kind, scope_key, created_at_ms DESC, id ASC);",
    )
}

fn detect_fts5(connection: &Connection) -> bool {
    connection
        .query_row(
            "SELECT sqlite_compileoption_used('ENABLE_FTS5')",
            [],
            |row| row.get::<_, i64>(0),
        )
        .is_ok_and(|enabled| enabled == 1)
}

fn create_fts5(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(
            content,
            content='memories',
            content_rowid='rowid'
         );
         CREATE TRIGGER IF NOT EXISTS memories_fts_insert AFTER INSERT ON memories BEGIN
            INSERT INTO memory_fts(rowid, content) VALUES (new.rowid, new.content);
         END;
         CREATE TRIGGER IF NOT EXISTS memories_fts_delete AFTER DELETE ON memories BEGIN
            INSERT INTO memory_fts(memory_fts, rowid, content)
            VALUES ('delete', old.rowid, old.content);
         END;
         CREATE TRIGGER IF NOT EXISTS memories_fts_update AFTER UPDATE ON memories BEGIN
            INSERT INTO memory_fts(memory_fts, rowid, content)
            VALUES ('delete', old.rowid, old.content);
            INSERT INTO memory_fts(rowid, content) VALUES (new.rowid, new.content);
         END;
         INSERT INTO memory_fts(memory_fts) VALUES ('rebuild');",
    )
}

fn collect_records(
    statement: &mut rusqlite::Statement<'_>,
    parameters: impl rusqlite::Params,
) -> Result<Vec<MemoryRecord>, MemoryError> {
    let rows = statement
        .query_map(parameters, |row| {
            let scope_kind: String = row.get(1)?;
            let scope_key: String = row.get(2)?;
            let scope = if scope_kind == "user" {
                MemoryScope::User
            } else {
                MemoryScope::Repository(scope_key)
            };
            Ok(MemoryRecord {
                id: row.get(0)?,
                scope,
                content: row.get(3)?,
                created_at_unix_ms: row.get(4)?,
                expires_at_unix_ms: row.get(5)?,
            })
        })
        .map_err(MemoryError::Sqlite)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(MemoryError::Sqlite)
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

/// Built-in memory provider error.
#[derive(Debug)]
pub enum MemoryError {
    /// `SQLite` operation failed.
    Sqlite(rusqlite::Error),
    /// Pooled connection creation or checkout failed.
    Pool(r2d2::Error),
    /// A connection pool must retain at least one connection.
    ZeroPoolSize,
    /// Repository scope key was empty.
    EmptyRepositoryScope,
    /// Memory identifier could not form a safe source handle.
    InvalidMemoryId,
    /// Expiration was not later than creation.
    InvalidExpiration,
    /// Database was created by a newer unsupported schema.
    NewerSchema(i64),
    /// `SQLite` integrity check returned a failure message.
    Integrity(String),
}

impl fmt::Display for MemoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(error) => write!(formatter, "SQLite memory operation failed: {error}"),
            Self::Pool(error) => write!(formatter, "SQLite memory pool failed: {error}"),
            Self::ZeroPoolSize => formatter.write_str("memory pool size must be greater than zero"),
            Self::EmptyRepositoryScope => {
                formatter.write_str("repository memory scope must not be empty")
            }
            Self::InvalidMemoryId => {
                formatter.write_str("memory ID is not safe for a source handle")
            }
            Self::InvalidExpiration => {
                formatter.write_str("memory expiration must be later than creation")
            }
            Self::NewerSchema(version) => write!(
                formatter,
                "memory database schema {version} is newer than supported schema {SCHEMA_VERSION}"
            ),
            Self::Integrity(result) => {
                write!(
                    formatter,
                    "memory database integrity check failed: {result}"
                )
            }
        }
    }
}

impl std::error::Error for MemoryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sqlite(error) => Some(error),
            Self::Pool(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory(id: &str, scope: MemoryScope, content: &str, created: i64) -> NewMemory {
        NewMemory {
            id: id.to_owned(),
            scope,
            content: content.to_owned(),
            created_at_unix_ms: created,
            expires_at_unix_ms: None,
        }
    }

    #[test]
    fn migration_persists_and_integrity_check_passes() {
        let directory = tempfile::tempdir().expect("temporary database directory");
        let path = directory.path().join("token-shrinker.db");
        {
            let mut store = MemoryStore::open(&path).expect("open memory store");
            store
                .remember(&memory("first", MemoryScope::User, "remember this", 1))
                .expect("store memory");
            store.integrity_check().expect("healthy database");
        }

        let store = MemoryStore::open(&path).expect("reopen memory store");
        let records = store
            .search(&MemoryScope::User, "remember", 2, 10)
            .expect("search memory");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, "first");
    }

    #[test]
    fn search_is_scope_isolated_and_deterministic() {
        let mut store = MemoryStore::open_in_memory().expect("open memory store");
        store
            .remember(&memory("b", MemoryScope::User, "shared needle", 5))
            .expect("store user memory");
        store
            .remember(&memory("a", MemoryScope::User, "shared needle", 5))
            .expect("store user memory");
        store
            .remember(&memory(
                "repo",
                MemoryScope::Repository("repo-1".to_owned()),
                "shared needle",
                9,
            ))
            .expect("store repository memory");

        let user = store
            .search(&MemoryScope::User, "needle", 10, 10)
            .expect("search user memories");

        assert_eq!(
            user.iter()
                .map(|record| record.id.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b"]
        );
    }

    #[test]
    fn expiration_and_transactional_forget_remove_records() {
        let mut store = MemoryStore::open_in_memory().expect("open memory store");
        let mut expiring = memory("expired", MemoryScope::User, "old", 1);
        expiring.expires_at_unix_ms = Some(5);
        store.remember(&expiring).expect("store expiring memory");
        store
            .remember(&memory("kept", MemoryScope::User, "new", 2))
            .expect("store retained memory");

        assert_eq!(store.prune_expired(5).expect("prune memory"), 1);
        assert!(store.forget("kept").expect("forget memory"));
        assert!(!store.forget("missing").expect("forget missing memory"));
        assert!(
            store
                .search(&MemoryScope::User, "", 10, 10)
                .expect("search empty memory store")
                .is_empty()
        );
    }

    #[test]
    fn memory_converts_to_addressable_context_candidate() {
        let record = MemoryRecord {
            id: "candidate".to_owned(),
            scope: MemoryScope::User,
            content: "use stable routing".to_owned(),
            created_at_unix_ms: 12,
            expires_at_unix_ms: None,
        };

        let candidate = record.into_candidate().expect("memory candidate");

        assert_eq!(candidate.source_id.as_str(), "memory:candidate");
        assert_eq!(candidate.source_kind, SourceKind::Memory);
        assert_eq!(candidate.content_hash.as_str().len(), 64);
    }

    #[test]
    fn deterministic_fallback_search_works_without_fts5() {
        let mut store = MemoryStore::open_in_memory().expect("open memory store");
        store.fts5_available = false;
        store
            .remember(&memory("fallback", MemoryScope::User, "Needle Case", 1))
            .expect("store memory");

        let records = store
            .search(&MemoryScope::User, "needle", 2, 10)
            .expect("fallback search");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, "fallback");
    }

    #[test]
    fn newer_schema_is_rejected_without_mutation() {
        let connection = Connection::open_in_memory().expect("open raw SQLite connection");
        connection
            .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
            .expect("set future schema");

        assert!(matches!(
            MemoryStore::from_connection(connection),
            Err(MemoryError::NewerSchema(version)) if version == SCHEMA_VERSION + 1
        ));
    }

    #[test]
    fn interrupted_transaction_rolls_back_without_partial_memory() {
        let mut store = MemoryStore::open_in_memory().expect("open memory store");
        {
            let transaction = store
                .connection
                .transaction()
                .expect("begin interrupted transaction");
            transaction
                .execute(
                    "INSERT INTO memories (
                        id, scope_kind, scope_key, content, created_at_ms, expires_at_ms
                     ) VALUES ('partial', 'user', '', 'must roll back', 1, NULL)",
                    [],
                )
                .expect("write uncommitted memory");
        }

        let records = store
            .search(&MemoryScope::User, "", 2, 10)
            .expect("inspect after interrupted transaction");
        assert!(records.is_empty());
        store.integrity_check().expect("database remains healthy");
    }

    #[test]
    fn pool_supports_bounded_concurrent_writes() {
        use std::{sync::Arc, thread};

        let directory = tempfile::tempdir().expect("temporary database directory");
        let path = directory.path().join("pooled.db");
        let pool = Arc::new(MemoryPool::open(&path, 2).expect("open memory pool"));
        let writers = (0..8)
            .map(|index| {
                let pool = Arc::clone(&pool);
                thread::spawn(move || {
                    pool.remember(&memory(
                        &format!("memory-{index}"),
                        MemoryScope::User,
                        "concurrent write",
                        i64::from(index),
                    ))
                })
            })
            .collect::<Vec<_>>();

        for writer in writers {
            writer
                .join()
                .expect("writer thread did not panic")
                .expect("pooled write succeeded");
        }

        let records = pool
            .search(&MemoryScope::User, "concurrent", 100, 20)
            .expect("search pooled memories");
        assert_eq!(records.len(), 8);
        pool.integrity_check().expect("pooled database healthy");
    }

    #[test]
    fn zero_sized_pool_is_rejected() {
        let directory = tempfile::tempdir().expect("temporary database directory");
        let path = directory.path().join("invalid-pool.db");
        assert!(matches!(
            MemoryPool::open(path, 0),
            Err(MemoryError::ZeroPoolSize)
        ));
    }
}
