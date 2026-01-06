use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::Utc;
use dotenvy::dotenv;
use tokio::fs;

mod archive_utils;
mod consts;
mod pull;
mod push;

#[tokio::main]
async fn main() -> Result<()> {
    println!("Running backup...");

    let _ = dotenv();

    let backup_folder = PathBuf::from_iter([
        "./backups",
        &Utc::now().format("%Y-%m-%d_%H-%M-%S").to_string(),
    ]);
    fs::create_dir_all(&backup_folder)
        .await
        .with_context(|| format!("Creating dir {}", backup_folder.to_string_lossy()))?;

    pull::run(&backup_folder).await?;

    Ok(())
}
