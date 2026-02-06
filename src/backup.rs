use std::{
    env,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use tokio::{
    fs::{self, File},
    process::Command,
    task::{JoinSet, block_in_place},
};
use tracing::info;

use crate::{
    archive_utils::pack_zip, config::CONFIG, recursive_dir_reader::RecursiveDirReader, uploader::S3Uploader
};

pub const DB_BACKUP_FILENAME: &str = "db-backup.sql";

/// Creates a dump of the immich database and zips it, returning the path to that zip file
/// Reference <https://docs.immich.app/administration/backup-and-restore/#database>
async fn backup_immich_db(backup_folder: &Path, s3_uploader: &S3Uploader) -> Result<()> {
    info!("Dumping immich database...");

    let mut dump_file_path = PathBuf::from(backup_folder);
    dump_file_path.push(DB_BACKUP_FILENAME);
    let dump_file = File::create(&dump_file_path)
        .await
        .with_context(|| format!("Creating {DB_BACKUP_FILENAME}"))?;

    let pg_dumpall_output = Command::new("pg_dumpall")
        .arg("--clean")
        .arg("--if-exists")
        .arg(format!("--host={}", CONFIG.db_host))
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
    pack_zip(&[&dump_file_path], &dump_file_zip_path).context("Zipping database dump")?;

    fs::remove_file(&dump_file_path)
        .await
        .with_context(|| format!("Removing {dump_file_path:?}"))?;

    s3_uploader
        .upload_file(&dump_file_zip_path, &dump_file_zip_name)
        .await
        .context("Uploading database dump")?;

    fs::remove_file(&dump_file_zip_path)
        .await
        .with_context(|| format!("Removing {dump_file_zip_path:?}"))?;

    Ok(())
}

async fn backup_immich_folder(
    backup_output_folder: &Path,
    upload_location: &Path,
    target_folder_name: &str,
    max_batch_size_bytes: u64,
    max_concurrent_uploads: usize,
    s3_uploader: &S3Uploader,
) -> Result<()> {
    let mut join_set = JoinSet::new();

    let mut target_folder_path = upload_location.to_path_buf();
    target_folder_path.push(target_folder_name);

    let mut batch_idx = 0_usize;
    let mut batch_files: Vec<PathBuf> = vec![];
    let mut batch_size_bytes = 0_u64;

    let mut dir_reader = RecursiveDirReader::new(&target_folder_path);
    let mut has_more_files = true;
    while has_more_files {
        let file_path = dir_reader.next().await;
        has_more_files = file_path.is_some();

        if let Some(file_path) = file_path {
            let file_path = file_path
                .with_context(|| format!("Reading file entry from {target_folder_path:?}"))?;
            let file_metadata = file_path
                .metadata()
                .with_context(|| format!("Reading metadata for {file_path:?}"))?;

            batch_size_bytes += file_metadata.len();
            batch_files.push(file_path);
        }

        if !has_more_files || batch_size_bytes >= max_batch_size_bytes {
            batch_idx += 1;
            info!(
                "Zipping up {target_folder_path:?} batch {batch_idx} ({} files)...",
                batch_files.len()
            );

            let zip_output_name = format!("{target_folder_name}-{batch_idx}.zip");
            let mut zip_output_path = backup_output_folder.to_path_buf();
            zip_output_path.push(&zip_output_name);

            block_in_place(|| {
                pack_zip(
                    &batch_files.iter().map(std::path::PathBuf::as_path).collect::<Vec<_>>(),
                    &zip_output_path,
                )
            })
            .with_context(|| format!("Zipping {target_folder_path:?} (batch {batch_idx})"))?;

            let batch_zip_size_bytes = zip_output_path
                .metadata()
                .with_context(|| format!("Getting zip metadata for batch {batch_idx}"))?
                .len();
            info!("Finished zipping batch {batch_idx} ({batch_zip_size_bytes} bytes)");

            if join_set.len() >= max_concurrent_uploads {
                info!("Max concurrent uploads reached, waiting for a previous upload to finish...");
                join_set
                    .join_next()
                    .await
                    .unwrap()
                    .context("Upload batch background task")??;
            }

            join_set.spawn({
                let s3_uploader = s3_uploader.clone();
                let target_folder_name = target_folder_name.to_string();

                async move {
                    s3_uploader
                        .upload_file(
                            &zip_output_path,
                            &format!("{target_folder_name}/{zip_output_name}"),
                        )
                        .await
                        .with_context(|| format!("Upload batch {batch_idx}"))?;

                    fs::remove_file(&zip_output_path)
                        .await
                        .with_context(|| format!("Deleting {zip_output_path:?}"))?;

                    Ok(()) as Result<()>
                }
            });

            batch_files.clear();
            batch_size_bytes = 0;
        }
    }

    while let Some(next_task) = join_set.join_next().await {
        next_task.context("Upload batch background task")??;
    }

    Ok(())
}

/// Zips up immich files (containing media library, profile pictures, and more)
/// Reference <https://docs.immich.app/administration/backup-and-restore/#filesystem>
async fn backup_immich_files(backup_output_folder: &Path, s3_uploader: &S3Uploader) -> Result<()> {
    info!("Zipping immich data...");

    const FOLDERS_TO_BACKUP: &[&str] = &["library", "upload", "profile"];

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

pub async fn run() -> Result<()> {
    info!("Backing up Immich...");

    let backup_name = Utc::now().format("%Y-%m-%d_%H-%M-%S").to_string();
    let mut backup_folder = env::temp_dir();
    backup_folder.extend(["backups", &backup_name]);

    fs::create_dir_all(&backup_folder)
        .await
        .with_context(|| format!("Creating dir {}", backup_folder.to_string_lossy()))?;

    let s3_uploader = S3Uploader::new_from_env(backup_name.clone()).await?;

    backup_immich_db(&backup_folder, &s3_uploader)
        .await
        .context("Backing up immich DB")?;

    backup_immich_files(&backup_folder, &s3_uploader)
        .await
        .context("Backing up immich files")?;

    info!("Done!");

    Ok(())
}
