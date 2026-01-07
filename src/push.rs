use std::{
    env,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use aws_sdk_s3::primitives::ByteStream;
use chrono::Utc;
use tokio::{fs, task::JoinSet};

async fn push_file(
    path: &Path,
    base_object_key: &str,
    bucket_name: &str,
    s3_client: &aws_sdk_s3::Client,
) -> Result<()> {
    let file_name = path.file_name().unwrap().to_str().unwrap();
    let object_key = format!("{base_object_key}/{file_name}");

    let archive_byte_stream = ByteStream::from_path(path)
        .await
        .with_context(|| format!("Creating ByteStream for {path:?}"))?;

    s3_client
        .put_object()
        .key(object_key)
        .body(archive_byte_stream)
        .bucket(bucket_name)
        .send()
        .await?;

    Ok(())
}

pub async fn run(backup_folder: &Path) -> Result<()> {
    let bucket_name = env::var("AWS_BUCKET_NAME").context("Missing env AWS_BUCKET_NAME")?;

    let aws_sdk_config = aws_config::load_from_env().await;
    let s3_client = aws_sdk_s3::Client::new(&aws_sdk_config);

    let base_object_key = format!("backup_{}", Utc::now().format("%Y-%m-%d_%H-%M-%S"));

    let mut upload_tasks: JoinSet<(PathBuf, Result<()>)> = JoinSet::new();

    let mut dir_reader = fs::read_dir(backup_folder)
        .await
        .with_context(|| format!("Listing entries in {backup_folder:?}"))?;
    while let Some(dir_entry) = dir_reader
        .next_entry()
        .await
        .with_context(|| format!("Reading dir entry from {backup_folder:?}"))?
    {
        let entry_path = dir_entry.path();

        let base_object_key = base_object_key.clone();
        let bucket_name = bucket_name.clone();
        let s3_client = s3_client.clone();

        upload_tasks.spawn(async move {
            (
                entry_path.clone(),
                push_file(&entry_path, &base_object_key, &bucket_name, &s3_client).await,
            )
        });
    }

    while let Some(upload_task_result) = upload_tasks.join_next().await {
        let (upload_path, upload_result) = upload_task_result?;
        upload_result.with_context(|| format!("While uploading {upload_path:?}"))?;
    }

    Ok(())
}
