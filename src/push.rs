use std::{env, path::Path};

use anyhow::{Context, Result};
use aws_sdk_s3::primitives::ByteStream;
use chrono::Utc;
use tokio::{fs, process::Command};

use crate::{archive_utils::pack_zip, consts::IMMICH_BACKUP_DIR};

/// Runs immich-go archive to pull data from Immich, then zips the files
async fn pull_and_zip(immich_go_command: &str) -> Result<Vec<String>> {
    let server_url = env::var("IMMICH_SERVER_URL").context("Missing env IMMICH_SERVER_URL")?;
    let api_key = env::var("IMMICH_API_KEY").context("Missing env IMMICH_API_KEY")?;

    println!(
        "Backing up assets from {}...",
        server_url
            .strip_prefix("https://")
            .unwrap_or(&server_url)
            .strip_prefix("http://")
            .unwrap_or(&server_url)
            .trim_matches('/')
    );

    // Command::new(immich_go_command)
    //     .arg("archive")
    //     .arg("from-immich")
    //     .arg(format!("--from-server={server_url}"))
    //     .arg(format!("--from-api-key={api_key}"))
    //     .arg(format!("--write-to-folder={IMMICH_BACKUP_DIR}"))
    //     .status()
    //     .await
    //     .with_context(|| format!("Failed executing: {immich_go_command} archive from-immich"))?;


    let mut backup_zips: Vec<String> = vec![];

    // TODO: Paralellize this
    let mut dir_reader = fs::read_dir(IMMICH_BACKUP_DIR)
        .await
        .with_context(|| format!("Reading dir {IMMICH_BACKUP_DIR}"))?;
    while let Some(entry) = dir_reader
        .next_entry()
        .await
        .with_context(|| format!("Reading dir {IMMICH_BACKUP_DIR}"))?
    {
        let entry_name = entry.file_name().to_string_lossy().to_string();
        let entry_path = format!("{IMMICH_BACKUP_DIR}/{entry_name}");
        let entry_type = entry
            .file_type()
            .await
            .with_context(|| format!("Determining file type of {entry_path}"))?;

        if !entry_type.is_dir() {
            continue;
        }

        println!("Compressing backup {entry_path}...");

        pack_zip(&entry_path)
            .await
            .with_context(|| format!("Zipping {entry_path}"))?;
        backup_zips.push(format!("{entry_path}.zip"));
    }

    // TODO: Delete folders

    Ok(backup_zips)
}

async fn push_to_s3(backup_zips: &[String]) -> Result<()> {
    let aws_sdk_config = aws_config::load_from_env().await;
    let s3_client = aws_sdk_s3::Client::new(&aws_sdk_config);

    let bucket_name = env::var("AWS_BUCKET_NAME").context("Missing env AWS_BUCKET_NAME")?;

    let current_time_str = Utc::now().format("%Y-%m-%d_%H-%M-%S").to_string();

    // TODO: Can we paralellize this?
    for zipped_backup_path in backup_zips {
        println!("Uploading {zipped_backup_path}...");

        let zip_name = zipped_backup_path.splitn(1, "/").last().unwrap();
        let object_key = format!("backup_{current_time_str}/{zip_name}");

        let archive_byte_stream = ByteStream::from_path(&Path::new(zipped_backup_path))
            .await
            .context("Creating ByteStream from immich-backup.zip")?;
        s3_client
            .put_object()
            .key(object_key)
            .body(archive_byte_stream)
            .bucket(&bucket_name)
            .send()
            .await?;
    }

    Ok(())
}

pub async fn backup(immich_go_command: &str) -> Result<()> {
    let backup_zips = pull_and_zip(immich_go_command).await?;

    push_to_s3(&backup_zips).await?;

    Ok(())
}
