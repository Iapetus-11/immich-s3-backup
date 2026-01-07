use std::env;

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

    let mut backup_folder = env::temp_dir();
    backup_folder.extend([
        "backups",
        &Utc::now().format("%Y-%m-%d_%H-%M-%S").to_string(),
    ]);

    fs::create_dir_all(&backup_folder)
        .await
        .with_context(|| format!("Creating dir {}", backup_folder.to_string_lossy()))?;

    pull::run(&backup_folder).await?;
    push::run(&backup_folder).await?;

    println!("Done!");

    Ok(())
}
