//! Local SQLite storage.
//!
//! It stores the daemon's own session history (when it started, when it
//! stopped, and whether it stopped cleanly), which lets the daemon notice a
//! previous unexpected termination, and the cached results of the last
//! hardware and utility scans (Phase 2).
//!
//! Extension point: later phases add tables by appending a step to
//! [`MIGRATIONS`]; `PRAGMA user_version` records how many have been applied.
//!
//! The database is application-owned data under `XDG_DATA_HOME`, created with
//! mode `0600`. A database written by a newer Sentinel is refused, never
//! downgraded or rewritten.

use std::fs;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use sentinel_core::{Result, SentinelError};
use serde::{Deserialize, Serialize};

/// Ordered schema migrations. Index `n` upgrades `user_version` n -> n+1.
const MIGRATIONS: &[&str] = &[
    "CREATE TABLE daemon_sessions (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        pid         INTEGER NOT NULL,
        app_version TEXT    NOT NULL,
        started_at  INTEGER NOT NULL,
        ended_at    INTEGER,
        end_reason  TEXT
    );",
    // Phase 2: one row per cached scan kind. The payload is a JSON document
    // whose format version is stored alongside it.
    "CREATE TABLE scan_cache (
        key            TEXT    PRIMARY KEY,
        schema_version INTEGER NOT NULL,
        discovered_at  INTEGER NOT NULL,
        saved_at       INTEGER NOT NULL,
        payload        TEXT    NOT NULL
    );",
    // Phase 6: document discovery. Payloads are JSON documents of the
    // sentinel-docs model; downloaded files live on disk, never in here.
    "CREATE TABLE doc_identities (
        identity_hash  TEXT    PRIMARY KEY,
        payload        TEXT    NOT NULL,
        first_seen     INTEGER NOT NULL,
        last_search_at INTEGER
    );
    CREATE TABLE doc_sessions (
        id            INTEGER PRIMARY KEY AUTOINCREMENT,
        identity_hash TEXT    NOT NULL,
        started_at    INTEGER NOT NULL,
        finished_at   INTEGER,
        status        TEXT    NOT NULL,
        payload       TEXT    NOT NULL
    );
    CREATE INDEX doc_sessions_identity ON doc_sessions (identity_hash, id);
    CREATE TABLE doc_candidates (
        id            INTEGER PRIMARY KEY AUTOINCREMENT,
        identity_hash TEXT    NOT NULL,
        doc_key       TEXT    NOT NULL,
        payload       TEXT    NOT NULL,
        discovered_at INTEGER NOT NULL,
        verified_at   INTEGER NOT NULL,
        local_file    TEXT,
        sha256        TEXT,
        UNIQUE (identity_hash, doc_key)
    );",
];

/// A cached scan as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheRow {
    pub schema_version: u32,
    pub discovered_at: u64,
    pub saved_at: u64,
    pub payload: String,
}

/// Sessions kept in the history table.
const SESSION_HISTORY_LIMIT: i64 = 200;

pub const END_CLEAN: &str = "clean";
pub const END_UNCLEAN: &str = "unclean";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: i64,
    pub pid: u32,
    pub app_version: String,
    pub started_at: u64,
    pub ended_at: Option<u64>,
    pub end_reason: Option<String>,
}

/// Result of opening a new daemon session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionStart {
    pub id: i64,
    /// True when the previous session never recorded its shutdown, i.e. the
    /// daemon was killed or crashed.
    pub previous_unclean: bool,
}

mod docs;
pub use docs::{DocCandidateRow, DocSessionRow};

#[derive(Debug)]
pub struct Storage {
    conn: Connection,
}

