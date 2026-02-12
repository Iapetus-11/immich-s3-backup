use std::path::Path;

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
    pub fn new(
        s3_client: aws_sdk_s3::Client,
        bucket_name: String,
        base_object_key: String,
    ) -> Self {
        Self {
            s3_client,
            bucket_name,
            base_object_key,
        }
    }

    pub async fn upload_file(
        &self,
        path: &Path,
        key: &str,
        content_type: Option<String>,
    ) -> Result<()> {
        info!("Uploading {path:?}...");

        let key = format!("{}/{key}", self.base_object_key);
        let key = key.trim_matches('/');

        let byte_stream = ByteStream::from_path(path)
            .await
            .with_context(|| format!("Creating ByteStream from path {path:?}"))?;

        self.s3_client
            .put_object()
            .bucket(&self.bucket_name)
            .key(key)
            .set_content_type(content_type)
            .body(byte_stream)
            .send()
            .await
            .with_context(|| format!("Uploading {path:?}"))?;

        info!("Done uploading {path:?}");

        Ok(())
    }
}
