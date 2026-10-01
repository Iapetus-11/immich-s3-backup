use std::time::Duration;

use aws_sdk_s3::config::{Credentials, Region, retry::RetryConfig, timeout::TimeoutConfig};

use crate::config::Config;

pub fn initialize_s3_client(config: &Config) -> aws_sdk_s3::Client {
    const CONNECT_TIMEOUT_SECONDS: u64 = 30;
    const ATTEMPT_TIMEOUT_SECONDS: u64 = 10 * 60;

    let request_timeout_seconds = config.upload_request_timeout_seconds;
    let timeout_config = TimeoutConfig::builder()
        .connect_timeout(Duration::from_secs(
            request_timeout_seconds.min(CONNECT_TIMEOUT_SECONDS),
        ))
        .operation_attempt_timeout(Duration::from_secs(
            request_timeout_seconds.min(ATTEMPT_TIMEOUT_SECONDS),
        ))
        .operation_timeout(Duration::from_secs(request_timeout_seconds))
        .build();

    let retry_config = RetryConfig::standard().with_max_attempts(config.upload_max_attempts);

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
