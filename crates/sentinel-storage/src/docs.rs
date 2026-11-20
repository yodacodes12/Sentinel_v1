//! Phase 6 document-discovery records (schema migration 3).
//!
//! The storage layer keeps typed columns for what it needs to query
//! (identity, keys, timestamps, stored file) and the full record as JSON
//! written by `sentinel-docs`. Downloaded documents are files on disk; only
//! their name and hash are stored here.

use rusqlite::{OptionalExtension, params};
use sentinel_core::Result;

use crate::{Storage, storage_err};

/// Sessions kept per identity.
const SESSION_LIMIT: i64 = 50;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocCandidateRow {
    pub id: u64,
    pub identity_hash: String,
    pub payload: String,
    pub local_file: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocSessionRow {
    pub id: u64,
    pub payload: String,
}

impl Storage {
    fn docs_ready(&self) -> Result<bool> {
        Ok(self.schema_version()? >= 3)
    }

    /// Record (or refresh) a search identity.
    pub fn save_doc_identity(&mut self, hash: &str, payload: &str, now: u64) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO doc_identities (identity_hash, payload, first_seen) VALUES (?1, ?2, ?3)
                 ON CONFLICT(identity_hash) DO UPDATE SET payload = excluded.payload",
                params![hash, payload, now as i64],
            )
            .map_err(storage_err)?;
        Ok(())
    }

    pub fn doc_last_search(&self, hash: &str) -> Result<Option<u64>> {
        if !self.docs_ready()? {
            return Ok(None);
        }
        self.conn
            .query_row(
                "SELECT last_search_at FROM doc_identities WHERE identity_hash = ?1",
                params![hash],
                |r| r.get::<_, Option<i64>>(0),
            )
            .optional()
            .map(|v| v.flatten().map(|t| t as u64))
            .map_err(storage_err)
    }

    pub fn set_doc_last_search(&mut self, hash: &str, at: Option<u64>) -> Result<()> {
        self.conn
            .execute(
                "UPDATE doc_identities SET last_search_at = ?2 WHERE identity_hash = ?1",
                params![hash, at.map(|t| t as i64)],
            )
            .map_err(storage_err)?;
        Ok(())
    }

    pub fn begin_doc_session(&mut self, hash: &str, started_at: u64, payload: &str) -> Result<u64> {
        self.conn
            .execute(
                "INSERT INTO doc_sessions (identity_hash, started_at, status, payload) VALUES (?1, ?2, 'running', ?3)",
                params![hash, started_at as i64, payload],
            )
            .map_err(storage_err)?;
        let id = self.conn.last_insert_rowid() as u64;
        // Bounded history per identity.
        self.conn
            .execute(
                "DELETE FROM doc_sessions WHERE identity_hash = ?1 AND id NOT IN
                   (SELECT id FROM doc_sessions WHERE identity_hash = ?1 ORDER BY id DESC LIMIT ?2)",
                params![hash, SESSION_LIMIT],
            )
            .map_err(storage_err)?;
        Ok(id)
    }

    pub fn finish_doc_session(
        &mut self,
        id: u64,
        finished_at: u64,
        status: &str,
        payload: &str,
    ) -> Result<()> {
        self.conn
            .execute(
                "UPDATE doc_sessions SET finished_at = ?2, status = ?3, payload = ?4 WHERE id = ?1",
                params![id as i64, finished_at as i64, status, payload],
            )
            .map_err(storage_err)?;
        Ok(())
    }

    /// Sessions left `running` by a daemon that stopped: returns their IDs
    /// so the caller can rewrite their payloads, and marks them interrupted.
    pub fn interrupted_doc_sessions(&mut self, now: u64) -> Result<Vec<DocSessionRow>> {
        if !self.docs_ready()? {
            return Ok(Vec::new());
        }
        let rows = {
            let mut stmt = self
                .conn
                .prepare("SELECT id, payload FROM doc_sessions WHERE status = 'running'")
                .map_err(storage_err)?;
            stmt.query_map([], |r| {
                Ok(DocSessionRow {
                    id: r.get::<_, i64>(0)? as u64,
                    payload: r.get(1)?,
                })
            })
            .map_err(storage_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(storage_err)?
        };
        self.conn
            .execute("UPDATE doc_sessions SET status = 'interrupted', finished_at = ?1 WHERE status = 'running'", params![now as i64])
            .map_err(storage_err)?;
        Ok(rows)
    }

    pub fn recent_doc_sessions(&self, hash: &str, limit: u32) -> Result<Vec<DocSessionRow>> {
        if !self.docs_ready()? {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare("SELECT id, payload FROM doc_sessions WHERE identity_hash = ?1 ORDER BY id DESC LIMIT ?2").map_err(storage_err)?;
        stmt.query_map(params![hash, limit], |r| {
            Ok(DocSessionRow {
                id: r.get::<_, i64>(0)? as u64,
                payload: r.get(1)?,
            })
        })
        .map_err(storage_err)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(storage_err)
    }

    /// Insert a candidate or update the existing one with the same key. The
    /// first discovery time and any stored file are kept. Returns the row ID.
    pub fn upsert_doc_candidate(
        &mut self,
        hash: &str,
        key: &str,
        payload: &str,
        discovered_at: u64,
        verified_at: u64,
    ) -> Result<u64> {
        self.conn
            .execute(
                "INSERT INTO doc_candidates (identity_hash, doc_key, payload, discovered_at, verified_at) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(identity_hash, doc_key) DO UPDATE SET payload = excluded.payload, verified_at = excluded.verified_at",
                params![hash, key, payload, discovered_at as i64, verified_at as i64],
            )
            .map_err(storage_err)?;
        self.conn
            .query_row(
                "SELECT id FROM doc_candidates WHERE identity_hash = ?1 AND doc_key = ?2",
                params![hash, key],
                |r| r.get::<_, i64>(0),
            )
            .map(|id| id as u64)
            .map_err(storage_err)
    }

    /// First discovery time of a candidate, if it was seen before.
    pub fn doc_candidate_discovered_at(&self, hash: &str, key: &str) -> Result<Option<u64>> {
        if !self.docs_ready()? {
            return Ok(None);
        }
        self.conn
            .query_row("SELECT discovered_at FROM doc_candidates WHERE identity_hash = ?1 AND doc_key = ?2", params![hash, key], |r| r.get::<_, i64>(0))
            .optional()
            .map(|v| v.map(|t| t as u64))
            .map_err(storage_err)
    }

    pub fn doc_candidates(&self, hash: &str) -> Result<Vec<DocCandidateRow>> {
        if !self.docs_ready()? {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare("SELECT id, identity_hash, payload, local_file FROM doc_candidates WHERE identity_hash = ?1 ORDER BY id").map_err(storage_err)?;
        stmt.query_map(params![hash], row)
            .map_err(storage_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(storage_err)
    }

    pub fn doc_candidate(&self, id: u64) -> Result<Option<DocCandidateRow>> {
        if !self.docs_ready()? {
            return Ok(None);
        }
        self.conn
            .query_row(
                "SELECT id, identity_hash, payload, local_file FROM doc_candidates WHERE id = ?1",
                params![id as i64],
                row,
            )
            .optional()
            .map_err(storage_err)
    }

    /// Record (or clear) the stored file of a candidate, with its new payload.
    pub fn set_doc_candidate_file(
        &mut self,
        id: u64,
        file: Option<&str>,
        sha256: Option<&str>,
        payload: &str,
    ) -> Result<()> {
        self.conn
            .execute("UPDATE doc_candidates SET local_file = ?2, sha256 = ?3, payload = ?4 WHERE id = ?1", params![id as i64, file, sha256, payload])
            .map_err(storage_err)?;
        Ok(())
    }

    /// How many candidates still reference a stored file.
    pub fn doc_file_references(&self, file: &str) -> Result<u64> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM doc_candidates WHERE local_file = ?1",
                params![file],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n as u64)
            .map_err(storage_err)
    }

    /// Forget search results and sessions of an identity. Candidates with a
    /// downloaded file are kept (remove the file first to forget them).
    pub fn clear_doc_cache(&mut self, hash: &str) -> Result<u64> {
        let removed = self
            .conn
            .execute(
                "DELETE FROM doc_candidates WHERE identity_hash = ?1 AND local_file IS NULL",
                params![hash],
            )
            .map_err(storage_err)?;
        self.conn
            .execute(
                "DELETE FROM doc_sessions WHERE identity_hash = ?1 AND status != 'running'",
                params![hash],
            )
            .map_err(storage_err)?;
        self.conn
            .execute(
                "UPDATE doc_identities SET last_search_at = NULL WHERE identity_hash = ?1",
                params![hash],
            )
            .map_err(storage_err)?;
        Ok(removed as u64)
    }
}

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<DocCandidateRow> {
    Ok(DocCandidateRow {
        id: r.get::<_, i64>(0)? as u64,
        identity_hash: r.get(1)?,
        payload: r.get(2)?,
        local_file: r.get(3)?,
    })
}

#[cfg(test)]
mod tests {
    use crate::{MIGRATIONS, Storage};

    #[test]
    fn candidates_are_deduplicated_and_keep_their_first_discovery() {
        let mut s = Storage::open_in_memory().unwrap();
        s.save_doc_identity("h", "{}", 10).unwrap();
        let a = s
            .upsert_doc_candidate("h", "https://dl.dell.com/x.pdf", "{\"v\":1}", 100, 100)
            .unwrap();
        let b = s
            .upsert_doc_candidate("h", "https://dl.dell.com/x.pdf", "{\"v\":2}", 200, 200)
            .unwrap();
        assert_eq!(a, b);
        assert_eq!(
            s.doc_candidate_discovered_at("h", "https://dl.dell.com/x.pdf")
                .unwrap(),
            Some(100)
        );
        let rows = s.doc_candidates("h").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].payload, "{\"v\":2}",
            "verification results are updated"
        );
        // Another identity has its own candidates.
        s.upsert_doc_candidate("other", "https://dl.dell.com/x.pdf", "{}", 1, 1)
            .unwrap();
        assert_eq!(s.doc_candidates("h").unwrap().len(), 1);
    }

    #[test]
    fn cache_expiry_and_clearing_keep_downloaded_files() {
        let mut s = Storage::open_in_memory().unwrap();
        s.save_doc_identity("h", "{}", 10).unwrap();
        assert_eq!(s.doc_last_search("h").unwrap(), None);
        s.set_doc_last_search("h", Some(500)).unwrap();
        assert_eq!(s.doc_last_search("h").unwrap(), Some(500));
        let kept = s.upsert_doc_candidate("h", "k1", "{}", 1, 1).unwrap();
        s.upsert_doc_candidate("h", "k2", "{}", 1, 1).unwrap();
        s.set_doc_candidate_file(kept, Some("ab.pdf"), Some("ab"), "{\"file\":1}")
            .unwrap();
        assert_eq!(s.doc_file_references("ab.pdf").unwrap(), 1);
        assert_eq!(s.clear_doc_cache("h").unwrap(), 1);
        let left = s.doc_candidates("h").unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].local_file.as_deref(), Some("ab.pdf"));
        assert_eq!(
            s.doc_last_search("h").unwrap(),
            None,
            "clearing forces a new search"
        );
    }

    #[test]
    fn interrupted_sessions_are_marked_after_a_restart() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("sentinel.db");
        {
            let mut s = Storage::open(&path).unwrap();
            s.begin_doc_session("h", 5, "{\"running\":true}").unwrap();
            let done = s.begin_doc_session("h", 6, "{}").unwrap();
            s.finish_doc_session(done, 7, "completed", "{\"done\":true}")
                .unwrap();
        }
        let mut s = Storage::open(&path).unwrap();
        let interrupted = s.interrupted_doc_sessions(9).unwrap();
        assert_eq!(interrupted.len(), 1);
        assert!(s.interrupted_doc_sessions(10).unwrap().is_empty());
        assert_eq!(s.recent_doc_sessions("h", 10).unwrap().len(), 2);
    }

    #[test]
    fn session_history_is_bounded() {
        let mut s = Storage::open_in_memory().unwrap();
        for i in 0..60 {
            s.begin_doc_session("h", i, "{}").unwrap();
        }
        assert_eq!(s.recent_doc_sessions("h", 100).unwrap().len(), 50);
    }

    #[test]
    fn a_phase_5_database_migrates_without_losing_data() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("sentinel.db");
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(MIGRATIONS[0]).unwrap();
            conn.execute_batch(MIGRATIONS[1]).unwrap();
            conn.pragma_update(None, "user_version", 2).unwrap();
            conn.execute("INSERT INTO scan_cache (key, schema_version, discovered_at, saved_at, payload) VALUES ('hardware', 2, 1, 1, '{}')", []).unwrap();
        }
        // Read-only clients of an un-migrated database see no documents.
        let ro = Storage::open_read_only(&path).unwrap().unwrap();
        assert!(ro.doc_candidates("h").unwrap().is_empty());
        drop(ro);
        let mut s = Storage::open(&path).unwrap();
        assert_eq!(s.schema_version().unwrap(), 3);
        assert!(
            s.load_cache("hardware").unwrap().is_some(),
            "the scan cache survives"
        );
        s.save_doc_identity("h", "{}", 1).unwrap();
        assert_eq!(s.upsert_doc_candidate("h", "k", "{}", 1, 1).unwrap(), 1);
    }
}
