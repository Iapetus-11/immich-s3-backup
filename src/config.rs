use std::path::PathBuf;
use std::sync::Arc;

use clap::{
    Parser,
    builder::{NonEmptyStringValueParser, RangedU64ValueParser},
    value_parser,
};
use dotenvy::dotenv;
use tracing::error;

pub type Config = Arc<ConfigArgs>;

#[derive(Debug, clap::Parser)]
pub struct ConfigArgs {
    // CLI ---------------------------------------------------------------------
    #[arg(long)]
    pub forever: bool,

    // Immich ------------------------------------------------------------------
    // TODO: Support Immich's `_FILE` secret variants (e.g. DB_PASSWORD_FILE, DB_URL_FILE).
    // Overrides the individual DB_* connection values when set.
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

    // Backup config -----------------------------------------------------------
    #[arg(long, env = "BACKUP_UPLOAD_BUCKET_NAME")]
    pub upload_bucket_name: String,

    #[arg(
        long,
        env = "BACKUP_HISTORY_FILE_PATH",
        default_value = "./backup-history.sqlite"
    )]
    pub history_file_path: PathBuf,

    #[arg(
        long,
        env = "BACKUP_INTERVAL_DAYS",
        default_value = "7",
        value_parser = value_parser!(u16).range(1..)
    )]
    pub interval_days: u16,

    #[arg(
        long,
        env = "BACKUP_BATCH_MAX_SIZE_BYTES",
        default_value = "10737418240", // 10 GiB
        value_parser = value_parser!(u64).range(5 * 1024 * 1024..)
    )]
    pub batch_max_size_bytes: u64,

    #[arg(
        long,
        env = "BACKUP_UPLOAD_CHUNK_SIZE_BYTES",
        default_value = "67108864", // 64 MiB
        value_parser = value_parser!(u64).range(5 * 1024 * 1024..=5 * 1024 * 1024 * 1024)
    )]
    /// Exceeding 2.5 GiB may cause uploads to fail.
    pub upload_chunk_size_bytes: u64,

    #[arg(
        long,
        env = "BACKUP_UPLOAD_FILE_CONCURRENCY",
        default_value = "1",
        value_parser = RangedU64ValueParser::<usize>::new().range(1..)
    )]
    /// Increasing this number increases the free space required, because more batch zips will be
    /// stored on disk for upload, so you may want to decrease $BACKUP_BATCH_MAX_SIZE_BYTES.
    pub upload_file_concurrency: usize,

    #[arg(
        long,
        env = "BACKUP_UPLOAD_CHUNK_CONCURRENCY",
        default_value = "3",
        value_parser = value_parser!(u64).range(1..)
    )]
    /// This value can be multiplied by $BACKUP_UPLOAD_FILE_CONCURRENCY to determine the maximum
    /// amount of concurrent uploads.
    pub upload_chunk_concurrency: u64,

    #[arg(
        long,
        env = "BACKUP_DATABASE_DUMP_TIMEOUT_SECONDS",
        default_value = "1200",
        value_parser = value_parser!(u64).range(1..)
    )]
    pub database_dump_timeout_seconds: u64,

    #[arg(
        long,
        env = "BACKUP_UPLOAD_TIMEOUT_SECONDS",
        default_value = "300",
        value_parser = value_parser!(u64).range(1..)
    )]
    /// Maximum time for one upload API request attempt.
    pub upload_timeout_seconds: u64,

    #[arg(long, env = "BACKUP_UPLOAD_MAX_RETRIES", default_value = "10")]
    pub upload_max_retries: u32,

    #[arg(
        long,
        env = "BACKUP_ARCHIVE_PASSWORD",
        hide_env_values = true,
        value_parser = NonEmptyStringValueParser::new()
    )]
    pub archive_password: String,
}

pub fn load_config() -> Config {
    if let Err(error) = dotenv()
        && !error.not_found()
    {
        error!("Failed to load .env file: {error}");
    }

    Arc::new(ConfigArgs::parse())
}

#[cfg(test)]
pub fn get_test_config() -> Config {
    Arc::new(
        ConfigArgs::try_parse_from([
            "immich_s3_backup",
            "--db-password=password123",
            "--upload-location=/uploads",
            "--aws-access-key-id=access-key-id",
            "--aws-secret-access-key=secret-access-key",
            "--upload-bucket-name=bucket",
            "--archive-password=password123",
        ])
        .expect("configuration should be valid"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use clap::CommandFactory;

    #[test]
    fn config_definition_is_valid() {
        ConfigArgs::command().debug_assert();
        get_test_config();
    }

    #[test]
    fn empty_archive_password_is_rejected() {
        let result = ConfigArgs::try_parse_from([
            "immich_s3_backup",
            "--db-password=password123",
            "--upload-location=/uploads",
            "--aws-access-key-id=access-key-id",
            "--aws-secret-access-key=secret-access-key",
            "--upload-bucket-name=bucket",
            "--archive-password=",
        ]);

        assert!(result.is_err());
    }
}
