use anyhow::{Context, Result, bail};
use tokio::fs::File;
use tokio::process::Command;

use crate::config::CONFIG;

pub async fn run_pg_dumpall(dump_file: File) -> Result<()> {
    let mut pg_dumpall_cmd = {
        let mut cmd = Command::new("pg_dumpall");
        cmd.arg("--clean");
        cmd.arg("--if-exists");
        apply_connection_options(&mut cmd);
        cmd
    };

    let pg_dumpall_result = pg_dumpall_cmd
        .stdout(dump_file.into_std().await)
        .status()
        .await
        .with_context(|| "Running pg_dumpall".to_string());

    match pg_dumpall_result {
        Ok(pg_dumpall_output) => {
            if !pg_dumpall_output.success() {
                let error_code = pg_dumpall_output
                    .code()
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                bail!("Error exit code from pg_dumpall: {error_code}")
            } else {
                Ok(())
            }
        }
        Err(error) => Err(error),
    }
}

fn apply_connection_options(cmd: &mut Command) {
    if let Some(db_url) = &CONFIG.db_url {
        cmd.arg(format!("--dbname={db_url}"));
    } else {
        cmd.arg(format!("--host={}", CONFIG.db_hostname))
            .arg(format!("--port={}", CONFIG.db_port))
            .arg(format!("--username={}", CONFIG.db_username))
            .arg(format!("--database={}", CONFIG.db_database_name));
        if let Some(db_password) = &CONFIG.db_password {
            cmd.env("PGPASSWORD", db_password);
        }
    }

    if let Some(db_ssl_mode) = &CONFIG.db_ssl_mode {
        cmd.env("PGSSLMODE", db_ssl_mode);
    }
}
