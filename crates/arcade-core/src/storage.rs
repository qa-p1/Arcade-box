use arcade_contract::{ResultStatus, ToolResult};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::Mutex,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub tool_id: String,
    pub status: String,
    pub created_at: String,
}

/// Persisted job metadata intentionally excludes requests, source paths,
/// output values, and error text. Job results remain in memory only.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredJob {
    pub id: String,
    pub tool_id: String,
    pub status: String,
    pub progress: Option<f64>,
    pub created_at: String,
    pub updated_at: String,
}

/// All persisted state lives behind this service. Input content and source paths
/// are intentionally absent from the history schema.
pub struct Storage {
    connection: Mutex<Connection>,
}

#[derive(Default)]
pub struct RankingSignals {
    pub favorites: HashSet<String>,
    pub uses: HashMap<String, u64>,
}

const MIGRATIONS: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS favorites (tool_id TEXT PRIMARY KEY);\
     CREATE TABLE IF NOT EXISTS usage (tool_id TEXT PRIMARY KEY, uses INTEGER NOT NULL DEFAULT 0, last_used TEXT NOT NULL);\
     CREATE TABLE IF NOT EXISTS history (id INTEGER PRIMARY KEY, tool_id TEXT NOT NULL, status TEXT NOT NULL, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);\
     CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);\
     CREATE TABLE IF NOT EXISTS custom_aliases (alias TEXT PRIMARY KEY, tool_id TEXT NOT NULL);\
     CREATE TABLE IF NOT EXISTS pipelines (id TEXT PRIMARY KEY, version INTEGER NOT NULL, definition_json TEXT NOT NULL, updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);\
     CREATE TABLE IF NOT EXISTS jobs (id TEXT PRIMARY KEY, tool_id TEXT NOT NULL, status TEXT NOT NULL, progress REAL, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);\
     CREATE TABLE IF NOT EXISTS provider_choices (capability TEXT PRIMARY KEY, executable_path TEXT NOT NULL);",
    "CREATE INDEX IF NOT EXISTS jobs_updated_at_idx ON jobs(updated_at DESC, created_at DESC);",
];

impl Storage {
    pub fn open(path: &Path) -> Result<Self, rusqlite::Error> {
        let connection = Connection::open(path)?;
        Self::initialize(connection)
    }

    pub fn in_memory() -> Result<Self, rusqlite::Error> {
        Self::initialize(Connection::open_in_memory()?)
    }

