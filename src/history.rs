use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::Connection;

use crate::config::CONFIG;

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BackupId(i64);

pub struct BackupHistoryManager {
    db: Connection,
}

impl BackupHistoryManager {
    pub fn new() -> Result<Self> {
        let db =
            Connection::open(&CONFIG.history_file).context("opening backup history database")?;
        let mut self_ = Self { db };

        self_.ensure_table()?;

        Ok(self_)
    }

    fn ensure_table(&mut self) -> Result<()> {
        self.db
            .execute(include_str!("sql/create_history_table.sql"), ())
            .context("creating backup history table")?;
        Ok(())
    }

    pub fn start_entry(&mut self) -> Result<BackupId> {
        let now = Utc::now();
        let id = BackupId(now.timestamp_millis());

        self.db
            .execute(include_str!("sql/start_history_entry.sql"), (id.0, now))
            .context("creating backup history entry")?;

        Ok(id)
    }

    pub fn finish_entry(&mut self, id: &BackupId, failed: bool) -> Result<()> {
        let now = Utc::now();
        self.db
            .execute(
                include_str!("sql/finish_history_entry.sql"),
                (id.0, failed, now),
            )
            .context("updating backup history entry")?;
        Ok(())
    }

    pub fn get_last_successful_entry(&mut self) -> Result<Option<DateTime<Utc>>> {
        let mut stmt = self
            .db
            .prepare(include_str!("sql/get_last_successful_history_entry.sql"))
            .context("preparing last successful backup query")?;

        let last_entry = stmt
            .query_map((), |row| row.get(0))
            .context("querying last successful backup")?
            .next();

        let Some(last_entry) = last_entry else {
            return Ok(None);
        };

        Ok(Some(
            last_entry.context("reading last successful backup timestamp")?,
        ))
    }
}
