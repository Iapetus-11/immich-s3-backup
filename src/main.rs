use std::process;

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
async fn main() {
    tracing::subscriber::set_global_default(
        tracing_subscriber::FmtSubscriber::builder()
            .with_ansi(true)
            .with_max_level(Level::INFO)
            .finish(),
    )
    .unwrap();

    let config = load_config();

    if config.forever {
        scheduler::BackupScheduler::new(config).run_forever().await;
        return;
    }

    if let Err(error) = runner::run(config).await {
        error!("Backup failed: {error:?}");
        process::exit(1);
    }
}
