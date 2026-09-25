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

pub struct PackZipArgs {
    source_dir: PathBuf,
    files: Vec<FileEntry>,
    output_file: File,
    compress: bool,
    password: &'static str,
}

pub async fn pack_zip(args: PackZipArgs) -> Result<()> {
    spawn_blocking(move || _pack_zip(args))
        .await
        .with_context(|| format!("Failed to pack zip file"))??;
    Ok(())
}

fn _pack_zip(
    PackZipArgs {
        source_dir,
        files,
        mut output_file,
        compress,
        password,
    }: PackZipArgs,
) -> Result<()> {
    let zip_options = get_file_options(compress, password);

    let mut zip_writer = ZipWriter::new(&mut output_file);

    for file in &files {
        let result = write_file_to_zip(&source_dir, file, &mut zip_writer, zip_options)
            .with_context(|| format!("Failed to write {file} to zip"));
    }

    zip_writer
        .finish()
        .with_context(|| format!("Failed to finalize zip"))?;

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
