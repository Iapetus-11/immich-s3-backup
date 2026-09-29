use std::time::Duration;

use anyhow::Result;
use anyhow::{Context, bail};
use chrono::Utc;
use tokio::time::{self};
use tracing::{error, info};

use crate::runner;
use crate::{config::CONFIG, history::BackupHistoryManager};

pub struct BackupScheduler;

impl BackupScheduler {
    fn get_duration_until_next_backup(&self) -> Result<Duration> {
        let mut history = BackupHistoryManager::new().context("opening backup history manager")?;

        let last_backup_at = history
            .get_last_successful_entry()
            .context("getting last successful backup")?;

        let Some(last_backup_at) = last_backup_at else {
            return Ok(Duration::ZERO);
        };

        let now = Utc::now();
        if last_backup_at > now {
            bail!("Last backup was somehow in the future");
        }

        let duration_between_backups = Duration::from_hours(24 * u64::from(CONFIG.interval_days));
        let duration_since_last_backup = (now - last_backup_at).to_std().unwrap();

        if duration_since_last_backup >= duration_between_backups {
            return Ok(Duration::ZERO);
        }

        Ok(duration_between_backups - duration_since_last_backup)
    }

    pub async fn run_forever(self) {
        loop {
            let mut duration_until_next_backup = match self.get_duration_until_next_backup() {
                Ok(duration) => duration,
                Err(error) => {
                    error!("Failed to schedule next backup: {error:#}");

                    info!("Retrying backup scheduling in 5 seconds...");
                    time::sleep(Duration::from_secs(5)).await;

                    continue;
                }
            };
            duration_until_next_backup = duration_until_next_backup.max(Duration::from_secs(5));

            info!(
                "Waiting {} seconds until next backup...",
                duration_until_next_backup.as_secs()
            );
            time::sleep(duration_until_next_backup).await;

            if let Err(error) = runner::run().await {
                error!("Backup failed: {error:#}");
            }
        }
    }
}
