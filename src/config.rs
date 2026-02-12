use std::{path::PathBuf, sync::LazyLock};

use clap::Parser;
use dotenvy::dotenv;

#[derive(Debug, clap::Parser)]
pub struct Config {
    // Immich
    #[arg(long, env = "DB_HOST")]
    pub db_host: String,
    #[arg(long, env = "DB_USERNAME")]
    pub db_username: String,
    #[arg(long, env = "DB_PASSWORD", hide_env_values = true)]
    pub db_password: String,
    #[arg(long, env = "UPLOAD_LOCATION")]
    pub upload_location: PathBuf,

    // S3/R2 credentials
    #[arg(long, env = "AWS_ACCESS_KEY_ID")]
    pub aws_access_key_id: String,
    #[arg(long, env = "AWS_SECRET_ACCESS_KEY", hide_env_values = true)]
    pub aws_secret_access_key: String,
    #[arg(long, env = "AWS_SESSION_TOKEN", hide_env_values = true)]
    pub aws_session_token: Option<String>,
    #[arg(long, env = "AWS_REGION", default_value = "auto")]
    pub aws_region: String,
    #[arg(long, env = "AWS_BUCKET_NAME")]
    pub aws_bucket_name: String,

    #[arg(long, env = "HISTORY_FILE", default_value = "./backup-history.json")]
    pub history_file: PathBuf,
    #[arg(long, env = "BACKUP_BATCH_SIZE_BYTES", default_value = "2500000000")]
    /// Exceeding 2.5gb (2500000000) may cause uploads to fail
    pub max_batch_size_bytes: u64,
    #[arg(long, env = "BACKUP_MAX_CONCURRENT_UPLOADS", default_value = "5")]
    pub max_concurrent_uploads: usize,
}

pub static CONFIG: LazyLock<Config> = LazyLock::new(|| {
    let _ = dotenv();
    Config::parse()
});

pub fn force_load_config() {
    let _ = *CONFIG;
}
