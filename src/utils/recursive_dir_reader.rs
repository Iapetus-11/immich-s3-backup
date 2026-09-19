use std::path::{Path, PathBuf};

use async_stream::stream;
use futures_util::Stream;
use tokio::{fs, io};

pub fn read_dir_recursive(
    path: impl AsRef<Path>,
) -> impl Stream<Item = Result<PathBuf, io::Error>> {
    let root = path.as_ref().to_path_buf();

    stream! {
        let mut directories = vec![root];

        while let Some(directory) = directories.pop() {
            let mut entries = fs::read_dir(directory).await?;

            while let Some(entry) = entries.next_entry().await? {
                let file_type = entry.file_type().await?;

                if file_type.is_dir() {
                    directories.push(entry.path());
                } else {
                    // Symlinks are not followed
                    yield Ok(entry.path());
                }
            }
        }
    }
}
