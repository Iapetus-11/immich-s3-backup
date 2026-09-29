use crate::{
    config::Config,
    postgres::run_pg_dumpall,
    s3_client::initialize_s3_client,
    utils::{
        archives::{PackZipArgs, pack_zip},
        dir_reader::FileEntry,
    },
    utils::{dir_reader::read_dir_recursive, uploader::S3Uploader},
};

use std::{
    env,
    path::{Path, PathBuf},
    pin::pin,
};

use anyhow::{Context, Result};
use chrono::Utc;
use futures_util::{Stream, StreamExt};
use tokio::{
    fs::{self, File},
    task::JoinSet,
};

use async_stream::try_stream;
use tracing::{error, info};

pub async fn backup_database_and_folders(config: Config) -> Result<()> {
    let backup_name = Utc::now().format("%Y-%m-%d_%H-%M-%S").to_string();
    info!("Starting immich backup {backup_name}");
    let backup_folder = env::temp_dir().join("backups").join(&backup_name);

    fs::create_dir_all(&backup_folder)
        .await
        .with_context(|| format!("creating backup directory {}", backup_folder.display()))?;

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
            .context("backing up Immich database")?;

        backup_all_immich_folders(&backup_folder, &s3_uploader)
            .await
            .context("backing up Immich folders")?;

        Ok(())
    }
    .await;

    if let Err(error) = fs::remove_dir_all(&backup_folder).await {
        error!(
            path = %backup_folder.display(),
            "Failed to remove temporary backup directory: {error}"
        );
    }

    backup_result?;

    info!(backup_name, "Backup files uploaded successfully");

    Ok(())
}

async fn dump_and_backup_immich_db(backup_folder: &Path, s3_uploader: &S3Uploader) -> Result<()> {
    const DB_BACKUP_FILENAME: &str = "db-backup.sql";

    info!("Dumping Immich database");

    let dump_file_path = backup_folder.join(DB_BACKUP_FILENAME);
    let dump_file = File::create(&dump_file_path)
        .await
        .with_context(|| format!("creating dump output file {DB_BACKUP_FILENAME}"))?;

    run_pg_dumpall(dump_file).await?;

    let dump_file_entry = FileEntry::from_path(dump_file_path.clone())
        .await
        .context("getting database dump file entry")?;

    let dump_zip_name = format!("{DB_BACKUP_FILENAME}.zip");
    let dump_zip_path = dump_file_path.with_file_name(&dump_zip_name);
    let dump_zip_file = File::create(&dump_zip_path)
        .await
        .with_context(|| format!("creating zip output file {}", dump_zip_path.display()))?;

    pack_zip(PackZipArgs {
        source_dir: backup_folder.to_path_buf(),
        file_entries: vec![dump_file_entry],
        output_file: dump_zip_file,
        compress: true,
        password: &CONFIG.zip_password,
    })
    .await
    .context("zipping database dump")?;

    if let Err(error) = fs::remove_file(&dump_file_path).await {
        error!(
            path = %dump_file_path.display(),
            "Failed to remove database dump file: {error}"
        );
    }

    let dump_file_zip_entry = FileEntry::from_path(dump_zip_path.clone())
        .await
        .context("getting database dump archive file entry")?;

    s3_uploader
        .upload_file(
            &dump_file_zip_entry,
            &dump_zip_name,
            Some("application/zip".to_string()),
        )
        .await
        .context("uploading database dump")?;

    if let Err(error) = fs::remove_file(&dump_zip_path).await {
        error!(
            path = %dump_zip_path.display(),
            "Failed to remove uploaded database dump archive: {error}"
        );
    }

    Ok(())
}

