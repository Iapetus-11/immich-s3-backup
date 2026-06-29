use anyhow::Result;
use tracing::error;

use crate::backup::{backup::backup_database_and_files, history::BackupHistoryManager};

pub async fn run() -> Result<()> {
    let mut history = match BackupHistoryManager::new() {
        Ok(history) => history,
        Err(error) => {
            return Err(error.context("Failed to create history manager"));
        }
    };

    let history_entry_id = match history.start_entry() {
        Ok(history_entry_id) => history_entry_id,
        Err(error) => {
            return Err(error.context("Failed to start history entry"));
        }
    };

    let result = backup_database_and_files().await;
    if let Err(error) = result {
        if let Err(error) = history.finish_entry(&history_entry_id, true) {
            error!("Failed to finish history entry: {error}");
        }

        return Err(error.context("Failed to backup"));
    } else {
        if let Err(error) = history.finish_entry(&history_entry_id, false) {
            error!("Failed to finish history entry: {error}");
        }
    }

    Ok(())
}
