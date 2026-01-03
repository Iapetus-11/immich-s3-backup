use anyhow::Result;
use dotenvy::dotenv;

use crate::immich_go_manager::ensure_immich_go;

mod backup;
mod immich_go_manager;
mod archive;

fn main() -> Result<()> {
    let _ = dotenv();

    ensure_immich_go()?;

    Ok(())
}
