use anyhow::Result;
use dotenvy::dotenv;

use crate::immich_go_manager::ensure_immich_go;

mod immich_go_manager;
mod backup;

fn main() -> Result<()> {
    dotenv()?;

    ensure_immich_go()?;

    Ok(())
}
