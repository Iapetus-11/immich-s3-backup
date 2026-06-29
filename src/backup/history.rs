use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::config::CONFIG;

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq, Debug)]
pub struct BackupId(i64);

#[derive(Deserialize, Serialize)]
pub struct BackupEntry {
    pub id: BackupId,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub failed: bool,
}

pub struct BackupHistoryManager {
    db: Connection,
}

impl BackupHistoryManager {
    pub fn new() -> Result<Self> {
        let db = Connection::open(&CONFIG.history_file).context("Opening Sqlite DB")?;
        let mut self_ = Self { db };

        self_.ensure_table()?;

        Ok(self_)
    }

    fn ensure_table(&mut self) -> Result<()> {
        self.db
            .execute(include_str!("../sql/create_history_table.sql"), ())
            .context("Creating history table")?;
        Ok(())
    }

    pub fn start_entry(&mut self) -> Result<BackupId> {
        let now = Utc::now();
        let id = BackupId(now.timestamp_millis());

        self.db
            .execute(include_str!("../sql/start_history_entry.sql"), (id.0, now))
            .context("Creating history record")?;

        Ok(id)
    }

    pub fn finish_entry(&mut self, id: &BackupId, failed: bool) -> Result<()> {
        let now = Utc::now();
        self.db
            .execute(
                include_str!("../sql/finish_history_entry.sql"),
                (id.0, failed, now),
            )
            .context("Updating history record")?;
        Ok(())
    }

    pub fn get_last_entry(&mut self) -> Result<Option<BackupEntry>> {
        let mut stmt = self
            .db
            .prepare(include_str!("../sql/get_last_history_entry.sql"))?;

        let last_entry = stmt
            .query_map((), |row| {
                Ok(BackupEntry {
                    id: BackupId(row.get(0)?),
                    started_at: row.get(1)?,
                    ended_at: row.get(2)?,
                    failed: row.get(3)?,
                })
            })?
            .next();

        let Some(last_entry) = last_entry else {
            return Ok(None);
        };

        Ok(Some(last_entry?))
    }
}
