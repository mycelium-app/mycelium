use anyhow::{Result, anyhow, bail};
use sha2::{Digest, Sha256};
use std::io::ErrorKind;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const NIX_MESSAGE: &str = "This node was installed by Nix; update it through the flake";

pub fn is_current(own: &str, wanted: &str) -> bool {
    let Some(own) = numbers(own) else {
        return false;
    };
    let Some(wanted) = numbers(wanted) else {
        return true;
    };

    own >= wanted
}

fn numbers(version: &str) -> Option<Vec<u64>> {
    let release = version.trim().trim_start_matches('v').split(['-', '+']).next()?;

    release.split('.').map(|part| part.parse().ok()).collect()
}

pub fn asset_name(os: &str, arch: &str) -> Result<String> {
    match (os, arch) {
        ("linux", "x86_64") => Ok("mycelium-linux-x86_64".into()),
        _ => Err(anyhow!("No build for {os} {arch}")),
    }
}

pub async fn install(client: &reqwest::Client, releases_url: &str, version: &str, asset: &str, target: &Path) -> Result<()> {
    let base = format!("{}/{version}/{asset}", releases_url.trim_end_matches('/'));
    let binary = download(client, &base).await?;
    let checksum = download(client, &format!("{base}.sha256")).await?;
    let expected = String::from_utf8_lossy(&checksum).split_whitespace().next().unwrap_or_default().to_lowercase();
    let actual: String = Sha256::digest(&binary).iter().map(|byte| format!("{byte:02x}")).collect();

    if expected != actual {
        bail!("The download of {asset} {version} did not match its checksum, so it was not installed.");
    }

    swap(&binary, target).map_err(|error| match error.kind() {
        ErrorKind::PermissionDenied | ErrorKind::ReadOnlyFilesystem => anyhow!(NIX_MESSAGE),
        _ => anyhow!("Could not replace {}: {error}", target.display()),
    })
}

pub fn running_binary() -> Result<PathBuf> {
    std::env::current_exe().map_err(|error| anyhow!("Could not find the running binary: {error}"))
}

async fn download(client: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    let response = client.get(url).send().await.map_err(|error| anyhow!("Could not download {url}: {error}"))?;

    if !response.status().is_success() {
        bail!("Could not download {url}: it answered {}", response.status());
    }

    let bytes = response.bytes().await.map_err(|error| anyhow!("Could not download {url}: {error}"))?;

    Ok(bytes.to_vec())
}

fn swap(binary: &[u8], target: &Path) -> std::io::Result<()> {
    let mut staged = target.as_os_str().to_owned();
    staged.push(".new");
    let staged = PathBuf::from(staged);

    std::fs::write(&staged, binary)?;
    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))?;

    let renamed = std::fs::rename(&staged, target);

    if renamed.is_err() {
        let _ = std::fs::remove_file(&staged);
    }

    renamed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_is_current_by_number_not_by_text() {
        assert!(is_current("v0.1.10", "v0.1.9"));
        assert!(is_current("0.2.0", "v0.2.0"));
        assert!(!is_current("v0.1.9", "v0.1.10"));
    }

    #[test]
    fn only_linux_x86_64_has_a_build() {
        assert_eq!(asset_name("linux", "x86_64").unwrap(), "mycelium-linux-x86_64");
        assert_eq!(asset_name("macos", "aarch64").unwrap_err().to_string(), "No build for macos aarch64");
    }
}
