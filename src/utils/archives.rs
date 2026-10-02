use std::{
    fs::File,
    io,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use tokio::task::spawn_blocking;
use zip::{ZIP64_BYTES_THR, ZipWriter, write::FileOptions};

use crate::utils::dir_reader::FileEntry;

pub struct PackZipArgs<F> {
    pub source_dir: PathBuf,
    pub file_entries: Vec<FileEntry>,
    pub output_file: F,
    pub compress: bool,
    pub password: String,
}

pub async fn pack_zip(
    PackZipArgs {
        source_dir,
        file_entries,
        output_file,
        compress,
        password,
    }: PackZipArgs<tokio::fs::File>,
) -> Result<()> {
    let args = PackZipArgs {
        source_dir,
        file_entries,
        output_file: output_file.into_std().await,
        compress,
        password,
    };

    spawn_blocking(move || blocking_pack_zip(args))
        .await
        .context("packing zip file")??;
    Ok(())
}

fn blocking_pack_zip(
    PackZipArgs {
        source_dir,
        file_entries,
        mut output_file,
        compress,
        password,
    }: PackZipArgs<File>,
) -> Result<()> {
    let zip_options = get_file_options(compress, password.as_str());

    let mut zip_writer = ZipWriter::new(&mut output_file);

    for file_entry in &file_entries {
        write_file_to_zip(&source_dir, file_entry, &mut zip_writer, zip_options)
            .with_context(|| format!("writing {file_entry} to zip"))?;
    }

    zip_writer.finish().context("finalizing zip")?;

    Ok(())
}

fn get_file_options(compress: bool, password: &str) -> FileOptions<'_, ()> {
    let mut options = FileOptions::default();

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
    mut options: FileOptions<'_, ()>,
) -> Result<()> {
    // AES and compression have overhead
    const SIZE_MARGIN: u64 = 16 * 1024 * 1024;
    options = options.large_file(file_entry.metadata.len() + SIZE_MARGIN >= ZIP64_BYTES_THR);

    let relative_entry_path = file_entry.path.strip_prefix(source_dir).with_context(|| {
        format!(
            "getting {file_entry} path relative to {}",
            source_dir.display()
        )
    })?;

    zip_writer
        .start_file_from_path(relative_entry_path, options)
        .with_context(|| format!("starting zip entry for {file_entry}"))?;

    let mut file = File::open(&file_entry.path).with_context(|| format!("opening {file_entry}"))?;

    io::copy(&mut file, zip_writer)
        .with_context(|| format!("copying data from {file_entry} into zip"))?;

    Ok(())
}
