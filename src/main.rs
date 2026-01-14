use std::process;

use dotenvy::dotenv;
use tracing::{Level, error};

mod archive_utils;
mod backup;
mod recursive_dir_reader;
mod uploader;

// TODO: Zipping and uploading should occur at the same time... then need to clear disk as it's deleted
// TODO: Parallelize uploading using worker system (mutex around vec of file upload paths?)

#[tokio::main]
async fn main() {
    tracing::subscriber::set_global_default(
        tracing_subscriber::FmtSubscriber::builder()
            .with_ansi(true)
            .with_max_level(Level::INFO)
            .finish(),
    )
    .unwrap();

    let _ = dotenv();

    if let Err(error) = backup::run().await {
        error!("{error:?}");
        process::exit(1);
    }
}
