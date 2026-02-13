use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use tokio::time::{self, Instant};
use tracing::error;

use crate::{
    backup::{self, history::BackupHistoryManager},
    config::CONFIG,
};

pub struct BackupScheduler {
    history: BackupHistoryManager,
    last_backup_at: Option<DateTime<Utc>>,
}

impl BackupScheduler {
    pub fn initialize() -> Result<Self> {
        let history_manager = BackupHistoryManager::open(&CONFIG.history_file)
            .context("Opening backup history manager")?;

        let last_backup_at = history_manager
            .entries()
            .iter()
            .filter_map(|e| e.ended_at)
            .max();

        Ok(Self {
            history: history_manager,
            last_backup_at,
        })
    }

    fn get_next_backup_instant(&self) -> Instant {
        todo!();
    }

    pub async fn run_forever(mut self) {
        loop {
            time::sleep_until(self.get_next_backup_instant()).await;

            let history_entry_id = self.history.start_entry();

            let result = backup::run().await;
            if let Err(error) = result {
                self.history.finish_entry(history_entry_id, true);
                error!("Failed to backup")
            } else {
                self.history.finish_entry(history_entry_id, false);
            }
        }
    }
}
