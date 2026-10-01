use std::process::ExitCode;

use tracing::{Level, error};

use crate::config::load_config;

mod backup;
mod config;
mod history;
mod postgres;
mod runner;
mod s3_client;
mod scheduler;
mod utils;

#[tokio::main]
async fn main() -> ExitCode {
    if let Err(error) = tracing_subscriber::FmtSubscriber::builder()
        .with_ansi(true)
        .with_max_level(Level::INFO)
        .try_init()
    {
        eprintln!("Failed to initialize logging: {error}");
        return ExitCode::FAILURE;
    }

    let config = load_config();

    if config.forever {
        scheduler::BackupScheduler::new(config).run_forever().await;
        return ExitCode::SUCCESS;
    }

    if let Err(error) = runner::run(config).await {
        error!("Backup failed: {error:?}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}
