use std::process;

use tracing::{Level, error};

mod archive_utils;
mod backup;
mod config;
mod recursive_dir_reader;
mod uploader;

#[tokio::main]
async fn main() {
    tracing::subscriber::set_global_default(
        tracing_subscriber::FmtSubscriber::builder()
            .with_ansi(true)
            .with_max_level(Level::INFO)
            .finish(),
    )
    .unwrap();

    if let Err(error) = backup::run().await {
        error!("{error:?}");
        process::exit(1);
    }
}
