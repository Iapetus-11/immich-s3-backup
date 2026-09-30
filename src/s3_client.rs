use std::time::Duration;

use aws_sdk_s3::config::{Credentials, Region, retry::RetryConfigBuilder};

use crate::config::Config;

pub fn initialize_s3_client(config: &Config) -> aws_sdk_s3::Client {
    let retry_config = RetryConfigBuilder::new()
        .max_attempts(config.max_upload_retries)
        .initial_backoff(Duration::from_secs(1))
        .max_backoff(Duration::from_secs(config.max_upload_retry_backoff))
        .build();

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
        .retry_config(retry_config);
    s3_config_builder.set_endpoint_url(config.aws_endpoint_url.clone());

    aws_sdk_s3::Client::from_conf(s3_config_builder.build())
}
