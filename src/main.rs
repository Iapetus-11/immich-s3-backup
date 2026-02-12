use std::process;

use tracing::{Level, error};

use crate::config::force_load_config;

mod backup;
mod config;
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

    if let Err(error) = backup::run().await {
        error!("{error:?}");
        process::exit(1);
    }
}
