use async_stream::try_stream;
use tracing::{error, info};

use crate::{
    backup::backup_database_and_folders,
    history::BackupHistoryManager,
    postgres::run_pg_dumpall,
    utils::{
        archives::{PackZipArgs, pack_zip},
        dir_reader::FileEntry,
    },
};

use std::{
    collections::HashSet,
    env,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result};
use chrono::Utc;
use futures_util::{Stream, StreamExt, pin_mut};
use tokio::{
    fs::{self, File},
    sync::Mutex,
    task::JoinSet,
};

use crate::{
    config::CONFIG,
    s3_client::initialize_s3_client,
    utils::{dir_reader::read_dir_recursive, uploader::S3Uploader},
};

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
