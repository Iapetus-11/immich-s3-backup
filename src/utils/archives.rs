use std::{
    fs::{File, remove_file},
    io::{self},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use tokio::task::spawn_blocking;
use zip::{ZIP64_BYTES_THR, ZipWriter, write::SimpleFileOptions};

use tracing::error;

use crate::utils::dir_reader::FileEntry;

pub async fn pack_zip(
    source_dir: PathBuf,
    files: Vec<FileEntry>,
    output_path: &Path,
    compress: bool,
    password: &'static str,
) -> Result<()> {
    let output_path_clone = output_path.to_path_buf();
    spawn_blocking(move || _pack_zip(source_dir, files, output_path_clone, compress, password))
        .await
        .with_context(|| format!("Failed to pack zip file {output_path:?}"))??;
    Ok(())
}

fn _pack_zip(
    source_dir: PathBuf,
    files: Vec<FileEntry>,
    output_zip_path: PathBuf,
    compress: bool,
    password: &'static str,
) -> Result<()> {
    let mut output_file = File::create(&output_zip_path)
        .with_context(|| format!("Creating file at {output_zip_path:?}"))?;

    let zip_options = get_file_options(compress, password);

    let mut zip_writer = ZipWriter::new(&mut output_file);

    for file in &files {
        let result = write_file_to_zip(&source_dir, file, &mut zip_writer, zip_options)
            .with_context(|| format!("Failed to write {file} to zip"));

        if result.is_err() {
            let cleanup_result = remove_file(&output_zip_path);
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

fn get_file_options(compress: bool, password: &'static str) -> SimpleFileOptions {
    let mut options = SimpleFileOptions::default();

    if compress {
        options = options
            .compression_method(zip::CompressionMethod::Deflated)
            .compression_level(Some(6));
    } else {
        options = options.compression_method(zip::CompressionMethod::Stored);
    }

    options = options.with_aes_encryption(zip::AesMode::Aes256, password);

    options
}

fn write_file_to_zip<ZW: io::Write + io::Seek>(
    source_dir: &Path,
    file_entry: &FileEntry,
    zip_writer: &mut ZipWriter<ZW>,
    mut options: SimpleFileOptions,
) -> Result<()> {
    options = options.large_file(file_entry.metadata.len() >= ZIP64_BYTES_THR);

    let relative_entry_path = file_entry
        .path
        .strip_prefix(source_dir)
        .ok()
        .with_context(|| format!("{file_entry} is outside of {source_dir:?}"))?;

    zip_writer
        .start_file_from_path(relative_entry_path, options)
        .with_context(|| format!("Starting file {file_entry}"))?;

    let mut file = File::open(&file_entry.path).with_context(|| format!("Opening {file_entry}"))?;

    io::copy(&mut file, zip_writer)
        .with_context(|| format!("Copying data from {file_entry} into zip"))?;

    Ok(())
}
