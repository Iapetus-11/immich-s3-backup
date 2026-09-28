use std::fmt::Display;
use std::fs::Metadata;
use std::path::PathBuf;

use anyhow::{Context, Result};
use async_stream::try_stream;
use futures_util::Stream;
use tokio::{fs, io};

#[derive(Clone)]
pub struct FileEntry {
    pub path: PathBuf,
    pub metadata: Metadata,
}

impl Display for FileEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.path.display())
    }
}

impl FileEntry {
    pub async fn from_path(path: PathBuf) -> io::Result<Self> {
        // TODO: This is pretty inneficient because tokio::fs::metadata uses spawn_blocking and this
        // runs for every file.
        let metadata = fs::metadata(&path).await?;
        Ok(Self { path, metadata })
    }
}

pub fn read_dir_recursive(root: PathBuf) -> impl Stream<Item = Result<FileEntry>> {
    try_stream! {
        let mut directories = vec![root];
        while let Some(directory) = directories.pop() {
            let mut entries = fs::read_dir(&directory).await.with_context(|| format!("Reading files from directory {directory:?}"))?;

            while let Some(entry) = entries.next_entry().await? {
                let entry_path = entry.path();
                let entry_type = entry
                    .file_type() // On most systems this shouldn't block
                    .await
                    .with_context(|| format!("Determining file type of {entry_path:?}"))?;

                if entry_type.is_dir() {
                    directories.push(entry.path());
                    continue;
                }
                if !entry_type.is_file() {
                    continue;
                }

                yield FileEntry::from_path(entry_path).await.with_context(|| format!("Constructing file entry from {:?}", entry.path()))?;
            }
        }
    }
}
