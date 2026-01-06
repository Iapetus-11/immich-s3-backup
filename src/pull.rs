use std::{
    env,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use tokio::{
    fs::{self, File},
    io::AsyncWriteExt,
    process::Command,
};

use crate::{archive_utils::pack_zip, consts::DB_BACKUP_FILENAME};

/// Creates a dump of the immich database and zips it, returning the path to that zip file
/// Reference https://docs.immich.app/administration/backup-and-restore/#database
async fn pull_immich_db(backup_folder: &Path) -> Result<()> {
    println!("Dumping immich database...");

    let db_username = env::var("DB_USERNAME").context("Missing env DB_USERNAME")?;
    let db_password = env::var("DB_PASSWORD").context("Missing env DB_PASSWORD")?;

    let pg_dumpall_output = Command::new("pg_dumpall")
        .arg("--clean")
        .arg("--if-exists")
        .arg(format!("--username={db_username}"))
        .env("PGPASSWORD", db_password)
        .output()
        .await
        .with_context(|| "Running pg_dumpall".to_string())?;

    if !pg_dumpall_output.status.success() {
        bail!(
            "Failed to run pg_dumpall: {} {}",
            String::from_utf8_lossy(&pg_dumpall_output.stdout),
            String::from_utf8_lossy(&pg_dumpall_output.stderr)
        );
    }

    let mut dump_file_path = PathBuf::from(backup_folder);
    dump_file_path.push(DB_BACKUP_FILENAME);

    let mut dump_file = File::create(&dump_file_path)
        .await
        .with_context(|| format!("Creating {DB_BACKUP_FILENAME}"))?;
    dump_file
        .write_all(&pg_dumpall_output.stdout)
        .await
        .with_context(|| format!("Writing {DB_BACKUP_FILENAME}"))?;

    pack_zip(&dump_file_path)
        .await
        .with_context(|| format!("Zipping {}", dump_file_path.to_string_lossy()))?;

    fs::remove_file(&dump_file_path)
        .await
        .with_context(|| format!("Removing old {}", dump_file_path.to_string_lossy()))?;

    Ok(())
}

/// Zips up immich files (containing media library, profile pictures, and more)
/// Reference https://docs.immich.app/administration/backup-and-restore/#filesystem
async fn pull_immich_files(backup_folder: &Path) -> Result<()> {
    println!("Zipping immich data...");

    let upload_location = env::var("UPLOAD_LOCATION").context("Missing env UPLOAD_LOCATION")?;
    let upload_location = Path::new(&upload_location);

    const FOLDERS_TO_BACKUP: &[&str] = &["library", "upload", "profile"];

    for folder_str in FOLDERS_TO_BACKUP {
        let mut folder_path = backup_folder.to_path_buf();
        folder_path.push(folder_str);

        let zipped_folder_path = pack_zip(&folder_path).await.with_context(|| {
            format!("Zipping {}/{folder_str}", upload_location.to_string_lossy())
        })?;

        let zipped_folder_file_name = zipped_folder_path.file_name().unwrap();

        // Move zipped folder to backup location
        fs::rename(
            &zipped_folder_path,
            PathBuf::from_iter([backup_folder, Path::new(zipped_folder_file_name)]),
        )
        .await
        .with_context(|| {
            format!(
                "Moving {} to {}",
                zipped_folder_file_name.to_string_lossy(),
                backup_folder.to_string_lossy()
            )
        })?;
    }

    Ok(())
}

pub async fn run(backup_folder: &Path) -> Result<()> {
    pull_immich_db(backup_folder)
        .await
        .context("Pulling immich DB")?;
    pull_immich_files(backup_folder)
        .await
        .context("Pulling immich files")?;

    Ok(())
}
