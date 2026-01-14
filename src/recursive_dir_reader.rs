use std::{
    fs::FileType,
    path::{Path, PathBuf},
};

use tokio::{
    fs::{self},
    io,
};

enum RecursiveReaderOp {
    Discover,
    Yield,
}

impl From<FileType> for RecursiveReaderOp {
    fn from(value: FileType) -> Self {
        // TODO: Should this follow sym-links?

        if value.is_dir() {
            RecursiveReaderOp::Discover
        } else {
            RecursiveReaderOp::Yield
        }
    }
}

/// Helper to make ergonomics of RecursiveDirReader::new nicer without adding an extra clone
pub enum MaybeOwnedPath<'p> {
    Owned(PathBuf),
    Borrowed(&'p Path),
}

impl<'p> MaybeOwnedPath<'p> {
    fn into_owned(self) -> PathBuf {
        match self {
            Self::Borrowed(borrowed) => borrowed.to_path_buf(),
            Self::Owned(owned) => owned,
        }
    }

    fn as_borrowed(&self) -> &Path {
        match self {
            Self::Borrowed(borrowed) => borrowed,
            Self::Owned(owned) => owned,
        }
    }
}

impl<'p> From<&'p Path> for MaybeOwnedPath<'p> {
    fn from(val: &'p Path) -> Self {
        MaybeOwnedPath::Borrowed(val)
    }
}

impl<'p> From<PathBuf> for MaybeOwnedPath<'p> {
    fn from(val: PathBuf) -> Self {
        MaybeOwnedPath::Owned(val)
    }
}

impl<'p> From<&'p PathBuf> for MaybeOwnedPath<'p> {
    fn from(val: &'p PathBuf) -> Self {
        MaybeOwnedPath::Borrowed(val)
    }
}

pub struct RecursiveDirReader<'p> {
    stack: Vec<(MaybeOwnedPath<'p>, RecursiveReaderOp)>,
}

impl<'p> RecursiveDirReader<'p> {
    pub fn new(path: impl Into<MaybeOwnedPath<'p>>) -> Self {
        Self {
            stack: vec![(path.into(), RecursiveReaderOp::Discover)],
        }
    }

    // TODO: Use AsyncIterator when it is released
    pub async fn next(&mut self) -> Option<Result<PathBuf, io::Error>> {
        let (path, instruction) = self.stack.pop()?;

        match instruction {
            RecursiveReaderOp::Discover => match fs::read_dir(path.as_borrowed()).await {
                Err(error) => Some(Err(error)),
                Ok(mut read_dir) => {
                    loop {
                        let Some(entry) = (match read_dir.next_entry().await {
                            Err(error) => return Some(Err(error)),
                            Ok(entry) => entry,
                        }) else {
                            break;
                        };

                        let entry_file_type = match entry.file_type().await {
                            Err(error) => return Some(Err(error)),
                            Ok(entry_file_type) => entry_file_type,
                        };

                        self.stack.push((
                            entry.path().into(),
                            RecursiveReaderOp::from(entry_file_type),
                        ));
                    }

                    Box::pin(self.next()).await
                }
            },
            RecursiveReaderOp::Yield => Some(Ok(path.into_owned())),
        }
    }
}
