//! Local telemetry and alert history (`$XDG_DATA_HOME/sentinel/history.db`).
//!
//! - `samples`: 30-second averages (min, max, sample count) per series.
//! - `hourly`: hourly roll-ups of `samples`, built for completed hours.
//! - `alerts`: every raised, escalated and resolved alert.
//!
//! Retention is enforced by [`History::maintain`]: detail, hourly data and
//! alerts older than the configured limits are deleted, so the database
//! cannot grow without bound. Writes are batched: the daemon flushes rows
//! in one transaction every few minutes, not per sample. Process data is
//! never stored. Everything stays on this computer.

use std::fs;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use sentinel_config::Retention;
use sentinel_core::{Result, SentinelError};
use sentinel_telemetry::model::{Alert, HistoryResolution, Quality, SeriesKey, SeriesPoint};
use sentinel_telemetry::series::HistoryRow;

const MIGRATIONS: &[&str] = &[
    "
    CREATE TABLE series (
        id   INTEGER PRIMARY KEY,
        key  TEXT NOT NULL UNIQUE,
        unit TEXT NOT NULL
    );
    CREATE TABLE samples (
        series INTEGER NOT NULL,
        t      INTEGER NOT NULL,
        avg    REAL,
        min    REAL,
        max    REAL,
        n      INTEGER NOT NULL,
        PRIMARY KEY (series, t)
    ) WITHOUT ROWID;
    CREATE INDEX samples_t ON samples (t);
    CREATE TABLE hourly (
        series INTEGER NOT NULL,
        t      INTEGER NOT NULL,
        avg    REAL,
        min    REAL,
        max    REAL,
        n      INTEGER NOT NULL,
        PRIMARY KEY (series, t)
    ) WITHOUT ROWID;
    CREATE INDEX hourly_t ON hourly (t);
    CREATE TABLE alerts (
        id          INTEGER PRIMARY KEY,
        detected_at INTEGER NOT NULL,
        updated_at  INTEGER NOT NULL,
        status      TEXT NOT NULL,
        data        TEXT NOT NULL
    );
    CREATE INDEX alerts_detected ON alerts (detected_at);
    CREATE TABLE meta (
        key   TEXT PRIMARY KEY,
        value INTEGER NOT NULL
    );
",
    "
    -- Provenance of each series: where its values come from and how they
    -- were obtained (direct, calculated, estimated).
    ALTER TABLE series ADD COLUMN source TEXT NOT NULL DEFAULT '';
    ALTER TABLE series ADD COLUMN quality TEXT NOT NULL DEFAULT '';
",
];

/// What a stored series is.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SeriesInfo {
    pub unit: String,
    pub source: String,
    /// `None` for series written before provenance was recorded.
    pub quality: Option<Quality>,
}

fn quality_text(q: Quality) -> &'static str {
    match q {
        Quality::Direct => "direct",
        Quality::Calculated => "calculated",
        Quality::Estimated => "estimated",
    }
}

fn quality_of(text: &str) -> Option<Quality> {
    match text {
        "direct" => Some(Quality::Direct),
        "calculated" => Some(Quality::Calculated),
        "estimated" => Some(Quality::Estimated),
        _ => None,
    }
}

const HOUR_MS: u64 = 3_600_000;

