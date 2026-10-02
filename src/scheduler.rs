use std::time::Duration;

use anyhow::{Context, Result, bail};
use chrono::Utc;
use tokio::time;
use tracing::{error, info};

use crate::config::Config;
use crate::history::BackupHistoryManager;
use crate::runner;

pub struct BackupScheduler {
    config: Config,
}

impl BackupScheduler {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub async fn run_forever(self) {
        const MAX_SLEEP: Duration = Duration::from_mins(15);

        loop {
            let duration_until_next_backup = match self.get_duration_until_next_backup() {
                Ok(duration) => duration,
                Err(error) => {
                    error!("Failed to schedule next backup: {error:#}");

                    info!("Retrying backup scheduling in 5 seconds...");
                    time::sleep(Duration::from_secs(5)).await;

                    continue;
                }
            };

            info!(
                "Waiting {} seconds until next backup...",
                duration_until_next_backup.as_secs()
            );
            if duration_until_next_backup > MAX_SLEEP {
                time::sleep(MAX_SLEEP).await;
                continue;
            }
            time::sleep(duration_until_next_backup).await;

            if let Err(error) = runner::run(self.config.clone()).await {
                error!("Backup failed: {error:#}");
            }
        }
    }

    fn get_duration_until_next_backup(&self) -> Result<Duration> {
        let history = BackupHistoryManager::new(&self.config.history_file_path)
            .context("opening backup history manager")?;

        let last_backup_at = history
            .get_last_successful_entry()
            .context("getting last successful backup")?;

        let Some(last_backup_at) = last_backup_at else {
            return Ok(Duration::from_secs(5));
        };

        let now = Utc::now();
        if last_backup_at > now {
            bail!("Last backup was somehow in the future");
        }

        let duration_between_backups =
            Duration::from_hours(24 * u64::from(self.config.interval_days));
        let duration_since_last_backup = (now - last_backup_at)
            .to_std()
            .context("calculating time since last successful backup")?;

        Ok(duration_between_backups
            .saturating_sub(duration_since_last_backup)
            .max(Duration::from_secs(5)))
    }
}
