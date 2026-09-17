use async_stream::try_stream;
use tracing::{error, info};

use crate::backup::history::BackupHistoryManager;

use std::{
    env,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use futures_util::{Stream, StreamExt, TryStreamExt, pin_mut};
use tokio::{
    fs::{self, File},
    process::Command,
    task::{block_in_place, spawn_blocking},
};

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
    let max_concurrent_uploads = usize::try_from(max_concurrent_uploads)
        .context("Max concurrent uploads does not fit in usize")?;
    if max_concurrent_uploads == 0 {
        bail!("Max concurrent uploads must be greater than zero");
    }

    let mut target_folder_path = upload_location.to_path_buf();
    target_folder_path.push(target_folder_name);

    let batch_iterator =
        iter_folder_batches(target_folder_path.clone(), max_batch_size_bytes).enumerate();
    pin_mut!(batch_iterator);

    // Preparing this stream packs one batch at a time. Each yielded future only
    // performs an upload, so the buffer limits uploads without parallelizing ZIP work.
    let upload_futures = try_stream! {
        while let Some((batch_idx, batch_files_result)) = batch_iterator.next().await {
            let batch_files = batch_files_result
                .with_context(|| format!("Building batch {batch_idx} for {target_folder_path:?}"))?;

            let (zip_output_name, zip_output_path) = pack_batch_zip(
                target_folder_name,
                &target_folder_path,
                batch_idx,
                batch_files,
                backup_output_folder,
            )
            .await
            .with_context(|| format!("Packing batch {batch_idx} zip"))?;

            yield upload_batch_zip(
                s3_uploader.clone(),
                target_folder_name.to_string(),
                batch_idx,
                zip_output_name,
                zip_output_path,
            );
        }
    };

    let upload_results = upload_futures.try_buffer_unordered(max_concurrent_uploads);
    pin_mut!(upload_results);

    // Drain every prepared upload so all temporary files are cleaned up. Return
    // the first failure and log any additional failures encountered afterward.
    let mut first_error = None;
    while let Some(upload_result) = upload_results.next().await {
        if let Err(upload_error) = upload_result {
            if first_error.is_none() {
                first_error = Some(upload_error);
            } else {
                error!("Additional batch failure: {upload_error:#}");
            }
        }
    }

    first_error.map_or(Ok(()), Err)
}

async fn upload_batch_zip(
    s3_uploader: S3Uploader,
    target_folder_name: String,
    batch_idx: usize,
    zip_output_name: String,
    zip_output_path: PathBuf,
) -> Result<()> {
    let upload_result = s3_uploader
        .upload_file(
            &zip_output_path,
            &format!("{target_folder_name}/{zip_output_name}"),
            Some("application/zip".to_string()),
        )
        .await
        .with_context(|| format!("Upload batch {batch_idx}"));

    let cleanup_result = remove_file_if_exists(&zip_output_path).await;

    match (upload_result, cleanup_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(upload_error), Ok(())) => Err(upload_error),
        (Ok(()), Err(cleanup_error)) => Err(cleanup_error),
        (Err(upload_error), Err(cleanup_error)) => {
            error!("Failed to clean up batch {batch_idx} after upload failure: {cleanup_error:#}");
            Err(upload_error)
        }
    }
}

fn iter_folder_batches(
    target_folder_path: PathBuf,
    max_batch_size_bytes: u64,
) -> impl Stream<Item = Result<Vec<PathBuf>>> {
    try_stream! {
        let mut batch_files: Vec<PathBuf> = vec![];
        let mut batch_size_bytes = 0_u64;

        let dir_reader = read_dir_recursive(&target_folder_path);
        pin_mut!(dir_reader);

        while let Some(file_path_result) = dir_reader.next().await {
            let file_path = file_path_result
                .with_context(|| format!("Reading file entry from {target_folder_path:?}"))?;

            let file_metadata = tokio::fs::metadata(&file_path)
                .await
                .with_context(|| format!("Reading metadata for {file_path:?}"))?;
            let file_size = file_metadata.len();

            if !batch_files.is_empty()
                && batch_size_bytes.saturating_add(file_size) > max_batch_size_bytes
            {
                yield std::mem::take(&mut batch_files);
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
) -> Result<(String, PathBuf)> {
    let zip_output_name = format!("{target_folder_name}-{batch_idx}.zip");
    let zip_output_path = backup_output_folder.join(&zip_output_name);

    let batch_zip_file_result: Result<_> = async {
        pack_zip(batch_files, &zip_output_path, false)
            .await
            .with_context(|| format!("Zipping {target_folder_path:?} (batch {batch_idx})"))?;

        fs::metadata(&zip_output_path)
            .await
            .with_context(|| format!("Getting zip metadata for batch {batch_idx}"))
    }
    .await;

    match batch_zip_file_result {
        Ok(batch_zip_file_metadata) => {
            info!(
                "Finished zipping batch {batch_idx} ({} bytes)",
                batch_zip_file_metadata.len()
            );
            Ok((zip_output_name, zip_output_path))
        }
        Err(pack_error) => {
            if let Err(cleanup_error) = remove_file_if_exists(&zip_output_path).await {
                error!("Failed to clean up incomplete zip {zip_output_path:?}: {cleanup_error:#}");
            }
            Err(pack_error)
        }
    }
}

async fn remove_file_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("Deleting file {path:?}")),
    }
}
