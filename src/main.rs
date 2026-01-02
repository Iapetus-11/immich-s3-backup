use anyhow::Result;

use crate::immich_go_manager::ensure_immich_go;

mod immich_go_manager;

fn main() -> Result<()> {
    ensure_immich_go()?;

    Ok(())
}