async fn backup_all_immich_folders(
    backup_output_folder: &Path,
    s3_uploader: &S3Uploader,
) -> Result<()> {
    const FOLDERS_TO_BACKUP: &[&str] = &["library", "upload", "profile"];

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

    let mut batch_iterator = pin!(
        iter_folder_batches(target_folder_path.clone(), CONFIG.max_batch_size_bytes).enumerate()
    );

    let mut upload_tasks = JoinSet::<Result<()>>::new();

    let uploads_result: Result<()> = async {
        while let Some((batch_idx, batch_files_result)) = batch_iterator.next().await {
            let batch_files = batch_files_result.with_context(|| {
                format!(
                    "getting batch {batch_idx} for {}",
                    target_folder_path.display()
                )
            })?;

            if upload_tasks.len() >= CONFIG.file_upload_concurrency {
                info!(
                    "Waiting on previous upload to complete before packing {target_folder_name} batch {batch_idx}..."
                );

                if let Some(next_upload_result) = upload_tasks.join_next().await {
                    next_upload_result.context("waiting for upload task")??;
                }
            }

            let batch_zip_file_entry = create_batch_zip(
                &target_folder_path,
                target_folder_name,
                batch_idx,
                batch_files,
                backup_output_folder,
            )
            .await
            .with_context(|| format!("packing batch {batch_idx} archive"))?;

            spawn_batch_upload_task(
                s3_uploader.clone(),
                &mut upload_tasks,
                target_folder_name.to_string(),
                batch_idx,
                batch_zip_file_entry,
            );
        }

        while let Some(next_upload_result) = upload_tasks.join_next().await {
            next_upload_result.context("waiting for upload task")??;
        }

        Ok(())
    }
    .await;

    if uploads_result.is_err() {
        upload_tasks.shutdown().await;
    }

    uploads_result
}

fn spawn_batch_upload_task(
    s3_uploader: S3Uploader,
    upload_tasks: &mut JoinSet<Result<()>>,
    target_folder_name: String,
    batch_idx: usize,
    batch_zip_file_entry: FileEntry,
) {
    upload_tasks.spawn(async move {
        s3_uploader
            .upload_file(
                &batch_zip_file_entry,
                &format!("{target_folder_name}/{batch_idx}.zip"),
                Some("application/zip".to_string()),
            )
            .await
            .with_context(|| format!("uploading batch {batch_idx}"))?;

        if let Err(error) = fs::remove_file(&batch_zip_file_entry.path).await {
            error!(
                folder = target_folder_name,
                batch = batch_idx,
                path = %batch_zip_file_entry,
                "Failed to remove uploaded batch archive: {error}"
            );
        }

        Ok(())
    });
}

fn iter_folder_batches(
    target_folder_path: PathBuf,
    max_batch_size_bytes: u64,
) -> impl Stream<Item = Result<Vec<FileEntry>>> {
    try_stream! {
        let mut batch_files: Vec<FileEntry> = vec![];
        let mut batch_size_bytes = 0_u64;

        let mut dir_reader = pin!(read_dir_recursive(target_folder_path.clone()));

        while let Some(file_entry_result) = dir_reader.next().await {
            let file_entry = file_entry_result.with_context(|| {
                format!("reading file entry from {}", target_folder_path.display())
            })?;
            let file_size = file_entry.metadata.len();

            if !batch_files.is_empty()
                && batch_size_bytes.saturating_add(file_size) >= max_batch_size_bytes
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
    target_folder_name: &str,
    batch_idx: usize,
    file_entries: Vec<FileEntry>,
    backup_output_folder: &Path,
) -> Result<FileEntry> {
    info!(
        folder = target_folder_name,
        batch = batch_idx,
        files = file_entries.len(),
        "Zipping backup batch..."
    );

    let output_path = backup_output_folder.join(format!("{batch_idx}.zip"));
    let output_file = File::create(&output_path)
        .await
        .with_context(|| format!("creating zip output file {}", output_path.display()))?;

    let zip_result = pack_zip(PackZipArgs {
        source_dir: CONFIG.upload_location.clone(),
        file_entries,
        output_file,
        compress: false,
        password: &CONFIG.zip_password,
    })
    .await
    .with_context(|| {
        format!(
            "zipping {} (batch {batch_idx})",
            target_folder_path.display()
        )
    });

    if let Err(zip_error) = zip_result {
        if let Err(error) = fs::remove_file(&output_path).await {
            error!(
                batch = batch_idx,
                path = %output_path.display(),
                "Failed to remove incomplete batch archive: {error}"
            );
        }
        return Err(zip_error);
    }

    let output_file_entry = FileEntry::from_path(output_path)
        .await
        .with_context(|| format!("getting file entry for batch {batch_idx} archive"))?;

    info!(
        folder = target_folder_name,
        batch = batch_idx,
        "Finished zipping backup batch"
    );

    Ok(output_file_entry)
}
