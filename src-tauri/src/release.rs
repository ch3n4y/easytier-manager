use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use walkdir::WalkDir;
use zip::ZipArchive;

use crate::error::{Error, Result};
use crate::paths::{
    default_config, service_wrapper, GITHUB_PROXY_PREFIXES, RELEASE_API_URL, REQUIRED_BINARIES,
};
use crate::util::{copy_file, set_mode};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub installed: bool,
    pub current_version: String,
    pub latest_version: String,
    pub has_update: bool,
    pub asset_name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GithubRelease {
    #[serde(default)]
    pub tag_name: String,
    #[serde(default)]
    pub assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
}

/// Resolve the newest EasyTier release and the macOS asset matching this arch.
///
/// Direct GitHub access is attempted first, then the mirrors in
/// [`GITHUB_PROXY_PREFIXES`] — the release API is often unreachable from
/// mainland networks.
pub async fn latest_asset() -> Result<(GithubRelease, ReleaseAsset)> {
    let client = reqwest::Client::new();
    let mut last_error: Option<Error> = None;

    for prefix in GITHUB_PROXY_PREFIXES {
        let url = format!("{prefix}{RELEASE_API_URL}");
        let release = match fetch_release(&client, &url).await {
            Ok(release) => release,
            Err(err) => {
                last_error = Some(err);
                continue;
            }
        };

        let asset_name = asset_name_for_version(&normalize_version(&release.tag_name));
        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == asset_name)
            .cloned();

        return match asset {
            Some(asset) => Ok((release, asset)),
            None => Err(Error::msg(format!("release asset not found: {asset_name}"))),
        };
    }

    Err(last_error.unwrap_or_else(|| Error::msg("github release request failed")))
}

async fn fetch_release(client: &reqwest::Client, url: &str) -> Result<GithubRelease> {
    let response = client
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "EasyTier-Manager")
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(Error::msg(format!(
            "github release request failed: {}",
            response.status()
        )));
    }

    Ok(response.json::<GithubRelease>().await?)
}

/// Download the release zip and unpack it into `<stage>/extract`.
pub async fn download_and_extract(url: &str, stage: &Path) -> Result<()> {
    let client = reqwest::Client::new();
    let zip_path = stage.join("easytier.zip");
    let extract_dir = stage.join("extract");
    let mut last_error: Option<Error> = None;

    for download_url in proxied_urls(url) {
        let response = match client.get(&download_url).send().await {
            Ok(response) => response,
            Err(err) => {
                last_error = Some(err.into());
                continue;
            }
        };

        if !response.status().is_success() {
            last_error = Some(Error::msg(format!(
                "download failed: {}",
                response.status()
            )));
            continue;
        }

        let bytes = match response.bytes().await {
            Ok(bytes) => bytes,
            Err(err) => {
                last_error = Some(err.into());
                continue;
            }
        };

        let zip_path = zip_path.clone();
        return tokio::task::spawn_blocking(move || {
            fs::write(&zip_path, &bytes)?;
            unzip(&zip_path, &extract_dir)
        })
        .await?;
    }

    Err(last_error.unwrap_or_else(|| Error::msg("download failed")))
}

/// The direct URL first, then the same URL behind each configured mirror.
pub fn proxied_urls(raw_url: &str) -> Vec<String> {
    let mut urls = vec![raw_url.to_string()];
    if raw_url.starts_with("https://github.com/") {
        for prefix in &GITHUB_PROXY_PREFIXES[1..] {
            urls.push(format!("{prefix}{raw_url}"));
        }
    }
    urls
}

