use async_stream::try_stream;
use tracing::{error, info};

use crate::{
    postgres::run_pg_dumpall,
    utils::{
        archives::{PackZipArgs, pack_zip},
        dir_reader::FileEntry,
    },
};

use std::{
    env,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use chrono::Utc;
use futures_util::{Stream, StreamExt, pin_mut};
use tokio::{
    fs::{self, File},
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

    let dump_file_entry = FileEntry::from_path(dump_file_path.clone())
        .await
        .context("Getting file entry for database dump file")?;

    let dump_zip_name = format!("{DB_BACKUP_FILENAME}.zip");
    let dump_zip_path = dump_file_path.with_file_name(&dump_zip_name);
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

    if let Err(cleanup_error) = fs::remove_file(&dump_file_path).await {
        error!("Failed to remove database dump file {dump_file_path:?}: {cleanup_error:?}");
    }

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

    if let Err(cleanup_error) = fs::remove_file(&dump_zip_path).await {
        error!("Failed to remove uploaded database dump zip {dump_zip_path:?}: {cleanup_error:?}");
    }

    Ok(())
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

    let uploads_result: Result<()> = async {
        while let Some((batch_idx, batch_files_result)) = batch_iterator.next().await {
            let batch_files = batch_files_result
                .with_context(|| format!("Getting batch {batch_idx} for {target_folder_path:?}"))?;

            let batch_zip_file_path = create_batch_zip(
                &target_folder_path,
                batch_idx,
                batch_files,
                backup_output_folder,
            )
            .await
            .with_context(|| format!("Packing batch {batch_idx} zip"))?;
            let batch_zip_file_entry = FileEntry::from_path(batch_zip_file_path)
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

            let s3_uploader = s3_uploader.clone();
            let target_folder_name = target_folder_name.to_string();
            upload_tasks.spawn(async move {
                s3_uploader
                    .upload_file(
                        &batch_zip_file_entry,
                        &format!("{target_folder_name}/{batch_idx}.zip"),
                        Some("application/zip".to_string()),
                    )
                    .await
                    .with_context(|| format!("Upload batch {batch_idx}"))?;

                if let Err(cleanup_error) = fs::remove_file(&batch_zip_file_entry.path).await {
                    error!(
                        "Failed to delete uploaded file {batch_zip_file_entry}: {cleanup_error}"
                    );
                }

                Ok(())
            });
        }

        while let Some(next_upload_result) = upload_tasks.join_next().await {
            next_upload_result.context("Waiting on next upload result")??;
        }

        Ok(())
    }
    .await;

    if uploads_result.is_err() {
        upload_tasks.shutdown().await;
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

        let dir_reader = read_dir_recursive(target_folder_path.clone());
        pin_mut!(dir_reader);

        while let Some(file_entry_result) = dir_reader.next().await {
            let file_entry = file_entry_result
                .with_context(|| format!("Reading file entry from {target_folder_path:?}"))?;
            let file_size = file_entry.metadata.len();

            if !batch_files.is_empty()
                && batch_size_bytes.saturating_add(file_size) > max_batch_size_bytes
            {
                yield std::mem::take(&mut batch_files);
                batch_size_bytes = 0;
            }

            batch_files.push(file_entry);
            batch_size_bytes = batch_size_bytes.saturating_add(file_size);
        }

        if !batch_files.is_empty() {
            yield batch_files;
        }
    }
}

async fn create_batch_zip(
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
            info!("Finished zipping batch {batch_idx}");
            Ok(zip_output_path)
        }
        Err(pack_error) => {
            if let Err(cleanup_error) = fs::remove_file(&zip_output_path).await {
                error!("Failed to clean up incomplete zip {zip_output_path:?}: {cleanup_error:?}");
            }

            Err(pack_error)
        }
    }
}