impl Storage {
    /// Open or create the database at `path` and apply pending migrations.
    pub fn open(path: &Path) -> Result<Self> {
        // Create the file ourselves so it is private from the first byte.
        fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(path)
            .map_err(|e| SentinelError::io("creating database", path, e))?;
        if let Ok(meta) = fs::metadata(path)
            && meta.permissions().mode() & 0o077 != 0
        {
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))
                .map_err(|e| SentinelError::io("restricting database", path, e))?;
        }
        let conn = Connection::open(path).map_err(storage_err)?;
        // Several connections may be open at once (the request handler and
        // a document-discovery worker): wait for a lock instead of failing.
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(storage_err)?;
        Self::prepare(conn)
    }

    /// Open an existing database for reading only: nothing is created,
    /// migrated or locked for writing. `Ok(None)` when there is no database.
    /// Used by clients to show cached scans while the daemon is stopped.
    pub fn open_read_only(path: &Path) -> Result<Option<Self>> {
        if !path.is_file() {
            return Ok(None);
        }
        let conn = Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(storage_err)?;
        conn.busy_timeout(std::time::Duration::from_millis(500))
            .map_err(storage_err)?;
        let version = schema_version(&conn)?;
        if version > MIGRATIONS.len() {
            return Err(SentinelError::Storage(format!(
                "the database has schema version {version}, newer than this Sentinel supports"
            )));
        }
        Ok(Some(Self { conn }))
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::prepare(Connection::open_in_memory().map_err(storage_err)?)
    }

    fn prepare(mut conn: Connection) -> Result<Self> {
        let current = schema_version(&conn)?;
        let latest = MIGRATIONS.len();
        if current > latest {
            return Err(SentinelError::Storage(format!(
                "the database has schema version {current}, newer than this Sentinel supports \
                 ({latest}); it has been left untouched"
            )));
        }
        for (index, sql) in MIGRATIONS.iter().enumerate().skip(current) {
            let tx = conn.transaction().map_err(storage_err)?;
            tx.execute_batch(sql).map_err(storage_err)?;
            tx.pragma_update(None, "user_version", (index + 1) as i64)
                .map_err(storage_err)?;
            tx.commit().map_err(storage_err)?;
        }
        Ok(Self { conn })
    }

    pub fn schema_version(&self) -> Result<usize> {
        schema_version(&self.conn)
    }

    /// Record that a daemon has started. Any session still marked open is a
    /// leftover from an unclean shutdown and is closed as such.
    pub fn begin_session(&mut self, pid: u32, app_version: &str, now: u64) -> Result<SessionStart> {
        let tx = self.conn.transaction().map_err(storage_err)?;
        let unclean = tx
            .execute(
                "UPDATE daemon_sessions SET ended_at = ?1, end_reason = ?2 WHERE ended_at IS NULL",
                params![now as i64, END_UNCLEAN],
            )
            .map_err(storage_err)?;
        tx.execute(
            "INSERT INTO daemon_sessions (pid, app_version, started_at) VALUES (?1, ?2, ?3)",
            params![pid, app_version, now as i64],
        )
        .map_err(storage_err)?;
        let id = tx.last_insert_rowid();
        tx.execute(
            "DELETE FROM daemon_sessions WHERE id <= ?1",
            params![id - SESSION_HISTORY_LIMIT],
        )
        .map_err(storage_err)?;
        tx.commit().map_err(storage_err)?;
        Ok(SessionStart {
            id,
            previous_unclean: unclean > 0,
        })
    }

    /// Record a clean shutdown of session `id`.
    pub fn end_session(&mut self, id: i64, now: u64) -> Result<()> {
        self.conn
            .execute(
                "UPDATE daemon_sessions SET ended_at = ?1, end_reason = ?2
                 WHERE id = ?3 AND ended_at IS NULL",
                params![now as i64, END_CLEAN, id],
            )
            .map_err(storage_err)?;
        Ok(())
    }

    /// Replace the cached scan stored under `key`.
    pub fn save_cache(
        &mut self,
        key: &str,
        schema_version: u32,
        discovered_at: u64,
        now: u64,
        payload: &str,
    ) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO scan_cache (key, schema_version, discovered_at, saved_at, payload)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(key) DO UPDATE SET schema_version = excluded.schema_version,
                    discovered_at = excluded.discovered_at, saved_at = excluded.saved_at,
                    payload = excluded.payload",
                params![
                    key,
                    schema_version,
                    discovered_at as i64,
                    now as i64,
                    payload
                ],
            )
            .map_err(storage_err)?;
        Ok(())
    }

    /// The cached scan under `key`. `Ok(None)` when nothing is cached, or
    /// when the database predates the cache table.
    pub fn load_cache(&self, key: &str) -> Result<Option<CacheRow>> {
        if self.schema_version()? < 2 {
            return Ok(None);
        }
        self.conn
            .query_row(
                "SELECT schema_version, discovered_at, saved_at, payload FROM scan_cache WHERE key = ?1",
                params![key],
                |row| {
                    Ok(CacheRow {
                        schema_version: row.get::<_, i64>(0)? as u32,
                        discovered_at: row.get::<_, i64>(1)? as u64,
                        saved_at: row.get::<_, i64>(2)? as u64,
                        payload: row.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(storage_err)
    }

    /// Most recent sessions, newest first.
    pub fn recent_sessions(&self, limit: u32) -> Result<Vec<SessionRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, pid, app_version, started_at, ended_at, end_reason
                 FROM daemon_sessions ORDER BY id DESC LIMIT ?1",
            )
            .map_err(storage_err)?;
        let rows = stmt
            .query_map(params![limit], |row| {
                Ok(SessionRecord {
                    id: row.get(0)?,
                    pid: row.get(1)?,
                    app_version: row.get(2)?,
                    started_at: row.get::<_, i64>(3)? as u64,
                    ended_at: row.get::<_, Option<i64>>(4)?.map(|v| v as u64),
                    end_reason: row.get(5)?,
                })
            })
            .map_err(storage_err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage_err)
    }
}

fn schema_version(conn: &Connection) -> Result<usize> {
    let version: Option<i64> = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .optional()
        .map_err(storage_err)?;
    Ok(version.unwrap_or(0).max(0) as usize)
}

pub(crate) fn storage_err(error: rusqlite::Error) -> SentinelError {
    SentinelError::Storage(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_database_is_migrated_to_latest() {
        let storage = Storage::open_in_memory().unwrap();
        assert_eq!(storage.schema_version().unwrap(), MIGRATIONS.len());
        assert!(storage.recent_sessions(10).unwrap().is_empty());
    }

    #[test]
    fn database_file_is_private_and_reopenable() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("sentinel.db");
        {
            let mut storage = Storage::open(&path).unwrap();
            storage.begin_session(1, "0.1.0", 100).unwrap();
        }
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let storage = Storage::open(&path).unwrap();
        assert_eq!(storage.recent_sessions(10).unwrap().len(), 1);
    }

    #[test]
    fn clean_shutdown_is_recorded() {
        let mut storage = Storage::open_in_memory().unwrap();
        let first = storage.begin_session(10, "0.1.0", 100).unwrap();
        assert!(!first.previous_unclean);
        storage.end_session(first.id, 160).unwrap();

        let second = storage.begin_session(11, "0.1.0", 200).unwrap();
        assert!(!second.previous_unclean, "a clean stop must not be flagged");

        let sessions = storage.recent_sessions(10).unwrap();
        assert_eq!(sessions[1].ended_at, Some(160));
        assert_eq!(sessions[1].end_reason.as_deref(), Some(END_CLEAN));
        assert_eq!(sessions[0].ended_at, None);
    }

    #[test]
    fn unclean_shutdown_is_detected_on_next_start() {
        let mut storage = Storage::open_in_memory().unwrap();
        storage.begin_session(10, "0.1.0", 100).unwrap();
        // No end_session: the daemon died.
        let next = storage.begin_session(11, "0.1.0", 300).unwrap();
        assert!(next.previous_unclean);

        let sessions = storage.recent_sessions(10).unwrap();
        assert_eq!(sessions[1].end_reason.as_deref(), Some(END_UNCLEAN));
        assert_eq!(sessions[1].ended_at, Some(300));
    }

    #[test]
    fn history_is_bounded() {
        let mut storage = Storage::open_in_memory().unwrap();
        for i in 0..(SESSION_HISTORY_LIMIT as u64 + 25) {
            let start = storage.begin_session(1, "0.1.0", i).unwrap();
            storage.end_session(start.id, i).unwrap();
        }
        let count = storage.recent_sessions(10_000).unwrap().len() as i64;
        assert_eq!(count, SESSION_HISTORY_LIMIT);
    }

    #[test]
    fn newer_schema_is_refused_untouched() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("sentinel.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.pragma_update(None, "user_version", 999).unwrap();
        }
        let err = Storage::open(&path).unwrap_err();
        assert!(matches!(err, SentinelError::Storage(_)));
        let conn = Connection::open(&path).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 999);
    }

    #[test]
    fn cache_round_trip_and_replace() {
        let mut storage = Storage::open_in_memory().unwrap();
        assert_eq!(storage.load_cache("hardware_inventory").unwrap(), None);
        storage
            .save_cache("hardware_inventory", 1, 100, 101, "{\"a\":1}")
            .unwrap();
        storage
            .save_cache("hardware_inventory", 1, 200, 201, "{\"a\":2}")
            .unwrap();
        let row = storage.load_cache("hardware_inventory").unwrap().unwrap();
        assert_eq!((row.discovered_at, row.saved_at), (200, 201));
        assert_eq!(row.payload, "{\"a\":2}");
    }

    #[test]
    fn read_only_open_creates_nothing_and_cannot_write() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("sentinel.db");
        assert!(Storage::open_read_only(&path).unwrap().is_none());
        assert!(!path.exists());
        Storage::open(&path)
            .unwrap()
            .save_cache("k", 1, 1, 1, "{}")
            .unwrap();
        let mut reader = Storage::open_read_only(&path).unwrap().unwrap();
        assert_eq!(reader.load_cache("k").unwrap().unwrap().payload, "{}");
        assert!(reader.save_cache("k", 1, 2, 2, "{}").is_err());
        let names: Vec<_> = fs::read_dir(tmp.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(names.len(), 1, "no journal or lock files: {names:?}");
    }

    #[test]
    fn phase_1_database_is_upgraded_in_place() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("sentinel.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(MIGRATIONS[0]).unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
            conn.execute(
                "INSERT INTO daemon_sessions (pid, app_version, started_at) VALUES (1, '0.1.0', 5)",
                [],
            )
            .unwrap();
        }
        assert_eq!(
            Storage::open_read_only(&path)
                .unwrap()
                .unwrap()
                .load_cache("k")
                .unwrap(),
            None
        );
        let storage = Storage::open(&path).unwrap();
        assert_eq!(storage.schema_version().unwrap(), MIGRATIONS.len());
        assert_eq!(
            storage.recent_sessions(5).unwrap().len(),
            1,
            "history survives"
        );
    }

    #[test]
    fn a_non_database_file_is_an_error_not_a_panic() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("sentinel.db");
        fs::write(
            &path,
            "this is definitely not a sqlite database file at all",
        )
        .unwrap();
        assert!(matches!(
            Storage::open(&path),
            Err(SentinelError::Storage(_))
        ));
        assert!(path.exists(), "the unreadable file must not be deleted");
    }
}