/// Extract a zip, rejecting entries that would escape `dest`.
pub fn unzip(zip_path: &Path, dest: &Path) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    let file = fs::File::open(zip_path)?;
    let mut archive = ZipArchive::new(file)?;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let Some(name) = entry.enclosed_name() else {
            return Err(Error::msg(format!(
                "zip entry escapes destination: {}",
                entry.name()
            )));
        };
        let target = dest.join(name);

        if entry.is_dir() {
            fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut out = fs::File::create(&target)?;
        io::copy(&mut entry, &mut out)?;
        out.flush()?;
        drop(out);

        if let Some(mode) = entry.unix_mode() {
            let _ = fs::set_permissions(&target, fs::Permissions::from_mode(mode));
        }
    }

    Ok(())
}

/// Collect the four binaries the service needs into `<stage>/bin`.
pub fn stage_binaries(stage: &Path) -> Result<()> {
    let extract_root = stage.join("extract");
    let bin_stage = stage.join("bin");
    fs::create_dir_all(&bin_stage)?;

    for binary in REQUIRED_BINARIES {
        let src = find_file(&extract_root, binary)?;
        copy_file(&src, &bin_stage.join(binary), 0o755)?;
    }
    Ok(())
}

/// Stage everything a first-time install has to copy into place: binaries plus
/// the generated service wrapper and config templates.
pub fn stage_install_files(stage: &Path, tag: &str) -> Result<()> {
    stage_binaries(stage)?;

    let files = [
        ("easytier-service", service_wrapper()),
        ("default.conf", default_config()),
        ("service.env", String::from("EASYTIER_CONFIG_SERVER=\"\"\n")),
        ("VERSION", format!("{}\n", normalize_version(tag))),
    ];

    for (name, contents) in files {
        fs::write(stage.join(name), contents)?;
    }

    set_mode(&stage.join("easytier-service"), 0o755)?;
    Ok(())
}

pub fn find_file(root: &Path, name: &str) -> Result<PathBuf> {
    for entry in WalkDir::new(root) {
        let entry = entry?;
        if !entry.file_type().is_dir() && entry.file_name() == name {
            return Ok(entry.into_path());
        }
    }
    Err(Error::msg(format!("required binary not found: {name}")))
}

pub fn asset_name_for_version(version: &str) -> String {
    if std::env::consts::ARCH == "aarch64" {
        format!("easytier-macos-aarch64-v{version}.zip")
    } else {
        format!("easytier-macos-x86_64-v{version}.zip")
    }
}

/// Strip the single leading `v` from a release tag.
pub fn normalize_version(tag: &str) -> String {
    let trimmed = tag.trim();
    trimmed.strip_prefix('v').unwrap_or(trimmed).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn builds_arch_specific_asset_name() {
        let expected = if std::env::consts::ARCH == "aarch64" {
            "easytier-macos-aarch64-v2.6.4.zip"
        } else {
            "easytier-macos-x86_64-v2.6.4.zip"
        };
        assert_eq!(asset_name_for_version("2.6.4"), expected);
    }

    #[test]
    fn strips_only_one_leading_v() {
        assert_eq!(normalize_version(" v2.6.4 "), "2.6.4");
        assert_eq!(normalize_version("2.6.4"), "2.6.4");
        assert_eq!(normalize_version("vv2.6.4"), "v2.6.4");
    }

    #[test]
    fn prefixes_proxy_urls_for_github_only() {
        let urls = proxied_urls(
            "https://github.com/EasyTier/EasyTier/releases/download/v2.6.4/easytier.zip",
        );
        assert_eq!(urls.len(), GITHUB_PROXY_PREFIXES.len());
        assert!(urls[0].starts_with("https://github.com/"));
        assert!(urls[1].starts_with("https://gh-proxy.com/https://github.com/"));

        let plain = proxied_urls("https://example.com/easytier.zip");
        assert_eq!(plain.len(), 1);
    }

    #[test]
    fn rejects_zip_entries_that_escape_destination() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("bad.zip");

        let file = fs::File::create(&zip_path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        writer
            .start_file::<_, ()>("../escape", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"nope").unwrap();
        writer.finish().unwrap();

        let out = dir.path().join("out");
        assert!(
            unzip(&zip_path, &out).is_err(),
            "traversal entry was accepted"
        );
    }
}
