use std::{path::PathBuf, sync::LazyLock};

use clap::{Parser, builder::RangedU64ValueParser, value_parser};
use dotenvy::dotenv;

#[derive(Debug, clap::Parser)]
pub struct Config {
    // CLI ---------------------------------------------------------------------
    #[arg(long, default_value = "false")]
    pub forever: bool,

    // Immich ------------------------------------------------------------------
    // TODO: Support Immich's `_FILE` secret variants (e.g. DB_PASSWORD_FILE, DB_URL_FILE).
    /// Overrides the individual DB_* connection values when set.
    #[arg(long, env = "DB_URL", hide_env_values = true)]
    pub db_url: Option<String>,

    #[arg(long, env = "DB_HOSTNAME", default_value = "database")]
    pub db_hostname: String,

    #[arg(long, env = "DB_PORT", default_value = "5432")]
    pub db_port: u16,

    #[arg(long, env = "DB_USERNAME", default_value = "postgres")]
    pub db_username: String,

    #[arg(
        long,
        env = "DB_PASSWORD",
        hide_env_values = true,
        required_unless_present = "db_url"
    )]
    pub db_password: Option<String>,

    #[arg(long, env = "DB_DATABASE_NAME", default_value = "immich")]
    pub db_database_name: String,

    #[arg(long, env = "DB_SSL_MODE")]
    pub db_ssl_mode: Option<String>,

    #[arg(long, env = "UPLOAD_LOCATION")]
    pub upload_location: PathBuf,

    // S3/R2 credentials -------------------------------------------------------
    #[arg(long, env = "AWS_ACCESS_KEY_ID")]
    pub aws_access_key_id: String,

    #[arg(long, env = "AWS_SECRET_ACCESS_KEY", hide_env_values = true)]
    pub aws_secret_access_key: String,

    #[arg(long, env = "AWS_SESSION_TOKEN", hide_env_values = true)]
    pub aws_session_token: Option<String>,

    #[arg(long, env = "AWS_REGION", default_value = "auto")]
    pub aws_region: String,

    #[arg(long, env = "AWS_ENDPOINT_URL")]
    pub aws_endpoint_url: Option<String>,

    #[arg(long, env = "AWS_BUCKET_NAME")]
    pub aws_bucket_name: String,

    // Backup config -----------------------------------------------------------
    #[arg(
        long,
        env = "BACKUP_HISTORY_FILE",
        default_value = "./backup-history.sqlite"
    )]
    pub history_file: PathBuf,

    #[arg(
        long,
        env = "BACKUP_INTERVAL_DAYS",
        default_value = "7",
        value_parser = value_parser!(u16).range(1..)
    )]
    pub interval_days: u16,

    #[arg(
        long,
        env = "BACKUP_BATCH_SIZE_BYTES",
        default_value = "10737418240", // 10 GiB
        value_parser = value_parser!(u64).range(5 * 1024 * 1024..)
    )]
    pub max_batch_size_bytes: u64,

    #[arg(
        long,
        env = "BACKUP_MULTIPART_CHUNK_SIZE",
        default_value = "67108864", // 64 MiB
        value_parser = value_parser!(u64).range(5 * 1024 * 1024..=5 * 1024 * 1024 * 1024)
    )]
    /// Exceeding 2.5 GiB may cause uploads to fail.
    pub multipart_chunk_size: u64,

    #[arg(
        long,
        env = "BACKUP_FILE_UPLOAD_CONCURRENCY",
        default_value = "1",
        value_parser = RangedU64ValueParser::<usize>::new().range(1..)
    )]
    /// Increasing this number increases the free space required, because more batch zips will be
    /// stored on disk for upload, so you may want to decrease $BACKUP_BATCH_SIZE_BYTES.
    pub file_upload_concurrency: usize,

    #[arg(
        long,
        env = "BACKUP_FILE_CHUNK_UPLOAD_CONCURRENCY",
        default_value = "3",
        value_parser = RangedU64ValueParser::<usize>::new().range(1..)
    )]
    /// This value can be multiplied by $BACKUP_FILE_UPLOAD_CONCURRENCY to determine the maximum
    /// amount of concurrent uploads.
    pub file_chunk_upload_concurrency: usize,

    #[arg(
        long,
        env = "BACKUP_MAX_UPLOAD_RETRIES",
        default_value = "10",
        value_parser = value_parser!(u32).range(1..)
    )]
    pub max_upload_retries: u32,

    #[arg(
        long,
        env = "BACKUP_MAX_UPLOAD_RETRY_BACKOFF",
        default_value = "30",
        value_parser = value_parser!(u64).range(1..)
    )]
    pub max_upload_retry_backoff: u64,

    #[arg(long, env = "BACKUP_ZIP_PASSWORD")]
    pub zip_password: String,
}

pub static CONFIG: LazyLock<Config> = LazyLock::new(|| {
    let _ = dotenv();
    Config::parse()
});

pub fn force_load_config() {
    LazyLock::force(&CONFIG);
}

#[cfg(test)]
mod tests {
    use super::*;

    use clap::CommandFactory;

    #[test]
    fn config_definition_is_valid() {
        Config::command().debug_assert();
    }

    #[test]
    fn config_parses_with_defaults() {
        Config::try_parse_from([
            "immich_s3_backup",
            "--db-password=x",
            "--upload-location=/tmp",
            "--aws-access-key-id=x",
            "--aws-secret-access-key=x",
            "--aws-bucket-name=x",
            "--zip-password=x",
        ])
        .unwrap();
    }
}