    fn initialize(mut connection: Connection) -> Result<Self, rusqlite::Error> {
        connection.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY);")?;
        for (index, migration) in MIGRATIONS.iter().enumerate() {
            let version = index as i64 + 1;
            let applied: Option<i64> = connection
                .query_row(
                    "SELECT version FROM schema_migrations WHERE version = ?1",
                    [version],
                    |r| r.get(0),
                )
                .optional()?;
            if applied.is_none() {
                let transaction = connection.transaction()?;
                transaction.execute_batch(migration)?;
                transaction.execute(
                    "INSERT INTO schema_migrations (version) VALUES (?1)",
                    [version],
                )?;
                transaction.commit()?;
            }
        }
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn ranking_signals(&self) -> Result<RankingSignals, rusqlite::Error> {
        let connection = self.connection.lock().expect("storage mutex poisoned");
        let mut signals = RankingSignals::default();
        let mut favorites = connection.prepare("SELECT tool_id FROM favorites")?;
        for id in favorites.query_map([], |r| r.get::<_, String>(0))? {
            signals.favorites.insert(id?);
        }
        let mut usage = connection.prepare("SELECT tool_id, uses FROM usage")?;
        for pair in usage.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
            let (id, count) = pair?;
            signals.uses.insert(id, count.max(0) as u64);
        }
        Ok(signals)
    }

    pub fn set_favorite(&self, tool_id: &str, favorite: bool) -> Result<(), rusqlite::Error> {
        let connection = self.connection.lock().expect("storage mutex poisoned");
        if favorite {
            connection.execute(
                "INSERT OR IGNORE INTO favorites (tool_id) VALUES (?1)",
                [tool_id],
            )?;
        } else {
            connection.execute("DELETE FROM favorites WHERE tool_id = ?1", [tool_id])?;
        }
        Ok(())
    }

    pub fn record_usage(&self, tool_id: &str, result: &ToolResult) -> Result<(), rusqlite::Error> {
        // Sensitive tools should not create a detailed history trail.
        if tool_id.starts_with("arcade.security.") {
            return Ok(());
        }
        let connection = self.connection.lock().expect("storage mutex poisoned");
        connection.execute(
            "INSERT INTO usage (tool_id, uses, last_used) VALUES (?1, 1, CURRENT_TIMESTAMP) \
             ON CONFLICT(tool_id) DO UPDATE SET uses = uses + 1, last_used = CURRENT_TIMESTAMP",
            [tool_id],
        )?;
        let status = match result.status {
            ResultStatus::Success => "success",
            ResultStatus::Error => "error",
        };
        connection.execute(
            "INSERT INTO history (tool_id, status) VALUES (?1, ?2)",
            params![tool_id, status],
        )?;
        Ok(())
    }

    pub fn history_count(&self) -> Result<u64, rusqlite::Error> {
        let connection = self.connection.lock().expect("storage mutex poisoned");
        connection.query_row("SELECT COUNT(*) FROM history", [], |r| {
            r.get::<_, i64>(0).map(|count| count.max(0) as u64)
        })
    }

    pub fn recent_history(&self, limit: usize) -> Result<Vec<HistoryEntry>, rusqlite::Error> {
        let connection = self.connection.lock().expect("storage mutex poisoned");
        let mut statement = connection
            .prepare("SELECT tool_id, status, created_at FROM history ORDER BY id DESC LIMIT ?1")?;
        statement
            .query_map([limit.min(100) as i64], |row| {
                Ok(HistoryEntry {
                    tool_id: row.get(0)?,
                    status: row.get(1)?,
                    created_at: row.get(2)?,
                })
            })?
            .collect()
    }

    pub fn favorites(&self) -> Result<Vec<String>, rusqlite::Error> {
        let connection = self.connection.lock().expect("storage mutex poisoned");
        let mut statement = connection.prepare("SELECT tool_id FROM favorites ORDER BY tool_id")?;
        statement.query_map([], |row| row.get(0))?.collect()
    }

    pub fn set_alias(&self, alias: &str, tool_id: &str) -> Result<(), rusqlite::Error> {
        let connection = self.connection.lock().expect("storage mutex poisoned");
        connection.execute("INSERT INTO custom_aliases (alias, tool_id) VALUES (?1, ?2) ON CONFLICT(alias) DO UPDATE SET tool_id = excluded.tool_id", params![alias.trim().to_lowercase(), tool_id])?;
        Ok(())
    }

    pub fn resolve_alias(&self, alias: &str) -> Result<Option<String>, rusqlite::Error> {
        let connection = self.connection.lock().expect("storage mutex poisoned");
        connection
            .query_row(
                "SELECT tool_id FROM custom_aliases WHERE alias = ?1",
                [alias.trim().to_lowercase()],
                |row| row.get(0),
            )
            .optional()
    }

    pub fn list_pipeline_definitions(&self) -> Result<Vec<(String, u32, String)>, rusqlite::Error> {
        let connection = self.connection.lock().expect("storage mutex poisoned");
        let mut statement = connection.prepare(
            "SELECT id, version, definition_json FROM pipelines ORDER BY id COLLATE NOCASE",
        )?;
        statement
            .query_map([], |row| {
                Ok((row.get(0)?, row.get::<_, i64>(1)? as u32, row.get(2)?))
            })?
            .collect()
    }

    pub fn pipeline_definition(&self, id: &str) -> Result<Option<(u32, String)>, rusqlite::Error> {
        let connection = self.connection.lock().expect("storage mutex poisoned");
        connection
            .query_row(
                "SELECT version, definition_json FROM pipelines WHERE id = ?1",
                [id],
                |row| Ok((row.get::<_, i64>(0)? as u32, row.get(1)?)),
            )
            .optional()
    }

    pub fn save_pipeline_definition(
        &self,
        id: &str,
        version: u32,
        definition_json: &str,
    ) -> Result<(), rusqlite::Error> {
        let connection = self.connection.lock().expect("storage mutex poisoned");
        connection.execute(
            "INSERT INTO pipelines (id, version, definition_json) VALUES (?1, ?2, ?3) \
             ON CONFLICT(id) DO UPDATE SET version = excluded.version, definition_json = excluded.definition_json, updated_at = CURRENT_TIMESTAMP",
            params![id, version, definition_json],
        )?;
        Ok(())
    }

    pub fn delete_pipeline_definition(&self, id: &str) -> Result<(), rusqlite::Error> {
        let connection = self.connection.lock().expect("storage mutex poisoned");
        connection.execute("DELETE FROM pipelines WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>, rusqlite::Error> {
        let connection = self.connection.lock().expect("storage mutex poisoned");
        connection
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), rusqlite::Error> {
        let connection = self.connection.lock().expect("storage mutex poisoned");
        connection.execute("INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value", params![key, value])?;
        Ok(())
    }

    pub fn create_job(&self, id: &str, tool_id: &str) -> Result<StoredJob, rusqlite::Error> {
        let connection = self.connection.lock().expect("storage mutex poisoned");
        connection.execute(
            "INSERT INTO jobs (id, tool_id, status) VALUES (?1, ?2, 'queued')",
            params![id, tool_id],
        )?;
        read_job(&connection, id)
    }

    pub fn update_job(
        &self,
        id: &str,
        status: &str,
        progress: Option<f64>,
    ) -> Result<StoredJob, rusqlite::Error> {
        const STATUSES: &[&str] = &[
            "queued",
            "running",
            "cancelling",
            "succeeded",
            "failed",
            "cancelled",
            "interrupted",
        ];
        if !STATUSES.contains(&status) {
            return Err(rusqlite::Error::InvalidParameterName(
                "unknown job status".into(),
            ));
        }
        if progress.is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value)) {
            return Err(rusqlite::Error::InvalidParameterName(
                "job progress must be between zero and one".into(),
            ));
        }

        let connection = self.connection.lock().expect("storage mutex poisoned");
        let changed = connection.execute(
            "UPDATE jobs SET status = ?2, progress = ?3, updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
            params![id, status, progress],
        )?;
        if changed == 0 {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
        read_job(&connection, id)
    }

    pub fn recent_jobs(&self, limit: usize) -> Result<Vec<StoredJob>, rusqlite::Error> {
        let connection = self.connection.lock().expect("storage mutex poisoned");
        let mut statement = connection.prepare(
            "SELECT id, tool_id, status, progress, created_at, updated_at FROM jobs \
             ORDER BY updated_at DESC, created_at DESC, id DESC LIMIT ?1",
        )?;
        statement
            .query_map([limit.min(1000) as i64], job_from_row)?
            .collect()
    }

    /// Mark active records as interrupted after a process restart. No request
    /// data is needed to explain that the work did not complete.
    pub fn recover_interrupted_jobs(&self) -> Result<usize, rusqlite::Error> {
        let connection = self.connection.lock().expect("storage mutex poisoned");
        connection.execute(
            "UPDATE jobs SET status = 'interrupted', updated_at = CURRENT_TIMESTAMP \
             WHERE status IN ('queued', 'running', 'cancelling')",
            [],
        )
    }

    /// Keep a bounded history of terminal job metadata while never pruning
    /// queued or running jobs.
    pub fn prune_terminal_jobs(&self, retain: usize) -> Result<usize, rusqlite::Error> {
        let connection = self.connection.lock().expect("storage mutex poisoned");
        connection.execute(
            "DELETE FROM jobs WHERE id IN (\
               SELECT id FROM jobs WHERE status IN ('succeeded', 'failed', 'cancelled', 'interrupted') \
               ORDER BY updated_at DESC, created_at DESC, id DESC LIMIT -1 OFFSET ?1\
             )",
            [retain.min(1000) as i64],
        )
    }
}

