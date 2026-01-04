async fn pull_immich_db() {
    todo!();
}

async fn pull_immich_files() {
    todo!();
}

async fn run() -> Result<()> {
    pull_immich_db().await?;
    pull_immich_files().await?;
}