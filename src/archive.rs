use std::process::Command;
use anyhow::{Context, Result};

pub fn unpack_targz(file_path: &str, files: &[&str]) -> Result<()> {
    Command::new("tar")
        .arg("-xzf")
        .arg(file_path)
        .args(files)
        .status()
        .with_context(|| format!("Unpacking archive {file_path}"))?;

    // TODO: Do we need to check status code? 

    Ok(())
}

fn pack_targz() {
    
}