use async_stream::try_stream;
use tracing::{error, info};

use crate::{
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

pub async fn backup_database_and_folders() -> Result<()> {
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
        CONFIG.multipart_chunk_size,
        CONFIG.file_chunk_upload_concurrency,
    );

    let backup_result: Result<()> = async {
        dump_and_backup_immich_db(&backup_folder, &s3_uploader)
            .await
            .context("Backing up immich DB")?;

        backup_all_immich_folders(&backup_folder, &s3_uploader)
            .await
            .context("Backing up immich folders")?;

        Ok(())
    }
    .await;

    if let Err(error) = fs::remove_dir_all(&backup_folder).await {
        error!("Failed to remove backup folder {backup_folder:?}: {error:?}");
    }

    backup_result?;

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

    run_pg_dumpall(dump_file).await?;

    let dump_zip_name = format!("{DB_BACKUP_FILENAME}.zip");
    let dump_zip_path = dump_file_path.with_file_name(&dump_zip_name);

    let pack_and_upload_result: Result<()> = async {
        let dump_file_entry = FileEntry::from_path(dump_file_path.clone())
            .await
            .context("Getting file entry for database dump file")?;

        let dump_zip_file = File::create(&dump_zip_path)
            .await
            .with_context(|| format!("Creating zip output file {dump_zip_path:?}"))?;

        pack_zip(PackZipArgs {
            source_dir: backup_folder.to_path_buf(),
            file_entries: vec![dump_file_entry],
            output_file: dump_zip_file,
            compress: true,
            password: &CONFIG.zip_password,
        })
        .await
        .context("Zipping database dump")?;

        let dump_file_zip_entry = FileEntry::from_path(dump_zip_path.clone())
            .await
            .context("Getting file entry for database dump zip")?;

        s3_uploader
            .upload_file(
                &dump_file_zip_entry,
                &dump_zip_name,
                Some("application/zip".to_string()),
            )
            .await
            .context("Uploading database dump")?;

        Ok(())
    }
    .await;

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
    let abandoned_zip_files = Arc::new(Mutex::new(HashSet::<PathBuf>::new()));

    let uploads_result: Result<()> = async {
        while let Some((batch_idx, batch_files_result)) = batch_iterator.next().await {
            let batch_files = batch_files_result
                .with_context(|| format!("Getting batch {batch_idx} for {target_folder_path:?}"))?;

            let zip_output_path = pack_batch_zip(
                &target_folder_path,
                batch_idx,
                batch_files,
                backup_output_folder,
            )
            .await
            .with_context(|| format!("Packing batch {batch_idx} zip"))?;

            {
                let mut zip_cleanup_paths = abandoned_zip_files.lock().await;
                zip_cleanup_paths.insert(zip_output_path.clone());
            }

            let zip_output_file = FileEntry::from_path(zip_output_path)
                .await
                .with_context(|| format!("Failed to get file entry for batch {batch_idx} zip"))?;

            if upload_tasks.len() >= CONFIG.file_upload_concurrency {
                info!(
                    "Waiting on previous upload to complete before uploading batch {batch_idx}..."
                );

                if let Some(next_upload_result) = upload_tasks.join_next().await {
                    next_upload_result.context("Waiting on next upload result")??;
                }
            }

            {
                let s3_uploader = s3_uploader.clone();
                let target_folder_name = target_folder_name.to_string();
                let abandoned_zip_files = abandoned_zip_files.clone();
                upload_tasks.spawn(async move {
                    s3_uploader
                        .upload_file(
                            &zip_output_file,
                            &format!("{target_folder_name}/{batch_idx}.zip"),
                            Some("application/zip".to_string()),
                        )
                        .await
                        .with_context(|| format!("Upload batch {batch_idx}"))?;

                    fs::remove_file(&zip_output_file.path)
                        .await
                        .with_context(|| {
                            format!("Deleting uploaded batch zip {zip_output_file}")
                        })?;

                    {
                        let mut zip_cleanup_paths = abandoned_zip_files.lock().await;
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

        let zip_cleanup_paths = abandoned_zip_files.lock().await;
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
    target_folder_path: &Path,
    batch_idx: usize,
    batch_files: Vec<FileEntry>,
    backup_output_folder: &Path,
) -> Result<PathBuf> {
    let zip_output_path = backup_output_folder.join(format!("{batch_idx}.zip"));
    let zip_output_file = File::create(&zip_output_path)
        .await
        .with_context(|| format!("Creating zip output file {zip_output_path:?}"))?;

    let batch_zip_file_result = pack_zip(PackZipArgs {
        source_dir: CONFIG.upload_location.clone(),
        file_entries: batch_files,
        output_file: zip_output_file,
        compress: false,
        password: &CONFIG.zip_password,
    })
    .await
    .with_context(|| format!("Zipping {target_folder_path:?} (batch {batch_idx})"));

    match batch_zip_file_result {
        Ok(_) => {
            let batch_zip_file_size = fs::metadata(&zip_output_path)
                .await
                .map(|metadata| format!("{} bytes", metadata.len()))
                .unwrap_or_else(|_| "failed to determine size".to_string());

            info!("Finished zipping batch {batch_idx} ({batch_zip_file_size})");

            Ok(zip_output_path)
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
