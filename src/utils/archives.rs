use std::{
    collections::HashSet,
    fs::{File, remove_file},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use tokio::task::spawn_blocking;
use zip::{ZipWriter, write::FileOptions};

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

    let mut seen_dirs = HashSet::<&Path>::new();
    let mut file_buffer = vec![0_u8; 2_usize.pow(20)].into_boxed_slice(); // 1MB

    let mut last_result = Ok(());
    for file_path in file_paths {
        last_result = write_file_to_zip(
            file_path,
            &mut zip_writer,
            &zip_options,
            &mut file_buffer,
            &mut seen_dirs,
        );

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

fn get_file_options(compress: bool) -> FileOptions<'static, ()> {
    let mut options = FileOptions::<()>::default();

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

fn write_file_to_zip<'p, 'o, ZW: io::Write + io::Seek>(
    file_path: &'p Path,
    zip_writer: &mut ZipWriter<ZW>,
    options: &FileOptions<'o, ()>,
    file_buffer: &mut [u8],
    seen_dirs: &mut HashSet<&'p Path>,
) -> Result<()> {
    ensure_file_parent_dir_in_zip(file_path, zip_writer, options, seen_dirs)?;

    zip_writer
        .start_file_from_path(file_path, *options)
        .with_context(|| format!("Starting file {file_path:?}"))?;

    let mut file = File::open(file_path).with_context(|| format!("Opening {file_path:?}"))?;

    loop {
        let read_count = file
            .read(file_buffer)
            .with_context(|| format!("Reading {file_path:?}"))?;
        if read_count == 0 {
            break;
        }

        zip_writer
            .write_all(&file_buffer[0..read_count])
            .with_context(|| format!("Writing buffered data from {file_path:?}"))?;
    }

    Ok(())
}

fn ensure_file_parent_dir_in_zip<'p, 'o, ZW: io::Write + io::Seek>(
    file_path: &'p Path,
    zip_writer: &mut ZipWriter<ZW>,
    options: &FileOptions<'o, ()>,
    seen_dirs: &mut HashSet<&'p Path>,
) -> Result<()> {
    let dir_path = file_path.parent().unwrap();

    if seen_dirs.contains(dir_path) {
        return Ok(());
    }

    seen_dirs.insert(dir_path);
    zip_writer
        .add_directory_from_path(dir_path, *options)
        .with_context(|| format!("Adding directory {dir_path:?}"))
}
