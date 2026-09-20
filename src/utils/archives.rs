use std::{
    fs::{File, remove_file},
    io::{self},
    path::Path,
};

use anyhow::{Context, Result};
use tokio::task::spawn_blocking;
use zip::{ZIP64_BYTES_THR, ZipWriter, write::SimpleFileOptions};

use tracing::error;

use crate::config::CONFIG;

use super::files::FileEntry;

pub async fn pack_zip(files: Vec<FileEntry>, output_zip_path: &Path, compress: bool) -> Result<()> {
    let output_zip_path_clone = output_zip_path.to_path_buf();
    spawn_blocking(move || _pack_zip(files, &output_zip_path_clone, compress))
        .await
        .with_context(|| format!("Failed to pack zip file {output_zip_path:?}"))??;
    Ok(())
}

fn _pack_zip(files: Vec<FileEntry>, output_zip_path: &Path, compress: bool) -> Result<()> {
    let mut output_file = File::create(output_zip_path)
        .with_context(|| format!("Creating file at {output_zip_path:?}"))?;

    let zip_options = get_file_options(compress);

    let mut zip_writer = ZipWriter::new(&mut output_file);

    for file in &files {
        let result = write_file_to_zip(file, &mut zip_writer, zip_options)
            .with_context(|| format!("Failed to write {file} to zip"));

        if result.is_err() {
            let cleanup_result = remove_file(output_zip_path);
            if let Err(cleanup_error) = cleanup_result {
                error!("Failed to cleanup {output_zip_path:?}: {cleanup_error:?}");
            }
            return result;
        }
    }

    zip_writer
        .finish()
        .with_context(|| format!("Failed to finalize {output_zip_path:?}"))?;

    Ok(())
}

fn get_file_options(compress: bool) -> SimpleFileOptions {
    let mut options = SimpleFileOptions::default();

    if compress {
        options = options
            .compression_method(zip::CompressionMethod::Deflated)
            .compression_level(Some(6));
    } else {
        options = options.compression_method(zip::CompressionMethod::Stored);
    }

    options = options.with_aes_encryption(zip::AesMode::Aes256, &CONFIG.zip_password);

    options
}

fn write_file_to_zip<ZW: io::Write + io::Seek>(
    file_entry: &FileEntry,
    zip_writer: &mut ZipWriter<ZW>,
    mut options: SimpleFileOptions,
) -> Result<()> {
    options = options.large_file(file_entry.metadata.len() >= ZIP64_BYTES_THR);

    zip_writer
        .start_file_from_path(&file_entry.path, options)
        .with_context(|| format!("Starting file {file_entry}"))?;

    let mut file = File::open(&file_entry.path).with_context(|| format!("Opening {file_entry}"))?;

    io::copy(&mut file, zip_writer)
        .with_context(|| format!("Copying data from {file_entry} into zip"))?;

    Ok(())
}
