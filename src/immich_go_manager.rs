use std::{
    env,
    fs::{self, File},
    io::{Read, Write},
};

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use ureq::{BodyReader, http::StatusCode};
use which::which_all;

use crate::archive::unpack_targz;

const DEFAULT_IMMICH_GO_RELEASE_TAG: &str = "v0.31.0";

#[derive(Deserialize)]
struct GitHubRelease {
    assets: Vec<ReleaseAsset>,
}

#[derive(Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}

impl GitHubRelease {
    fn get_asset_for_current_platform(&self) -> Result<&ReleaseAsset> {
        let os_name = match env::consts::OS {
            "linux" => "linux",
            "macos" => "darwin",
            os => bail!("unsupported operating system: {os}"),
        };

        let arch_name = match env::consts::ARCH {
            "x86_64" => "x86_64",
            "aarch64" => "arm64",
            arch => bail!("unsupported architecture: {arch}"),
        };

        for asset in &self.assets {
            let asset_name = asset.name.to_lowercase();

            if asset_name.contains(os_name) && asset_name.contains(arch_name) {
                if !asset.name.ends_with(".tar.gz") {
                    bail!("Asset {} does not end with .tar.gz", asset.name);
                }

                return Ok(asset);
            }
        }

        Err(anyhow!(
            "No asset found for current platform, available assets: {}",
            self.assets
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }
}

fn is_immich_go_installed() -> Result<bool> {
    Ok(which_all("immich-go").context("Searching for immich-go executable")?.next().is_some())
}

fn get_github_release(tag: &str) -> Result<GitHubRelease> {
    let mut response = ureq::get(format!(
        "https://api.github.com/repos/simulot/immich-go/releases/tags/{tag}"
    ))
    .call()?;
    let response_body = response.body_mut().read_to_string()?;

    if response.status() == StatusCode::NOT_FOUND {
        bail!("Unable to find immich-go release for tag: {tag}");
    }

    if !response.status().is_success() {
        bail!(
            "While fetching immich-go release, received HTTP error {}: {}",
            response.status().as_u16(),
            response_body
        );
    }

    let release: GitHubRelease =
        serde_json::from_str(&response_body).with_context(|| "Failed to parse Release")?;

    Ok(release)
}

fn write_file_from_response(file: &mut File, response_reader: &mut BodyReader<'_>) -> Result<()> {
    let mut buffer = [0; 2_usize.pow(16)];

    loop {
        let read_count = response_reader
            .read(&mut buffer)
            .context("Reading response")?;
        if read_count == 0 {
            break;
        }

        file.write(&buffer[0..read_count])
            .context("Writing file from response")?;
    }

    Ok(())
}

fn download_binary_from_release(release: &GitHubRelease) -> Result<()> {
    let asset_to_download = release.get_asset_for_current_platform()?;

    let mut response = ureq::get(&asset_to_download.browser_download_url)
        .call()
        .context("Failed to download asset from GitHub")?;

    if !response.status().is_success() {
        bail!(
            "Received HTTP error {} while downloading asset",
            response.status().as_u16(),
        );
    }

    let mut response_reader = response.body_mut().as_reader();

    let mut file = File::create("immich-go.tar.gz").context("Creating immich-go.tar.gz file")?;
    write_file_from_response(&mut file, &mut response_reader)?;

    unpack_targz("immich-go.tar.gz", &["immich-go"])?;

    fs::remove_file("immich-go.tar.gz").context("Deleting immich-go.tar.gz")?;

    Ok(())
}

fn download_immich_go() -> Result<()> {
    let release_tag = env::var("IMMICH_GO_RELEASE_TAG")
        .unwrap_or_else(|_| DEFAULT_IMMICH_GO_RELEASE_TAG.to_string());

    println!("Downloading immich-go {release_tag}...");

    let release =
        get_github_release(&release_tag).context("Failed to get release details from GitHub")?;
    download_binary_from_release(&release).context("Failed to install immich-go from GitHub")?;

    Ok(())
}

pub fn ensure_immich_go() -> Result<()> {
    if !is_immich_go_installed()? {
        download_immich_go()?;
    }

    Ok(())
}
