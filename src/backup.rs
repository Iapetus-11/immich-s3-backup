use std::{env, path::Path, process::Command};

use anyhow::{Context, Result};

pub fn backup() -> Result<i32> {
    let server_url = env::var("IMMICH_SERVER_URL").context("Failed to get env var IMMICH_SERVER_URL")?;
    let api_key = env::var("IMMICH_API_KEY").context("Failed to get env var IMMICH_API_KEY")?;

    let use_local_executable = Path::new("immich-go").exists();
    let command = match use_local_executable {
        true => "./immich-go",
        false => "immich-go",
    };

    Command::new(command)
        .arg("archive")
        .arg("from-immich")
        .arg(format!("--from-server={server_url}"))
        .arg(format!("--from-api-key={api_key}"))
        .arg("--write-to-folder=./backup")
        .output()
        .context("Failed executing");
}