fn read_job(connection: &Connection, id: &str) -> Result<StoredJob, rusqlite::Error> {
    connection.query_row(
        "SELECT id, tool_id, status, progress, created_at, updated_at FROM jobs WHERE id = ?1",
        [id],
        job_from_row,
    )
}

fn job_from_row(row: &rusqlite::Row<'_>) -> Result<StoredJob, rusqlite::Error> {
    Ok(StoredJob {
        id: row.get(0)?,
        tool_id: row.get(1)?,
        status: row.get(2)?,
        progress: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn migration_and_favorites_survive_queries() {
        let db = Storage::in_memory().unwrap();
        db.set_favorite("arcade.text.case", true).unwrap();
        assert!(
            db.ranking_signals()
                .unwrap()
                .favorites
                .contains("arcade.text.case")
        );
        db.set_favorite("arcade.text.case", false).unwrap();
        assert!(
            !db.ranking_signals()
                .unwrap()
                .favorites
                .contains("arcade.text.case")
        );
    }

    #[test]
    fn job_storage_keeps_only_safe_metadata_and_validates_progress() {
        let db = Storage::in_memory().unwrap();
        let created = db.create_job("job-1", "arcade.video.compress").unwrap();
        assert_eq!(created.status, "queued");
        assert_eq!(created.progress, None);

        let updated = db.update_job("job-1", "running", Some(0.42)).unwrap();
        assert_eq!(updated.progress, Some(0.42));
        assert_eq!(updated.status, "running");
        assert!(db.update_job("job-1", "running", Some(f64::NAN)).is_err());
        assert!(db.update_job("job-1", "secret-content", None).is_err());

        let rows = db.recent_jobs(10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].tool_id, "arcade.video.compress");
        // The persisted row type has no request, path, output, or message field.
        let json = serde_json::to_value(&rows[0]).unwrap();
        assert!(json.get("request").is_none());
        assert!(json.get("result").is_none());
        assert!(json.get("message").is_none());
    }

    #[test]
    fn recovery_marks_only_active_jobs_interrupted() {
        let db = Storage::in_memory().unwrap();
        db.create_job("queued", "arcade.video.compress").unwrap();
        db.create_job("running", "arcade.image.resize").unwrap();
        db.update_job("running", "running", Some(0.5)).unwrap();
        db.create_job("done", "arcade.text.case").unwrap();
        db.update_job("done", "succeeded", Some(1.0)).unwrap();

        assert_eq!(db.recover_interrupted_jobs().unwrap(), 2);
        let jobs = db.recent_jobs(10).unwrap();
        let statuses: HashMap<_, _> = jobs.into_iter().map(|job| (job.id, job.status)).collect();
        assert_eq!(
            statuses.get("queued").map(String::as_str),
            Some("interrupted")
        );
        assert_eq!(
            statuses.get("running").map(String::as_str),
            Some("interrupted")
        );
        assert_eq!(statuses.get("done").map(String::as_str), Some("succeeded"));
    }

    #[test]
    fn pruning_retains_active_jobs_and_latest_terminal_metadata() {
        let db = Storage::in_memory().unwrap();
        db.create_job("active", "arcade.video.compress").unwrap();
        for id in ["done-a", "done-b", "done-c"] {
            db.create_job(id, "arcade.text.case").unwrap();
            db.update_job(id, "succeeded", Some(1.0)).unwrap();
        }
        db.prune_terminal_jobs(1).unwrap();
        let jobs = db.recent_jobs(10).unwrap();
        assert_eq!(
            jobs.iter().filter(|job| job.status == "succeeded").count(),
            1
        );
        assert!(jobs.iter().any(|job| job.id == "active"));
    }
}
