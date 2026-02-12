use std::{
    fs::File,
    io::{self, Read, Write},
    path::Path,
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::task::block_in_place;

#[derive(Clone, Deserialize, Serialize, PartialEq)]
pub struct BackupId(i64);

#[derive(Deserialize, Serialize)]
pub struct BackupEntry {
    id: BackupId,
    started_at: DateTime<Utc>,
    ended_at: Option<DateTime<Utc>>,
    failed: bool,
}

pub struct BackupHistoryManager {
    file: File,
    entries: Vec<BackupEntry>,
}

impl BackupHistoryManager {
    pub fn open(path: &Path) -> Result<Self, io::Error> {
        let (file, entries) = block_in_place(|| -> Result<(File, Vec<BackupEntry>), io::Error> {
            let mut file = File::options()
                .write(true)
                .read(true)
                .create(true)
                .truncate(false)
                .open(path)?;
            file.lock()?;

            let mut file_data = String::new();
            file.read_to_string(&mut file_data)?;
            let entries: Vec<BackupEntry> = serde_json::from_str(&file_data)?;

            Ok((file, entries))
        })?;

        Ok(Self { file, entries })
    }

    pub async fn run_and_record<T, E>(
        mut self,
        f: impl AsyncFnOnce() -> Result<T, E>,
    ) -> Result<Result<T, E>, io::Error> {
        let entry_id = self.start_entry()?;

        let result = f().await;

        self.finish_entry(entry_id, result.is_err())?;
        self.close()?;

        Ok(result)
    }

    fn commit(&mut self) -> Result<(), io::Error> {
        block_in_place(|| {
            self.file
                .write_all(serde_json::to_string_pretty(&self.entries)?.as_bytes())
        })
    }

    fn start_entry(&mut self) -> Result<BackupId, io::Error> {
        let now = Utc::now();
        let id = BackupId(now.timestamp_millis());

        self.entries.push(BackupEntry {
            id: id.clone(),
            started_at: now,
            ended_at: None,
            failed: false,
        });

        self.commit()?;

        Ok(id)
    }

    fn finish_entry(&mut self, id: BackupId, failed: bool) -> Result<(), io::Error> {
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) else {
            return Ok(());
        };

        entry.failed = failed;
        entry.ended_at = Some(Utc::now());

        self.commit()?;

        Ok(())
    }

    fn close(mut self) -> Result<(), io::Error> {
        block_in_place(|| -> Result<(), io::Error> {
            self.file.flush()?;
            self.file.unlock()?;
            Ok(())
        })
    }
}
