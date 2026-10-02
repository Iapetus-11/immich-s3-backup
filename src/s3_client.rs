use std::time::Duration;

use aws_sdk_s3::config::{Credentials, Region, retry::RetryConfig, timeout::TimeoutConfig};

use crate::config::Config;

pub fn initialize_s3_client(config: &Config) -> aws_sdk_s3::Client {
    const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
    const MAX_RETRY_BACKOFF: Duration = Duration::from_secs(20);

    let attempt_timeout = Duration::from_secs(config.upload_timeout_seconds);
    let timeout_config = TimeoutConfig::builder()
        .connect_timeout(CONNECT_TIMEOUT.min(attempt_timeout))
        .operation_attempt_timeout(attempt_timeout)
        .build();

    let retry_config = RetryConfig::standard()
        .with_max_attempts(config.upload_max_retries + 1)
        .with_max_backoff(MAX_RETRY_BACKOFF);

    let mut s3_config_builder = aws_sdk_s3::Config::builder()
        .behavior_version_latest()
        .credentials_provider(Credentials::new(
            config.aws_access_key_id.clone(),
            config.aws_secret_access_key.clone(),
            config.aws_session_token.clone(),
            None,
            "Clap",
        ))
        .region(Region::new(config.aws_region.clone()))
        .retry_config(retry_config)
        .timeout_config(timeout_config);
    s3_config_builder.set_endpoint_url(config.aws_endpoint_url.clone());

    aws_sdk_s3::Client::from_conf(s3_config_builder.build())
}
