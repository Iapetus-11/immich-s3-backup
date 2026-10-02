use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension};

use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BackupId(i64);

pub struct BackupHistoryManager {
    db: Connection,
}

impl BackupHistoryManager {
    pub fn new(history_file_path: &Path) -> Result<Self> {
        let db = Connection::open(history_file_path).context("opening backup history database")?;
        let self_ = Self { db };

        self_.ensure_table()?;

        Ok(self_)
    }

    fn ensure_table(&self) -> Result<()> {
        self.db
            .execute(include_str!("sql/create_history_table.sql"), ())
            .context("creating backup history table")?;
        Ok(())
    }

    pub fn start_entry(&self) -> Result<BackupId> {
        let now = Utc::now();
        let id = BackupId(now.timestamp_millis());

        self.db
            .execute(include_str!("sql/start_history_entry.sql"), (id.0, now))
            .context("creating backup history entry")?;

        Ok(id)
    }

    pub fn finish_entry(&self, id: BackupId, failed: bool) -> Result<()> {
        let now = Utc::now();
        self.db
            .execute(
                include_str!("sql/finish_history_entry.sql"),
                (id.0, failed, now),
            )
            .context("updating backup history entry")?;
        Ok(())
    }

    pub fn get_last_successful_entry(&self) -> Result<Option<DateTime<Utc>>> {
        let mut stmt = self
            .db
            .prepare(include_str!("sql/get_last_successful_history_entry.sql"))
            .context("preparing last successful backup query")?;

        stmt.query_row((), |row| row.get(0))
            .optional()
            .context("querying last successful backup")
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    use chrono::TimeZone;

    #[test]
    fn get_last_successful_entry_ignores_unfinished_backups() {
        let db = Connection::open_in_memory().unwrap();
        let history = BackupHistoryManager { db };
        history.ensure_table().unwrap();

        let completed_backup_started_at = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        history
            .db
            .execute(
                include_str!("sql/start_history_entry.sql"),
                (1, completed_backup_started_at),
            )
            .unwrap();

        let completed_backup_ended_at = Utc.with_ymd_and_hms(2026, 1, 1, 1, 0, 0).unwrap();
        history
            .db
            .execute(
                include_str!("sql/finish_history_entry.sql"),
                (1, false, completed_backup_ended_at),
            )
            .unwrap();

        let unfinished_backup_started_at = Utc.with_ymd_and_hms(2026, 2, 1, 0, 0, 0).unwrap();
        history
            .db
            .execute(
                include_str!("sql/start_history_entry.sql"),
                (2, unfinished_backup_started_at),
            )
            .unwrap();

        let last_successful = history.get_last_successful_entry().unwrap();

        assert_eq!(last_successful, Some(completed_backup_started_at));
    }
}
