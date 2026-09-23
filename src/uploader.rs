use anyhow::{Context, Result, bail};
use aws_sdk_s3::primitives::{ByteStream, Length};
use aws_sdk_s3::types::{CompletedMultipartUpload, CompletedPart};
use futures_util::{StreamExt, TryStreamExt, stream};
use tracing::{error, info};

use crate::config::CONFIG;
use crate::utils::dir_reader::FileEntry;

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
        file: &FileEntry,
        key: &str,
        content_type: Option<String>,
    ) -> Result<()> {
        info!("Uploading {file}...");

        let key = format!("{}/{key}", self.base_object_key);
        let key = key.trim_matches('/');

        let create_response = self
            .s3_client
            .create_multipart_upload()
            .bucket(&self.bucket_name)
            .key(key)
            .set_content_type(content_type)
            .send()
            .await
            .context("Starting multipart upload")?;
        let upload_id = create_response.upload_id().unwrap_or_default();

        let upload_result = async {
            let upload_parts = self
                .upload_file_multipart_parts(file, key, upload_id)
                .await?;

            let complete_input = CompletedMultipartUpload::builder()
                .set_parts(Some(upload_parts))
                .build();

            self.s3_client
                .complete_multipart_upload()
                .bucket(&self.bucket_name)
                .key(key)
                .upload_id(upload_id)
                .multipart_upload(complete_input)
                .send()
                .await
                .with_context(|| format!("Finalizing multipart upload for file {file}"))?;

            Ok(())
        }
        .await;

        if upload_result.is_err() {
            let cleanup_result = self
                .s3_client
                .abort_multipart_upload()
                .bucket(&self.bucket_name)
                .key(key)
                .upload_id(upload_id)
                .send()
                .await;

            if let Err(cleanup_error) = cleanup_result {
                error!("Failed to abort multipart upload: {cleanup_error:?}");
            }
        } else {
            info!("Done uploading {file}");
        }

        upload_result
    }

    async fn upload_file_multipart_parts(
        &self,
        file: &FileEntry,
        key: &str,
        upload_id: &str,
    ) -> Result<Vec<CompletedPart>> {
        let part_count = file.metadata.len().div_ceil(CONFIG.multipart_chunk_size) as i32;
        if part_count < 1 {
            bail!("File {file} is empty");
        }

        let upload_part_futures = stream::iter(1..=part_count)
            .map(|part_idx| self.upload_file_multipart_part(file, key, upload_id, part_idx))
            .buffered(CONFIG.file_chunk_upload_concurrency);

        upload_part_futures.try_collect().await
    }

    async fn upload_file_multipart_part(
        &self,
        file: &FileEntry,
        key: &str,
        upload_id: &str,
        part_idx: i32,
    ) -> Result<CompletedPart> {
        let offset = (part_idx as u64 - 1) * CONFIG.multipart_chunk_size;
        let remaining_bytes = file.metadata.len().saturating_sub(offset);
        let byte_stream_length = Length::Exact(CONFIG.multipart_chunk_size.min(remaining_bytes));

        let byte_stream = ByteStream::read_from()
            .path(&file.path)
            .offset(offset)
            .length(byte_stream_length)
            .build()
            .await
            .with_context(|| format!("Creating ByteStream from file {file}"))?;

        let upload_response = self
            .s3_client
            .upload_part()
            .bucket(&self.bucket_name)
            .key(key)
            .upload_id(upload_id)
            .part_number(part_idx)
            .body(byte_stream)
            .send()
            .await
            .with_context(|| format!("Uploading part {part_idx} from file {file}"))?;

        Ok(CompletedPart::builder()
            .e_tag(upload_response.e_tag().unwrap_or_default())
            .part_number(part_idx)
            .build())
    }
}
