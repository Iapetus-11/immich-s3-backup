use async_stream::try_stream;
use tracing::error;

use crate::backup::history::BackupHistoryManager;

use std::{
    env,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use futures_util::{Stream, StreamExt, pin_mut};
use tokio::{
    fs::{self, File},
    process::Command,
    task::{JoinSet, block_in_place, spawn_blocking},
};
use tracing::info;

use crate::{
    backup::uploader::S3Uploader,
    config::CONFIG,
    utils::{
        archives::pack_zip, aws_s3::initialize_s3_client, recursive_dir_reader::read_dir_recursive,
    },
};

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

    let result = backup_database_and_folders().await;
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

// TODO: Cleanup temp files on failures

async fn backup_database_and_folders() -> Result<()> {
    info!("Backing up Immich...");

    let backup_name = Utc::now().format("%Y-%m-%d_%H-%M-%S").to_string();
    let mut backup_folder = env::temp_dir();
    backup_folder.extend(["backups", &backup_name]);

    fs::create_dir_all(&backup_folder)
        .await
        .with_context(|| format!("Creating dir {}", backup_folder.to_string_lossy()))?;

    let s3_client = initialize_s3_client();
    let s3_uploader = S3Uploader::new(
        s3_client,
        CONFIG.aws_bucket_name.clone(),
        backup_name.clone(),
    );

    dump_and_backup_immich_db(&backup_folder, &s3_uploader)
        .await
        .context("Backing up immich DB")?;

    backup_all_immich_folders(&backup_folder, &s3_uploader)
        .await
        .context("Backing up immich folders")?;

    info!("Done!");

    Ok(())
}

async fn dump_and_backup_immich_db(backup_folder: &Path, s3_uploader: &S3Uploader) -> Result<()> {
    const DB_BACKUP_FILENAME: &str = "db-backup.sql";

    info!("Dumping immich database...");

    let mut dump_file_path = PathBuf::from(backup_folder);
    dump_file_path.push(DB_BACKUP_FILENAME);
    let dump_file = File::create(&dump_file_path)
        .await
        .with_context(|| format!("Creating {DB_BACKUP_FILENAME}"))?;

    let pg_dumpall_output = Command::new("pg_dumpall")
        .arg("--clean")
        .arg("--if-exists")
        .arg(format!("--host={}", CONFIG.db_host)) // TODO: Does this support port?
        .arg(format!("--username={}", CONFIG.db_username))
        .env("PGPASSWORD", &CONFIG.db_password)
        .stdout(dump_file.into_std().await)
        .status()
        .await
        .with_context(|| "Running pg_dumpall".to_string())?;

    if !pg_dumpall_output.success() {
        bail!("Failed to run pg_dumpall");
    }

    let dump_file_zip_name = format!("{DB_BACKUP_FILENAME}.zip");
    let dump_file_zip_path = dump_file_path.with_file_name(&dump_file_zip_name);

    block_in_place(|| -> Result<_> {
        pack_zip(&[&dump_file_path], &dump_file_zip_path, true).context("Zipping database dump")
    })?;

    fs::remove_file(&dump_file_path)
        .await
        .with_context(|| format!("Removing {dump_file_path:?}"))?;

    s3_uploader
        .upload_file(
            &dump_file_zip_path,
            &dump_file_zip_name,
            Some("application/zip".to_string()),
        )
        .await
        .context("Uploading database dump")?;

    fs::remove_file(&dump_file_zip_path)
        .await
        .with_context(|| format!("Removing {dump_file_zip_path:?}"))?;

    Ok(())
}

async fn backup_all_immich_folders(
    backup_output_folder: &Path,
    s3_uploader: &S3Uploader,
) -> Result<()> {
    const FOLDERS_TO_BACKUP: &[&str] = &["library", "upload", "profile"];

    info!("Zipping immich data...");

    for target_folder_name in FOLDERS_TO_BACKUP {
        backup_immich_folder(
            backup_output_folder,
            &CONFIG.upload_location,
            target_folder_name,
            CONFIG.max_batch_size_bytes,
            CONFIG.max_concurrent_uploads,
            s3_uploader,
        )
        .await?;
    }

    Ok(())
}

async fn backup_immich_folder(
    backup_output_folder: &Path,
    upload_location: &Path,
    target_folder_name: &str,
    max_batch_size_bytes: u64,
    max_concurrent_uploads: u64,
    s3_uploader: &S3Uploader,
) -> Result<()> {
    let mut upload_tasks = JoinSet::new();

    let mut target_folder_path = upload_location.to_path_buf();
    target_folder_path.push(target_folder_name);

    let batch_iterator = iter_folder_batches(&target_folder_path, max_batch_size_bytes).enumerate();
    pin_mut!(batch_iterator);

    while let Some((batch_idx, batch_files_result)) = batch_iterator.next().await {
        let batch_files = batch_files_result?; // TODO: Better error handling

        if upload_tasks.len() as u64 >= max_concurrent_uploads {
            info!("Max concurrent uploads reached, waiting for a previous upload to finish...");
            upload_tasks
                .join_next()
                .await
                .unwrap() // TODO: Better error handling
                .context("Upload batch background task")??;
        }

        let (zip_output_name, zip_output_path) = pack_batch_zip(
            target_folder_name,
            &target_folder_path,
            batch_idx,
            batch_files,
            backup_output_folder,
        )
        .await
        .with_context(|| format!("Packing batch {batch_idx} zip"))?;

        upload_tasks.spawn({
            let s3_uploader = s3_uploader.clone();
            let target_folder_name = target_folder_name.to_string();

            async move {
                s3_uploader
                    .upload_file(
                        &zip_output_path,
                        &format!("{target_folder_name}/{zip_output_name}"),
                        Some("application/zip".to_string()),
                    )
                    .await
                    .with_context(|| format!("Upload batch {batch_idx}"))?;

                fs::remove_file(&zip_output_path)
                    .await
                    .with_context(|| format!("Deleting {zip_output_path:?}"))?;

                Ok(()) as Result<()>
            }
        });
    }

    while let Some(next_task) = upload_tasks.join_next().await {
        next_task.context("Upload batch background task")??;
    }

    Ok(())
}

fn iter_folder_batches(
    target_folder_path: &Path,
    max_batch_size_bytes: u64,
) -> impl Stream<Item = anyhow::Result<Vec<PathBuf>>> {
    try_stream! {
        let mut batch_files: Vec<PathBuf> = vec![];
        let mut batch_size_bytes = 0_u64;

        let dir_reader = read_dir_recursive(target_folder_path);
        pin_mut!(dir_reader);

        while let Some(file_path_result) = dir_reader.next().await {
            let file_path = file_path_result
                .with_context(|| format!("Reading file entry from {target_folder_path:?}"))?;

            let file_metadata = tokio::fs::metadata(&file_path)
                .await
                .with_context(|| format!("Reading metadata for {file_path:?}"))?;
            let file_size = file_metadata.len();

            if batch_size_bytes + file_size > max_batch_size_bytes {
                yield batch_files;
                batch_files = vec![];
                batch_size_bytes = 0;
            }

            batch_files.push(file_path);
            batch_size_bytes += file_size;
        }

        if !batch_files.is_empty() {
            yield batch_files;
        }
    }
}

async fn pack_batch_zip(
    target_folder_name: &str,
    target_folder_path: &Path,
    batch_idx: usize,
    batch_files: Vec<PathBuf>,
    backup_output_folder: &Path,
) -> anyhow::Result<(String, PathBuf)> {
    let zip_output_name = format!("{target_folder_name}-{batch_idx}.zip");
    let zip_output_path = {
        let mut path = backup_output_folder.to_path_buf();
        path.push(&zip_output_name);
        path
    };

    {
        let batch_files = batch_files.clone();
        let zip_output_path = zip_output_path.clone();
        let target_folder_path = target_folder_path.to_path_buf();
        spawn_blocking(move || {
            pack_zip(
                &batch_files
                    .iter()
                    .map(std::path::PathBuf::as_path)
                    .collect::<Vec<_>>(),
                &zip_output_path,
                false,
            )
            .with_context(|| format!("Zipping {target_folder_path:?} (batch {batch_idx})"))
        })
        .await
    }
    .with_context(|| format!("Executing zip of {target_folder_path:?} (batch {batch_idx})"))??;

    let batch_zip_size_bytes = zip_output_path
        .metadata()
        .with_context(|| format!("Getting zip metadata for batch {batch_idx}"))?
        .len();
    info!("Finished zipping batch {batch_idx} ({batch_zip_size_bytes} bytes)");

    Ok((zip_output_name, zip_output_path))
}
