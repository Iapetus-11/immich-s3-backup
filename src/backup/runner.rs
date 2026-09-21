use async_stream::try_stream;
use tracing::{error, info};

use crate::{backup::history::BackupHistoryManager, utils::files::FileEntry};

use std::{
    collections::HashSet,
    env,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use futures_util::{Stream, StreamExt, pin_mut};
use tokio::{
    fs::{self, File},
    process::Command,
    sync::Mutex,
    task::JoinSet,
};

use crate::{
    backup::uploader::S3Uploader,
    config::CONFIG,
    utils::{archives::pack_zip, aws_s3::initialize_s3_client, files::read_dir_recursive},
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

async fn backup_database_and_folders() -> Result<()> {
    info!("Backing up Immich...");

    let backup_name = Utc::now().format("%Y-%m-%d_%H-%M-%S").to_string();

    let backup_folder = env::temp_dir().join("backups").join(&backup_name);

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

    let dump_file_path = PathBuf::from(backup_folder).join(DB_BACKUP_FILENAME);
    let dump_file = File::create(&dump_file_path)
        .await
        .with_context(|| format!("Creating {DB_BACKUP_FILENAME}"))?;

    let pg_dumpall_result = Command::new("pg_dumpall")
        .arg("--clean")
        .arg("--if-exists")
        .arg(format!("--host={}", CONFIG.db_host)) // TODO: Parse & support port numbers
        .arg(format!("--username={}", CONFIG.db_username))
        .env("PGPASSWORD", &CONFIG.db_password)
        .stdout(dump_file.into_std().await)
        .status()
        .await
        .with_context(|| "Running pg_dumpall".to_string());

    let pg_dumpall_result = match pg_dumpall_result {
        Ok(pg_dumpall_output) => {
            if !pg_dumpall_output.success() {
                let error_code = pg_dumpall_output
                    .code()
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                bail!("Error exit code from pg_dumpall: {error_code}")
            } else {
                Ok(())
            }
        }
        Err(error) => Err(error),
    };

    if let Err(pg_dumpall_error) = pg_dumpall_result {
        let rm_dump_file_result = remove_file_if_exists(&dump_file_path).await;
        if let Err(rm_dump_file_error) = rm_dump_file_result {
            error!("Failed to clean up dump file: {rm_dump_file_error:?}");
        }

        return Err(pg_dumpall_error);
    }

    let dump_file_zip_name = format!("{DB_BACKUP_FILENAME}.zip");
    let dump_file_zip_path = dump_file_path.with_file_name(&dump_file_zip_name);

    let pack_and_upload_result: Result<()> = async {
        let dump_file_entry = FileEntry::from_path(dump_file_path.clone())
            .await
            .context("Getting file entry for database dump file")?;

        pack_zip(vec![dump_file_entry.clone()], &dump_file_zip_path, true)
            .await
            .context("Zipping database dump")?;

        s3_uploader
            .upload_file(
                &dump_file_entry,
                &dump_file_zip_name,
                Some("application/zip".to_string()),
            )
            .await
            .context("Uploading database dump")?;

        Ok(())
    }
    .await;

    let rm_dump_file_result = remove_file_if_exists(&dump_file_path).await;
    if let Err(rm_dump_file_error) = rm_dump_file_result {
        error!("Failed to clean up dump file: {rm_dump_file_error:?}");
    }

    let rm_dump_zip_file_result = remove_file_if_exists(&dump_file_zip_path).await;
    if let Err(rm_dump_zip_file_error) = rm_dump_zip_file_result {
        error!("Failed to clean up dump zip file: {rm_dump_zip_file_error:?}");
    }

    pack_and_upload_result
}

async fn backup_all_immich_folders(
    backup_output_folder: &Path,
    s3_uploader: &S3Uploader,
) -> Result<()> {
    const FOLDERS_TO_BACKUP: &[&str] = &["library", "upload", "profile"];

    info!("Zipping immich data...");

    for target_folder_name in FOLDERS_TO_BACKUP {
        backup_immich_folder(backup_output_folder, target_folder_name, s3_uploader).await?;
    }

    Ok(())
}

async fn backup_immich_folder(
    backup_output_folder: &Path,
    target_folder_name: &str,
    s3_uploader: &S3Uploader,
) -> Result<()> {
    let target_folder_path = CONFIG.upload_location.join(target_folder_name);

    let batch_iterator =
        iter_folder_batches(target_folder_path.clone(), CONFIG.max_batch_size_bytes).enumerate();
    pin_mut!(batch_iterator);

    let mut upload_tasks = JoinSet::<Result<()>>::new();
    let zip_cleanup_paths = Arc::new(Mutex::new(HashSet::<PathBuf>::new()));

    let uploads_result: Result<()> = async {
        while let Some((batch_idx, batch_files_result)) = batch_iterator.next().await {
            let batch_files = batch_files_result.with_context(|| {
                format!("Building batch {batch_idx} for {target_folder_path:?}")
            })?;

            let (zip_output_name, zip_output_path) = pack_batch_zip(
                target_folder_name,
                &target_folder_path,
                batch_idx,
                batch_files,
                backup_output_folder,
            )
            .await
            .with_context(|| format!("Packing batch {batch_idx} zip"))?;

            {
                let mut zip_cleanup_paths = zip_cleanup_paths.lock().await;
                zip_cleanup_paths.insert(zip_output_path.clone());
            }

            let zip_output_file = FileEntry::from_path(zip_output_path)
                .await
                .with_context(|| format!("Failed to get file entry for {zip_output_name}"))?;

            if upload_tasks.len() >= CONFIG.file_upload_concurrency
                && let Some(next_upload_result) = upload_tasks.join_next().await
            {
                next_upload_result.context("Waiting on next upload result")??;
            }

            {
                let s3_uploader = s3_uploader.clone();
                let target_folder_name = target_folder_name.to_string();
                let zip_cleanup_paths = zip_cleanup_paths.clone();
                upload_tasks.spawn(async move {
                    s3_uploader
                        .upload_file(
                            &zip_output_file,
                            &format!("{target_folder_name}/{zip_output_name}"),
                            Some("application/zip".to_string()),
                        )
                        .await
                        .with_context(|| format!("Upload batch {batch_idx}"))?;

                    fs::remove_file(&zip_output_file.path)
                        .await
                        .with_context(|| {
                            format!("Failed to delete uploaded batch zip {zip_output_file}")
                        })?;

                    {
                        let mut zip_cleanup_paths = zip_cleanup_paths.lock().await;
                        zip_cleanup_paths.remove(&zip_output_file.path);
                    }

                    Ok(())
                });
            }
        }

        while let Some(next_upload_result) = upload_tasks.join_next().await {
            next_upload_result.context("Waiting on next upload result")??;
        }

        Ok(())
    }
    .await;

    if uploads_result.is_err() {
        upload_tasks.shutdown().await;

        let zip_cleanup_paths = zip_cleanup_paths.lock().await;
        for zip_cleanup_path in zip_cleanup_paths.iter() {
            let cleanup_result = remove_file_if_exists(zip_cleanup_path).await;
            if let Err(cleanup_error) = cleanup_result {
                error!("Failed to cleanup zip: {cleanup_error:?}")
            }
        }
    }

    uploads_result
}

fn iter_folder_batches(
    target_folder_path: PathBuf,
    max_batch_size_bytes: u64,
) -> impl Stream<Item = Result<Vec<FileEntry>>> {
    try_stream! {
        let mut batch_files: Vec<FileEntry> = vec![];
        let mut batch_size_bytes = 0_u64;

        let dir_reader = read_dir_recursive(&target_folder_path);
        pin_mut!(dir_reader);

        while let Some(file_result) = dir_reader.next().await {
            let file = file_result
                .with_context(|| format!("Reading file entry from {target_folder_path:?}"))?;

            let file_size = file.metadata.len();

            if !batch_files.is_empty()
                && batch_size_bytes.saturating_add(file_size) > max_batch_size_bytes
            {
                yield std::mem::take(&mut batch_files);
                batch_size_bytes = 0;
            }

            batch_files.push(file);
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
    batch_files: Vec<FileEntry>,
    backup_output_folder: &Path,
) -> Result<(String, PathBuf)> {
    let zip_output_name = format!("{target_folder_name}-{batch_idx}.zip");
    let zip_output_path = backup_output_folder.join(&zip_output_name);

    let batch_zip_file_result = pack_zip(batch_files, &zip_output_path, false)
        .await
        .with_context(|| format!("Zipping {target_folder_path:?} (batch {batch_idx})"));

    match batch_zip_file_result {
        Ok(_) => {
            let batch_zip_file_size = fs::metadata(&zip_output_path)
                .await
                .map(|metadata| format!("{} bytes", metadata.len()))
                .unwrap_or_else(|_| "failed to determine size".to_string());

            info!("Finished zipping batch {batch_idx} ({batch_zip_file_size})");

            Ok((zip_output_name, zip_output_path))
        }
        Err(pack_error) => {
            if let Err(cleanup_error) = remove_file_if_exists(&zip_output_path).await {
                error!("Failed to clean up incomplete zip {zip_output_path:?}: {cleanup_error:?}");
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
