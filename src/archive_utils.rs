use std::{
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::{Context, Result};
use tokio::process::Command;

pub async fn pack_zip(path: &Path) -> Result<PathBuf> {
    let path = path.to_string_lossy().to_string();

    let output_path_str = format!("{path}.zip");
    let output_path = PathBuf::from_str(&output_path_str).context("Constructing output PathBuf")?;

    Command::new("zip")
        .arg("-9rq")
        .arg(output_path_str)
        .arg(&path)
        .status()
        .await
        .with_context(|| format!("Zipping {path}"))?;

    Ok(output_path)
}
