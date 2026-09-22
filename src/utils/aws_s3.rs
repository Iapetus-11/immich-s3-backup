use std::time::Duration;

use aws_config::{BehaviorVersion, Region, retry::RetryConfigBuilder};
use aws_sdk_s3::config::{Credentials, SharedCredentialsProvider};

use crate::config::CONFIG;

pub fn initialize_s3_client() -> aws_sdk_s3::Client {
    let retry_config = RetryConfigBuilder::new()
        .max_attempts(CONFIG.max_upload_retries)
        .initial_backoff(Duration::from_secs(1))
        .max_backoff(Duration::from_secs(CONFIG.max_upload_retry_backoff))
        .build();

    let mut aws_sdk_config_builder = aws_config::SdkConfig::builder()
        .behavior_version(BehaviorVersion::latest())
        .credentials_provider(SharedCredentialsProvider::new(Credentials::new(
            CONFIG.aws_access_key_id.clone(),
            CONFIG.aws_secret_access_key.clone(),
            CONFIG.aws_session_token.clone(),
            None,
            "Clap",
        )))
        .region(Some(Region::from_static(&CONFIG.aws_region)))
        .retry_config(retry_config);
    aws_sdk_config_builder.set_endpoint_url(CONFIG.aws_endpoint_url.clone());
    let aws_sdk_config = aws_sdk_config_builder.build();

    aws_sdk_s3::Client::new(&aws_sdk_config)
}
