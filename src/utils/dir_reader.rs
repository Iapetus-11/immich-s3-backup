use std::fmt::Display;
use std::fs::Metadata;
use std::path::{Path, PathBuf};

use async_stream::stream;
use futures_util::Stream;
use tokio::{fs, io};

#[derive(Clone)]
pub struct FileEntry {
    pub path: PathBuf,
    pub metadata: Metadata,
}

impl Display for FileEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.path)
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

pub fn read_dir_recursive(path: impl AsRef<Path>) -> impl Stream<Item = io::Result<FileEntry>> {
    let root = path.as_ref().to_path_buf();

    stream! {
        let mut directories = vec![root];

        while let Some(directory) = directories.pop() {
            let mut entries = fs::read_dir(directory).await?;

            while let Some(entry) = entries.next_entry().await? {
                let entry_type = entry.file_type().await?; // On most systems this shouldn't block
                if entry_type.is_symlink() {
                    continue;
                }

                if entry_type.is_dir() {
                    directories.push(entry.path());
                    continue;
                }

                let entry = FileEntry::from_path(entry.path()).await?;
                yield Ok(entry);
            }
        }
    }
}