fn err(e: rusqlite::Error) -> SentinelError {
    SentinelError::Storage(e.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Maintenance {
    pub hours_rolled_up: u64,
    pub detailed_deleted: usize,
    pub hourly_deleted: usize,
    pub alerts_deleted: usize,
}

pub struct History {
    conn: Connection,
}

impl History {
    pub fn open(path: &Path) -> Result<Self> {
        fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(path)
            .map_err(|e| SentinelError::io("creating the history database", path, e))?;
        if let Ok(meta) = fs::metadata(path)
            && meta.permissions().mode() & 0o077 != 0
        {
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))
                .map_err(|e| SentinelError::io("restricting the history database", path, e))?;
        }
        Self::prepare(Connection::open(path).map_err(err)?)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::prepare(Connection::open_in_memory().map_err(err)?)
    }

    /// Read-only access for clients while the daemon is stopped. Nothing is
    /// created or migrated; `Ok(None)` when there is no history yet.
    pub fn open_read_only(path: &Path) -> Result<Option<Self>> {
        if !path.is_file() {
            return Ok(None);
        }
        let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(err)?;
        conn.busy_timeout(std::time::Duration::from_millis(500))
            .map_err(err)?;
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(err)?;
        if version as usize != MIGRATIONS.len() {
            return Ok(None);
        }
        Ok(Some(Self { conn }))
    }

    fn prepare(mut conn: Connection) -> Result<Self> {
        conn.busy_timeout(std::time::Duration::from_secs(2))
            .map_err(err)?;
        let current: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(err)?;
        let current = current.max(0) as usize;
        if current > MIGRATIONS.len() {
            return Err(SentinelError::Storage(format!(
                "the history database has schema version {current}, newer than this Sentinel supports; it was left untouched"
            )));
        }
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(current) {
            let tx = conn.transaction().map_err(err)?;
            tx.execute_batch(sql).map_err(err)?;
            tx.pragma_update(None, "user_version", (i + 1) as i64)
                .map_err(err)?;
            tx.commit().map_err(err)?;
        }
        Ok(Self { conn })
    }

    /// The series' id, creating it or bringing its unit, source and quality
    /// up to date (the quality of a series can change, e.g. a battery whose
    /// power reading becomes calculated).
    fn series_id(conn: &Connection, row: &HistoryRow) -> rusqlite::Result<i64> {
        let quality = quality_text(row.quality);
        conn.prepare_cached(
            "INSERT INTO series (key, unit, source, quality) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(key) DO UPDATE SET unit = excluded.unit, source = excluded.source,
                 quality = excluded.quality
             WHERE unit != excluded.unit OR source != excluded.source OR quality != excluded.quality",
        )?
        .execute(params![row.key.as_str(), row.unit, row.source, quality])?;
        conn.prepare_cached("SELECT id FROM series WHERE key = ?1")?
            .query_row(params![row.key.as_str()], |r| r.get(0))
    }

    /// Write a batch of rows in one transaction. Returns rows written.
    pub fn write_rows(&mut self, rows: &[HistoryRow]) -> Result<usize> {
        if rows.is_empty() {
            return Ok(0);
        }
        let tx = self.conn.transaction().map_err(err)?;
        {
            let mut insert = tx
                .prepare_cached("INSERT OR REPLACE INTO samples (series, t, avg, min, max, n) VALUES (?1, ?2, ?3, ?4, ?5, ?6)")
                .map_err(err)?;
            for row in rows {
                let id = Self::series_id(&tx, row).map_err(err)?;
                insert
                    .execute(params![
                        id,
                        row.t as i64,
                        row.avg,
                        row.min,
                        row.max,
                        row.samples
                    ])
                    .map_err(err)?;
            }
        }
        tx.commit().map_err(err)?;
        Ok(rows.len())
    }

    pub fn upsert_alert(&mut self, alert: &Alert) -> Result<()> {
        let data =
            serde_json::to_string(alert).map_err(|e| SentinelError::Storage(e.to_string()))?;
        let status = if alert.resolved_at.is_some() {
            "resolved"
        } else {
            "active"
        };
        self.conn
            .execute(
                "INSERT INTO alerts (id, detected_at, updated_at, status, data) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(id) DO UPDATE SET updated_at = excluded.updated_at, status = excluded.status, data = excluded.data",
                params![alert.id as i64, alert.detected_at as i64, alert.last_updated_at as i64, status, data],
            )
            .map_err(err)?;
        Ok(())
    }

    /// Newest alerts first.
    pub fn alerts(&self, limit: u32) -> Result<Vec<Alert>> {
        let mut stmt = self
            .conn
            .prepare("SELECT data FROM alerts ORDER BY detected_at DESC, id DESC LIMIT ?1")
            .map_err(err)?;
        let rows = stmt
            .query_map(params![limit], |r| r.get::<_, String>(0))
            .map_err(err)?;
        Ok(rows
            .filter_map(|r| r.ok())
            .filter_map(|d| serde_json::from_str(&d).ok())
            .collect())
    }

    /// Alerts still active when the daemon last stopped.
    pub fn unresolved_alerts(&self) -> Result<Vec<Alert>> {
        let mut stmt = self
            .conn
            .prepare("SELECT data FROM alerts WHERE status = 'active' ORDER BY id")
            .map_err(err)?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0)).map_err(err)?;
        Ok(rows
            .filter_map(|r| r.ok())
            .filter_map(|d| serde_json::from_str(&d).ok())
            .collect())
    }

    pub fn max_alert_id(&self) -> Result<u64> {
        let id: Option<i64> = self
            .conn
            .query_row("SELECT MAX(id) FROM alerts", [], |r| r.get(0))
            .map_err(err)?;
        Ok(id.unwrap_or(0).max(0) as u64)
    }

    pub fn query(
        &self,
        key: &SeriesKey,
        since_ms: u64,
        resolution: HistoryResolution,
    ) -> Result<(SeriesInfo, Vec<SeriesPoint>)> {
        let table = match resolution {
            HistoryResolution::Hourly => "hourly",
            _ => "samples",
        };
        let Some((id, info)) = self
            .conn
            .query_row(
                "SELECT id, unit, source, quality FROM series WHERE key = ?1",
                params![key.as_str()],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        SeriesInfo {
                            unit: r.get(1)?,
                            source: r.get(2)?,
                            quality: quality_of(&r.get::<_, String>(3)?),
                        },
                    ))
                },
            )
            .optional()
            .map_err(err)?
        else {
            return Ok((SeriesInfo::default(), Vec::new()));
        };
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT t, avg, min, max FROM {table} WHERE series = ?1 AND t >= ?2 ORDER BY t"
            ))
            .map_err(err)?;
        let points = stmt
            .query_map(params![id, since_ms as i64], |r| {
                Ok(SeriesPoint {
                    t: r.get::<_, i64>(0)? as u64,
                    v: r.get(1)?,
                    min: r.get(2)?,
                    max: r.get(3)?,
                })
            })
            .map_err(err)?
            .filter_map(|p| p.ok())
            .collect();
        Ok((info, points))
    }

    fn meta(&self, key: &str) -> Result<Option<u64>> {
        self.conn
            .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| {
                r.get::<_, i64>(0)
            })
            .optional()
            .map(|v| v.map(|v| v.max(0) as u64))
            .map_err(err)
    }

    /// Roll completed hours into `hourly` and delete data older than the
    /// retention limits.
    pub fn maintain(&mut self, now_ms: u64, retention: &Retention) -> Result<Maintenance> {
        let mut report = Maintenance::default();
        let current_hour = now_ms - now_ms % HOUR_MS;
        let from = match self.meta("rolled_up_to")? {
            Some(t) => t,
            None => {
                let first: Option<i64> = self
                    .conn
                    .query_row("SELECT MIN(t) FROM samples", [], |r| r.get(0))
                    .map_err(err)?;
                match first {
                    Some(t) => (t.max(0) as u64) - (t.max(0) as u64) % HOUR_MS,
                    None => current_hour,
                }
            }
        };
        let tx = self.conn.transaction().map_err(err)?;
        if from < current_hour {
            // Weighted by the number of present samples; gaps (n = 0) do
            // not dilute the average.
            tx.execute(
                "INSERT OR REPLACE INTO hourly (series, t, avg, min, max, n)
                 SELECT series, (t / ?3) * ?3,
                        SUM(CASE WHEN n > 0 THEN avg * n END) / NULLIF(SUM(n), 0),
                        MIN(min), MAX(max), SUM(n)
                 FROM samples WHERE t >= ?1 AND t < ?2 GROUP BY series, t / ?3",
                params![from as i64, current_hour as i64, HOUR_MS as i64],
            )
            .map_err(err)?;
            report.hours_rolled_up = (current_hour - from) / HOUR_MS;
            tx.execute(
                "INSERT INTO meta (key, value) VALUES ('rolled_up_to', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![current_hour as i64],
            )
            .map_err(err)?;
        }
        let detail_cut = now_ms.saturating_sub(u64::from(retention.detailed_hours) * HOUR_MS);
        let hourly_cut = now_ms.saturating_sub(u64::from(retention.hourly_days) * 24 * HOUR_MS);
        let alert_cut = now_ms.saturating_sub(u64::from(retention.alert_days) * 24 * HOUR_MS);
        report.detailed_deleted = tx
            .execute(
                "DELETE FROM samples WHERE t < ?1",
                params![detail_cut as i64],
            )
            .map_err(err)?;
        report.hourly_deleted = tx
            .execute(
                "DELETE FROM hourly WHERE t < ?1",
                params![hourly_cut as i64],
            )
            .map_err(err)?;
        report.alerts_deleted = tx
            .execute(
                "DELETE FROM alerts WHERE status = 'resolved' AND updated_at < ?1",
                params![alert_cut as i64],
            )
            .map_err(err)?;
        tx.execute("DELETE FROM series WHERE id NOT IN (SELECT series FROM samples) AND id NOT IN (SELECT series FROM hourly)", [])
            .map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(report)
    }

    pub fn row_counts(&self) -> Result<(u64, u64, u64)> {
        let count = |table: &str| -> Result<u64> {
            self.conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| {
                    r.get::<_, i64>(0)
                })
                .map(|v| v as u64)
                .map_err(err)
        };
        Ok((count("samples")?, count("hourly")?, count("alerts")?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sentinel_telemetry::model::*;

    fn row(key: &str, t: u64, avg: Option<f64>, n: u32) -> HistoryRow {
        HistoryRow {
            key: SeriesKey::new(key).unwrap(),
            unit: "%",
            source: "/proc/stat",
            quality: Quality::Calculated,
            t,
            avg,
            min: avg,
            max: avg,
            samples: n,
        }
    }

    fn alert(id: u64, detected_at: u64, resolved: Option<u64>) -> Alert {
        Alert {
            id,
            rule: "cpu_high".into(),
            category: AlertCategory::Cpu,
            severity: AlertSeverity::Warning,
            title: "High CPU".into(),
            explanation: "x".into(),
            recommendation: None,
            metric: "cpu.total".into(),
            subject: None,
            threshold: "90%".into(),
            evidence: "e".into(),
            detected_at,
            last_updated_at: resolved.unwrap_or(detected_at),
            resolved_at: resolved,
            status: if resolved.is_some() {
                AlertStatus::Resolved
            } else {
                AlertStatus::Active
            },
            acknowledged: false,
            notified: true,
            threshold_source: ThresholdSource::Default,
        }
    }

    #[test]
    fn rows_round_trip_with_gaps() {
        let mut h = History::open_in_memory().unwrap();
        h.write_rows(&[
            row("cpu.total", 0, Some(10.0), 3),
            row("cpu.total", 30_000, None, 0),
            row("cpu.total", 60_000, Some(30.0), 2),
        ])
        .unwrap();
        let (info, pts) = h
            .query(
                &SeriesKey::new("cpu.total").unwrap(),
                0,
                HistoryResolution::Detailed,
            )
            .unwrap();
        assert_eq!(info.unit, "%");
        assert_eq!(info.source, "/proc/stat");
        assert_eq!(info.quality, Some(Quality::Calculated));
        assert_eq!(
            pts.iter().map(|p| p.v).collect::<Vec<_>>(),
            [Some(10.0), None, Some(30.0)]
        );
        assert!(
            h.query(
                &SeriesKey::new("nope").unwrap(),
                0,
                HistoryResolution::Detailed
            )
            .unwrap()
            .1
            .is_empty()
        );
    }

    #[test]
    fn a_version_1_database_gains_provenance_columns() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(MIGRATIONS[0]).unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
            conn.execute(
                "INSERT INTO series (key, unit) VALUES ('cpu.total', '%')",
                [],
            )
            .unwrap();
        }
        let mut h = History::open(&path).unwrap();
        let key = SeriesKey::new("cpu.total").unwrap();
        // The old series is readable, with unknown provenance...
        let (info, _) = h.query(&key, 0, HistoryResolution::Detailed).unwrap();
        assert_eq!((info.source.as_str(), info.quality), ("", None));
        // ... and gains it on the next write.
        h.write_rows(&[row("cpu.total", 0, Some(1.0), 1)]).unwrap();
        let (info, pts) = h.query(&key, 0, HistoryResolution::Detailed).unwrap();
        assert_eq!(info.quality, Some(Quality::Calculated));
        assert_eq!(pts.len(), 1);
    }

    #[test]
    fn hourly_rollup_is_weighted_and_ignores_gaps() {
        let mut h = History::open_in_memory().unwrap();
        // Hour 0: 10% x 3 samples, 40% x 1, and a gap.
        h.write_rows(&[
            row("cpu.total", 0, Some(10.0), 3),
            row("cpu.total", 30_000, Some(40.0), 1),
            row("cpu.total", 60_000, None, 0),
        ])
        .unwrap();
        // Hour 1: still in progress at "now".
        h.write_rows(&[row("cpu.total", HOUR_MS + 1_000, Some(99.0), 1)])
            .unwrap();
        let report = h.maintain(HOUR_MS + 5_000, &Retention::default()).unwrap();
        assert_eq!(report.hours_rolled_up, 1);
        let (_, pts) = h
            .query(
                &SeriesKey::new("cpu.total").unwrap(),
                0,
                HistoryResolution::Hourly,
            )
            .unwrap();
        assert_eq!(pts.len(), 1, "the current hour is not rolled up yet");
        assert_eq!(pts[0].v, Some(17.5));
        assert_eq!((pts[0].min, pts[0].max), (Some(10.0), Some(40.0)));
        // Running again does not duplicate.
        assert_eq!(
            h.maintain(HOUR_MS + 6_000, &Retention::default())
                .unwrap()
                .hours_rolled_up,
            0
        );
    }

    #[test]
    fn retention_is_enforced() {
        let mut h = History::open_in_memory().unwrap();
        let day = 24 * HOUR_MS;
        let now = 200 * day;
        h.write_rows(&[
            row("cpu.total", now - 2 * day, Some(1.0), 1),
            row("cpu.total", now - HOUR_MS, Some(2.0), 1),
        ])
        .unwrap();
        h.upsert_alert(&alert(1, now - 120 * day, Some(now - 100 * day)))
            .unwrap();
        h.upsert_alert(&alert(2, now - 120 * day, None)).unwrap();
        h.upsert_alert(&alert(3, now - day, Some(now - day)))
            .unwrap();
        let r = h
            .maintain(
                now,
                &Retention {
                    detailed_hours: 24,
                    hourly_days: 30,
                    alert_days: 90,
                },
            )
            .unwrap();
        assert_eq!(r.detailed_deleted, 1);
        assert_eq!(
            r.alerts_deleted, 1,
            "old resolved alert removed; unresolved and recent kept"
        );
        let ids: Vec<u64> = h.alerts(10).unwrap().iter().map(|a| a.id).collect();
        assert_eq!(ids, [3, 2]);
        assert_eq!(h.max_alert_id().unwrap(), 3);
        assert_eq!(h.unresolved_alerts().unwrap().len(), 1);
        // Hourly data older than the hourly retention goes too.
        let r = h.maintain(now + 40 * day, &Retention::default()).unwrap();
        assert!(r.hourly_deleted >= 1);
        assert_eq!(h.row_counts().unwrap().0, 0);
    }

    #[test]
    fn alerts_are_upserted() {
        let mut h = History::open_in_memory().unwrap();
        h.upsert_alert(&alert(5, 10, None)).unwrap();
        h.upsert_alert(&alert(5, 10, Some(20))).unwrap();
        let all = h.alerts(10).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].status, AlertStatus::Resolved);
    }

    #[test]
    fn files_are_private_and_read_only_open_creates_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.db");
        assert!(History::open_read_only(&path).unwrap().is_none());
        assert!(!path.exists());
        History::open(&path)
            .unwrap()
            .write_rows(&[row("cpu.total", 0, Some(1.0), 1)])
            .unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let ro = History::open_read_only(&path).unwrap().unwrap();
        assert_eq!(
            ro.query(
                &SeriesKey::new("cpu.total").unwrap(),
                0,
                HistoryResolution::Detailed
            )
            .unwrap()
            .1
            .len(),
            1
        );
    }
}
