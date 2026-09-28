use tracing::error;

use crate::{backup::backup_database_and_folders, history::BackupHistoryManager};

use anyhow::{Context, Result};

pub async fn run() -> Result<()> {
    let mut history = BackupHistoryManager::new().context("Failed to create history manager")?;

    let history_entry_id = history
        .start_entry()
        .context("Failed to start history entry")?;

    let result = backup_database_and_folders().await;
    if let Err(error) = result {
        if let Err(error) = history.finish_entry(&history_entry_id, true) {
            error!("Failed to finish history entry: {error:?}");
        }

        return Err(error.context("Failed to backup"));
    } else {
        if let Err(error) = history.finish_entry(&history_entry_id, false) {
            error!("Failed to finish history entry: {error:?}");
        }
    }

    Ok(())
}
