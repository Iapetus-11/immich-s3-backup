use std::time::Duration;

use anyhow::Result;
use anyhow::{Context, bail};
use chrono::{DateTime, Utc};
use tokio::time::{self};
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

    fn get_duration_until_next_backup(&self) -> Result<Duration> {
        let Some(last_backup_at) = self.last_backup_at else {
            return Ok(Duration::ZERO);
        };

        let now = Utc::now();
        if last_backup_at > now {
            bail!("Last backup was somehow in the future");
        }

        let duration_between_backups = Duration::from_hours(24 * u64::from(CONFIG.interval_days));
        let duration_since_last_backup = (now - last_backup_at).to_std().unwrap();

        if duration_since_last_backup > duration_between_backups {
            return Ok(Duration::ZERO);
        }

        Ok(duration_between_backups - duration_since_last_backup)
    }

    pub async fn run_forever(mut self) {
        loop {
            let duration_until_next_backup = match self.get_duration_until_next_backup() {
                Ok(duration) => duration,
                Err(error) => {
                    error!("Failed to get duration until next backup: {error:?}");
                    continue;
                }
            };
            time::sleep(duration_until_next_backup).await;

            let history_entry_id = self.history.start_entry();

            let result = backup::run().await;
            if let Err(error) = result {
                self.history.finish_entry(&history_entry_id, true);
                error!("Failed to backup: {error:?}");
            } else {
                self.history.finish_entry(&history_entry_id, false);
            }
        }
    }
}
