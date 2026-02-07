use std::{ops::Deref, path::PathBuf, sync::LazyLock};

use clap::Parser;
use dotenvy::dotenv;


// TODO: What about the other AWS/CF envs?
// https://docs.aws.amazon.com/sdkref/latest/guide/environment-variables.html
#[derive(Debug, clap::Parser)]
pub struct Config {
    #[arg(long, env = "DB_HOST")]
    pub db_host: String,
    #[arg(long, env = "DB_USERNAME")]
    pub db_username: String,
    #[arg(long, env = "DB_PASSWORD")]
    pub db_password: String,

    #[arg(long, env = "UPLOAD_LOCATION")]
    pub upload_location: PathBuf,

    #[arg(long, env = "BACKUP_BATCH_SIZE_BYTES")]
    pub max_batch_size_bytes: u64,
    #[arg(long, env = "BACKUP_MAX_CONCURRENT_UPLOADS")]
    pub max_concurrent_uploads: usize,

    #[arg(long, env = "AWS_BUCKET_NAME")]
    pub aws_bucket_name: String,
}

pub static CONFIG: LazyLock<Config> = LazyLock::new(|| {
    let _ = dotenv();
    Config::parse()
});

pub fn force_load_config() {
    let _ = CONFIG.deref();
}