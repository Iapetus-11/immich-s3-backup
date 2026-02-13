use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::task::block_in_place;
use tracing::error;

#[derive(Clone, Deserialize, Serialize, PartialEq, Debug)]
pub struct BackupId(i64);

#[derive(Deserialize, Serialize)]
pub struct BackupEntry {
    pub id: BackupId,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub failed: bool,
}

pub struct BackupHistoryManager {
    file: File,
    entries: Vec<BackupEntry>,
}

impl BackupHistoryManager {
    pub fn open(path: &Path) -> Result<Self> {
        let (file, entries) = block_in_place(|| -> Result<(File, Vec<BackupEntry>)> {
            let mut file = File::options()
                .write(true)
                .read(true)
                .create(true)
                .truncate(false)
                .open(path)
                .context("Failed to open backup history file")?;
            file.lock().context("Failed to lock backup history file")?;

            let mut file_data = String::new();
            file.read_to_string(&mut file_data)?;
            let entries: Vec<BackupEntry> = serde_json::from_str(&file_data)?;

            Ok((file, entries))
        })?;

        Ok(Self { file, entries })
    }

    fn commit(&mut self) -> Result<()> {
        block_in_place(|| {
            self.file
                .write_all(serde_json::to_string_pretty(&self.entries)?.as_bytes())
                .map_err(Into::into)
        })
    }

    pub fn start_entry(&mut self) -> BackupId {
        let now = Utc::now();
        let id = BackupId(now.timestamp_millis());

        self.entries.push(BackupEntry {
            id: id.clone(),
            started_at: now,
            ended_at: None,
            failed: false,
        });

        let commit_result = self.commit();
        if let Err(error) = commit_result {
            error!("Failed to commit backup history entry {id:?}: {error:?}");
        };

        id
    }

    pub fn finish_entry(&mut self, id: BackupId, failed: bool) {
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) else {
            error!("Could not find backup history entry {id:?}");
            return;
        };

        entry.failed = failed;
        entry.ended_at = Some(Utc::now());

        let commit_result = self.commit();
        if let Err(error) = commit_result {
            error!("Failed to commit backup history entry {id:?}: {error:?}");
        };
    }

    fn close(&mut self) {
        let result = block_in_place(|| -> Result<()> {
            self.file.flush().context("Flushing backup history file")?;
            self.file
                .unlock()
                .context("Unlocking backup history file")?;
            Ok(())
        });

        if let Err(error) = result {
            error!("Failed to close backup history manager: {error:?}");
        }
    }

    pub fn entries(&self) -> &Vec<BackupEntry> {
        &self.entries
    }
}

impl Drop for BackupHistoryManager {
    fn drop(&mut self) {
        self.close();
    }
}
