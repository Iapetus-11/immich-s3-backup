use anyhow::Result;
use dotenvy::dotenv;

use crate::push::backup;

mod archive_utils;
mod push;
mod pull;
mod consts;

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenv();

    backup().await?;

    Ok(())
}
