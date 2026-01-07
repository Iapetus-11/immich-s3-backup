use std::{
    fs::{File, remove_file},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use tokio::task::spawn_blocking;
use zip::{ZipWriter, write::SimpleFileOptions};

fn write_file_to_zip<ZW: io::Write + io::Seek>(
    path: &Path,
    zip_writer: &mut ZipWriter<ZW>,
    options: SimpleFileOptions,
    file_buffer: &mut [u8],
) -> Result<()> {
    use std::fs::File;

    zip_writer
        .start_file_from_path(path, options)
        .with_context(|| format!("Starting file {path:?}"))?;

    let mut file = File::open(path).with_context(|| format!("Opening {path:?}"))?;

    loop {
        let read_count = file
            .read(file_buffer)
            .with_context(|| format!("Reading {path:?}"))?;
        if read_count == 0 {
            break;
        }

        zip_writer
            .write_all(&file_buffer[0..read_count])
            .with_context(|| format!("Writing buffered data from {path:?}"))?;
    }

    Ok(())
}

fn write_all_to_zip_recursively<ZW: io::Write + io::Seek>(
    path: &Path,
    zip_writer: &mut ZipWriter<ZW>,
    options: SimpleFileOptions,
    file_buffer: &mut [u8],
) -> Result<()> {
    if path.is_file() {
        return write_file_to_zip(path, zip_writer, options, file_buffer);
    }

    if path.is_dir() {
        zip_writer
            .add_directory_from_path(path, options)
            .with_context(|| format!("Adding directory {path:?}"))?;

        for entry in path
            .read_dir()
            .with_context(|| format!("Listing files in {path:?}"))?
        {
            let entry = entry.with_context(|| format!("Reading entry in {path:?}"))?;
            write_all_to_zip_recursively(&entry.path(), zip_writer, options, file_buffer)?;
        }

        return Ok(());
    }

    if !path.exists() {
        bail!("Path {path:?} does not exist");
    }

    bail!("Path {path:?} is neither a file or directory, and cannot be zipped");
}

pub async fn pack_zip(target_path: &Path) -> Result<PathBuf> {
    let target_name = target_path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let output_file_name = format!("{target_name}.zip");
    let output_path = target_path.with_file_name(&output_file_name);

    // Would be nice to use LZMA here, but the crate fails to build on Rust 1.91.1
    let zip_options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(Some(9));

    {
        let target_path = target_path.to_path_buf();

        spawn_blocking(move || -> Result<()> {
            let mut output_file = File::create(&output_path)
                .with_context(|| format!("Creating file at {output_path:?}"))?;

            let mut zip_writer = ZipWriter::new(&mut output_file);
            let mut file_buffer = [0; 2_usize.pow(20)]; // 1MB

            let zip_result = write_all_to_zip_recursively(
                &target_path,
                &mut zip_writer,
                zip_options,
                &mut file_buffer,
            );

            if zip_result.is_err() {
                let _ = remove_file(output_path);
            }

            zip_result
        })
        .await
    }
    .with_context(|| format!("Zipping {target_path:?}"))??;

    Ok(target_path.with_file_name(output_file_name))
}
