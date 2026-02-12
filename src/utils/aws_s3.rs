use std::time::Duration;

use aws_config::retry::RetryConfigBuilder;
use aws_sdk_s3::config::{Credentials, SharedCredentialsProvider};

use crate::config::CONFIG;

pub fn initialize_s3_client() -> aws_sdk_s3::Client {
    let retry_config = RetryConfigBuilder::new()
        .max_attempts(10)
        .initial_backoff(Duration::from_secs(2))
        .max_backoff(Duration::from_secs(30))
        .build();

    let aws_sdk_config = aws_config::SdkConfig::builder()
        .credentials_provider(SharedCredentialsProvider::new(Credentials::new(
            CONFIG.aws_access_key_id.clone(),
            CONFIG.aws_secret_access_key.clone(),
            CONFIG.aws_session_token.clone(),
            None,
            "Clap",
        )))
        .retry_config(retry_config)
        .build();

    aws_sdk_s3::Client::new(&aws_sdk_config)
}
