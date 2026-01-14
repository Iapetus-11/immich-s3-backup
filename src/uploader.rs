use std::{env, path::Path};

use anyhow::{Context, Result};
use aws_sdk_s3::primitives::ByteStream;
use tracing::info;

#[derive(Clone)]
pub struct S3Uploader {
    s3_client: aws_sdk_s3::Client,
    bucket_name: String,
    base_object_key: String,
}

impl S3Uploader {
    pub async fn new_from_env(backup_name: String) -> Result<Self> {
        let bucket_name = env::var("AWS_BUCKET_NAME").context("Missing env AWS_BUCKET_NAME")?;

        let aws_sdk_config = aws_config::load_from_env().await;
        let s3_client = aws_sdk_s3::Client::new(&aws_sdk_config);

        Ok(Self {
            s3_client,
            bucket_name,
            base_object_key: backup_name,
        })
    }

    pub async fn upload_file(&self, path: &Path, key: &str) -> Result<()> {
        info!("Uploading {path:?}...");

        let key = format!("{}/{key}", self.base_object_key);
        let key = key.trim_matches('/');

        let byte_stream = ByteStream::from_path(path)
            .await
            .with_context(|| format!("Creating ByteStream from path {path:?}"))?;

        self.s3_client
            .put_object()
            .key(key)
            .body(byte_stream)
            .bucket(&self.bucket_name)
            .send()
            .await
            .with_context(|| format!("Uploading {path:?}"))?;

        info!("Done uploading {path:?}");

        Ok(())
    }
}
