use std::time::Duration;

use aws_sdk_s3::config::{Credentials, Region, retry::RetryConfigBuilder};

use crate::config::CONFIG;

pub fn initialize_s3_client() -> aws_sdk_s3::Client {
    let retry_config = RetryConfigBuilder::new()
        .max_attempts(CONFIG.max_upload_retries)
        .initial_backoff(Duration::from_secs(1))
        .max_backoff(Duration::from_secs(CONFIG.max_upload_retry_backoff))
        .build();

    let mut s3_config_builder = aws_sdk_s3::Config::builder()
        .behavior_version_latest()
        .credentials_provider(Credentials::new(
            CONFIG.aws_access_key_id.clone(),
            CONFIG.aws_secret_access_key.clone(),
            CONFIG.aws_session_token.clone(),
            None,
            "Clap",
        ))
        .region(Region::new(CONFIG.aws_region.clone()))
        .retry_config(retry_config);
    s3_config_builder.set_endpoint_url(CONFIG.aws_endpoint_url.clone());

    aws_sdk_s3::Client::from_conf(s3_config_builder.build())
}
