use std::process;

use tracing::{Level, error};

use crate::config::{CONFIG, force_load_config};

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
    force_load_config();

    tracing::subscriber::set_global_default(
        tracing_subscriber::FmtSubscriber::builder()
            .with_ansi(true)
            .with_max_level(Level::INFO)
            .finish(),
    )
    .unwrap();

    if CONFIG.forever {
        scheduler::BackupScheduler.run_forever().await;
    } else if let Err(error) = runner::run().await {
        error!("Backup failed: {error:?}");
        process::exit(1);
    }
}
