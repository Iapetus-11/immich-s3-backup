// use std::{env, path::Path};

// use anyhow::{Context, Result};
// use aws_sdk_s3::primitives::ByteStream;
// use chrono::Utc;

// use crate::consts::IMMICH_BACKUP_DIR;

// pub async fn run(backup_folder: &Path) -> Result<()> {
//     let aws_sdk_config = aws_config::load_from_env().await;
//     let s3_client = aws_sdk_s3::Client::new(&aws_sdk_config);

//     let bucket_name = env::var("AWS_BUCKET_NAME").context("Missing env AWS_BUCKET_NAME")?;

//     let current_time_str = Utc::now().format("%Y-%m-%d_%H-%M-%S").to_string();

//     // TODO: Can we paralellize this?
//     for zipped_backup_path in backup_zips {
//         println!("Uploading {zipped_backup_path}...");

//         let zip_name = zipped_backup_path.splitn(1, "/").last().unwrap();
//         let object_key = format!("backup_{current_time_str}/{zip_name}");

//         let archive_byte_stream = ByteStream::from_path(&Path::new(zipped_backup_path))
//             .await
//             .context("Creating ByteStream from immich-backup.zip")?;
//         s3_client
//             .put_object()
//             .key(object_key)
//             .body(archive_byte_stream)
//             .bucket(&bucket_name)
//             .send()
//             .await?;
//     }

//     Ok(())
// }
