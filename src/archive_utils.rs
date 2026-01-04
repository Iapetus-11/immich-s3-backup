use anyhow::{Context, Result};
use tokio::process::Command;

pub async fn unpack_targz(path: &str, files: &[&str]) -> Result<()> {
    Command::new("tar")
        .arg("-xzf")
        .arg(path)
        .args(files)
        .status()
        .await
        .with_context(|| format!("Unpacking archive {path}"))?;

    Ok(())
}

pub async fn pack_zip(path: &str) -> Result<()> {
    Command::new("zip")
        .arg("-9rq")
        .arg(format!("{path}.zip"))
        .arg(path)
        .status()
        .await
        .with_context(|| format!("Zipping {path}"))?;

    Ok(())
}
