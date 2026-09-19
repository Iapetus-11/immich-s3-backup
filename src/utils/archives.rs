use std::{
    collections::HashSet,
    fs::{File, remove_file},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use tokio::task::spawn_blocking;
use zip::{ZIP64_BYTES_THR, ZipWriter, write::SimpleFileOptions};

use tracing::error;

use crate::config::CONFIG;

pub async fn pack_zip(
    file_paths: Vec<PathBuf>,
    output_zip_path: &Path,
    compress: bool,
) -> Result<()> {
    let output_zip_path_clone = output_zip_path.to_path_buf();
    spawn_blocking(move || {
        let file_paths = file_paths.iter().map(PathBuf::as_path).collect::<Vec<_>>();
        _pack_zip(&file_paths, &output_zip_path_clone, compress)
    })
    .await
    .with_context(|| format!("Failed to pack zip file {output_zip_path:?}"))??;
    Ok(())
}

fn _pack_zip(file_paths: &[&Path], output_zip_path: &Path, compress: bool) -> Result<()> {
    let mut output_file = File::create(output_zip_path)
        .with_context(|| format!("Creating file at {output_zip_path:?}"))?;

    let zip_options = get_file_options(compress);

    let mut zip_writer = ZipWriter::new(&mut output_file);

    let mut last_result = Ok(());
    for file_path in file_paths {
        last_result = write_file_to_zip(file_path, &mut zip_writer, zip_options);

        if last_result.is_err() {
            let _ = remove_file(output_zip_path);
            break;
        }
    }

    last_result.with_context(|| {
        format!(
            "Zipping {} files into {output_zip_path:?}",
            file_paths.len()
        )
    })
}

fn get_file_options(compress: bool) -> SimpleFileOptions {
    let mut options = SimpleFileOptions::default();

    if compress {
        options = options
            .compression_method(zip::CompressionMethod::Deflated)
            .compression_level(Some(9));
    } else {
        options = options.compression_method(zip::CompressionMethod::Stored);
    }

    options = options.with_aes_encryption(zip::AesMode::Aes256, &CONFIG.zip_password);

    options
}

fn write_file_to_zip<ZW: io::Write + io::Seek>(
    file_path: &Path,
    zip_writer: &mut ZipWriter<ZW>,
    mut options: SimpleFileOptions,
) -> Result<()> {
    zip_writer
        .start_file_from_path(file_path, options)
        .with_context(|| format!("Starting file {file_path:?}"))?;

    let mut file = File::open(file_path).with_context(|| format!("Opening {file_path:?}"))?;

    let metadata_result = file.metadata();
    match metadata_result {
        Ok(metadata) => {
            options = options.large_file(metadata.len() >= ZIP64_BYTES_THR);
        }
        Err(error) => {
            error!("Failed to read {file_path:?} metadata (cannot determine large_file zip flag)");
        }
    }

    io::copy(&mut file, zip_writer).context("Copying data from {file_path:?} into zip")?;

    Ok(())
}
