use tracing::error;

use crate::config::Config;
use crate::{backup::backup_database_and_folders, history::BackupHistoryManager};

use anyhow::{Context, Result};

pub async fn run(config: Config) -> Result<()> {
    let history = BackupHistoryManager::new(&config.history_file_path)
        .context("creating backup history manager")?;

    let history_entry_id = history
        .start_entry()
        .context("starting backup history entry")?;

    let result = backup_database_and_folders(&config).await;
    if let Err(error) = result {
        if let Err(error) = history.finish_entry(history_entry_id, true) {
            error!(
                backup_id = ?history_entry_id,
                "Failed to mark backup history entry as failed: {error:#}"
            );
        }

        return Err(error);
    }

    if let Err(error) = history.finish_entry(history_entry_id, false) {
        error!(
            backup_id = ?history_entry_id,
            "Failed to mark backup history entry as successful: {error:#}"
        );
    }

    Ok(())
}
