use anyhow::{Context, Result, bail};
use tokio::fs::File;
use tokio::process::Command;

use crate::config::Config;

pub async fn run_pg_dumpall(config: &Config, dump_file: File) -> Result<()> {
    let mut pg_dumpall_cmd = {
        let mut cmd = Command::new("pg_dumpall");
        cmd.arg("--clean");
        cmd.arg("--if-exists");
        apply_connection_options(config, &mut cmd);
        cmd
    };

    let pg_dumpall_status = pg_dumpall_cmd
        .stdout(dump_file.into_std().await)
        .status()
        .await
        .context("running pg_dumpall command")?;

    if !pg_dumpall_status.success() {
        bail!("pg_dumpall failed with status {pg_dumpall_status}");
    }

    Ok(())
}

fn apply_connection_options(config: &Config, cmd: &mut Command) {
    if let Some(db_url) = &config.db_url {
        cmd.arg(format!("--dbname={db_url}"));
    } else {
        cmd.arg(format!("--host={}", config.db_hostname))
            .arg(format!("--port={}", config.db_port))
            .arg(format!("--username={}", config.db_username))
            .arg(format!("--database={}", config.db_database_name));
        if let Some(db_password) = &config.db_password {
            cmd.env("PGPASSWORD", db_password);
        }
    }

    if let Some(db_ssl_mode) = &config.db_ssl_mode {
        cmd.env("PGSSLMODE", db_ssl_mode);
    }
}
