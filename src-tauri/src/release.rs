use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use walkdir::WalkDir;
use zip::ZipArchive;

use crate::error::{Error, Result};
use crate::paths::{
    default_config, GITHUB_PROXY_PREFIXES, OPTIONAL_LIBRARIES, RELEASE_API_URL, REQUIRED_BINARIES,
    REQUIRED_LIBRARIES,
};
use crate::settings;
use crate::util::copy_file;
#[cfg(target_os = "macos")]
use crate::util::set_mode;

/// Fail fast when a host is unreachable so the next mirror gets a turn.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// The release lookup is a small JSON response.
const API_TIMEOUT: Duration = Duration::from_secs(20);
/// Abort a transfer that stops producing data — the classic failure mode for a
/// throttled GitHub download, which would otherwise hang forever.
const READ_TIMEOUT: Duration = Duration::from_secs(30);
/// Upper bound for a multi-megabyte asset.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(60 * 30);
/// How often download progress is reported.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);

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
    /// Release notes. Present in the API payload, absent from the manifest
    /// shape the updater uses, hence the default. Only the Android update path
    /// reads them, so desktop treats them as spare.
    #[serde(default)]
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub body: Option<String>,
    #[serde(default)]
    pub published_at: Option<String>,
    /// Pre-releases are listed but marked, so the picker can warn about them.
    #[serde(default)]
    pub prerelease: bool,
    /// Drafts never have usable assets and are filtered out of the listing.
    #[serde(default)]
    pub draft: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
}

/// Resolve the newest EasyTier release and the asset matching this OS and arch.
///
/// The user's configured accelerator is tried first, then direct access, then
/// the built-in mirrors — the release API is often unreachable from mainland
/// networks.
/// Only the desktop builds fetch a core; Android links it in.
#[cfg(desktop)]
pub async fn latest_asset() -> Result<(GithubRelease, ReleaseAsset)> {
    let client = api_client()?;
    let mut last_error: Option<Error> = None;

    for url in api_urls(RELEASE_API_URL) {
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

/// The release listing, newest first. Forty entries covers roughly a year of
/// EasyTier releases, which is as far back as anyone reasonably rolls.
#[cfg(desktop)]
const RELEASE_LIST_API_URL: &str =
    "https://api.github.com/repos/EasyTier/EasyTier/releases?per_page=40";
#[cfg(desktop)]
const RELEASE_TAG_API_URL: &str = "https://api.github.com/repos/EasyTier/EasyTier/releases/tags/";

/// One selectable core version, flattened from GitHub's release payload.
///
/// The picker needs the tag (what actually gets installed), the version it
/// displays, the publication date, and whether this platform has an asset at
/// all: a release without one cannot be installed and must not be offered.
// No `#[cfg(desktop)]` here: Android still answers `list_core_releases`, just
// with an empty list, so the type has to exist on every platform.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CoreRelease {
    pub tag: String,
    pub version: String,
    pub published_at: String,
    pub prerelease: bool,
    pub asset_name: String,
}

/// Flatten one API entry, dropping anything this platform cannot install.
#[cfg(desktop)]
fn core_release_from(release: &GithubRelease) -> Option<CoreRelease> {
    if release.draft || release.tag_name.is_empty() {
        return None;
    }

    let version = normalize_version(&release.tag_name);
    let asset_name = asset_name_for_version(&version);
    if !release.assets.iter().any(|asset| asset.name == asset_name) {
        return None;
    }

    Some(CoreRelease {
        tag: release.tag_name.clone(),
        version,
        published_at: release.published_at.clone().unwrap_or_default(),
        prerelease: release.prerelease,
        asset_name,
    })
}

/// Every core version this platform can install, newest first.
///
/// Same mirror chain as the single-release lookup: the GitHub API is often
/// unreachable from mainland networks, so a failing mirror moves on.
#[cfg(desktop)]
pub async fn list_releases() -> Result<Vec<CoreRelease>> {
    let client = api_client()?;
    let mut last_error: Option<Error> = None;

    for url in api_urls(RELEASE_LIST_API_URL) {
        match fetch_release_list(&client, &url).await {
            Ok(releases) => {
                return Ok(releases.iter().filter_map(core_release_from).collect());
            }
            Err(err) => last_error = Some(err),
        }
    }

    Err(last_error.unwrap_or_else(|| Error::msg("github release list request failed")))
}

async fn fetch_release_list(
    client: &reqwest::Client,
    url: &str,
) -> Result<Vec<GithubRelease>> {
    let response = client
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "EasyTier-Manager")
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(Error::msg(format!(
            "github release list request failed: {}",
            response.status()
        )));
    }

    Ok(response.json::<Vec<GithubRelease>>().await?)
}

/// Resolve one specific tag — a version the user picked from the listing.
#[cfg(desktop)]
pub async fn release_by_tag(tag: &str) -> Result<(GithubRelease, ReleaseAsset)> {
    let tag = tag.trim();
    // The tag is interpolated into a URL, so anything outside this alphabet is
    // refused rather than escaped: release tags are known to be `v1.2.3`-shaped.
    let valid = !tag.is_empty()
        && tag
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
    if !valid {
        return Err(Error::msg(format!("invalid release tag: {tag}")));
    }

    let url = format!("{RELEASE_TAG_API_URL}{tag}");
    let client = api_client()?;
    let mut last_error: Option<Error> = None;

    for candidate in api_urls(&url) {
        let release = match fetch_release(&client, &candidate).await {
            Ok(release) => release,
            Err(err) => {
                last_error = Some(err);
                continue;
            }
        };

        let asset_name = asset_name_for_version(&normalize_version(&release.tag_name));
        return match release
            .assets
            .iter()
            .find(|asset| asset.name == asset_name)
            .cloned()
        {
            Some(asset) => Ok((release, asset)),
            None => Err(Error::msg(format!("release asset not found: {asset_name}"))),
        };
    }

    Err(last_error.unwrap_or_else(|| Error::msg("github release request failed")))
}

pub(crate) fn api_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(API_TIMEOUT)
        .build()?)
}

fn download_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .timeout(DOWNLOAD_TIMEOUT)
        .build()?)
}

pub(crate) async fn fetch_release(
    client: &reqwest::Client,
    url: &str,
) -> Result<GithubRelease> {
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

/// Download the release zip and unpack it into `<stage>/extract`, reporting
/// `(received, total)` bytes as it goes.
pub async fn download_and_extract(
    url: &str,
    stage: &Path,
    on_progress: impl Fn(u64, Option<u64>),
) -> Result<()> {
    let client = download_client()?;
    let zip_path = stage.join("easytier.zip");
    let extract_dir = stage.join("extract");
    let mut last_error: Option<Error> = None;

    for download_url in download_urls(url) {
        match fetch_bytes(&client, &download_url, &on_progress).await {
            Ok(bytes) => {
                let zip_path = zip_path.clone();
                return tokio::task::spawn_blocking(move || {
                    fs::write(&zip_path, &bytes)?;
                    unzip(&zip_path, &extract_dir)
                })
                .await?;
            }
            Err(err) => last_error = Some(err),
        }
    }

    Err(last_error.unwrap_or_else(|| Error::msg("download failed")))
}

async fn fetch_bytes(
    client: &reqwest::Client,
    url: &str,
    on_progress: &impl Fn(u64, Option<u64>),
) -> Result<Vec<u8>> {
    let mut response = client.get(url).send().await?;
    if !response.status().is_success() {
        return Err(Error::msg(format!(
            "download failed: {}",
            response.status()
        )));
    }

    let total = response.content_length();
    let mut body: Vec<u8> = Vec::with_capacity(total.unwrap_or(0) as usize);
    let mut received = 0u64;
    let mut last_report = std::time::Instant::now();
    on_progress(0, total);

    while let Some(chunk) = response.chunk().await? {
        received += chunk.len() as u64;
        body.extend_from_slice(&chunk);

        if last_report.elapsed() >= PROGRESS_INTERVAL {
            on_progress(received, total);
            last_report = std::time::Instant::now();
        }
    }

    on_progress(received, total.or(Some(received)));
    Ok(body)
}

/// Candidate URLs for downloading a release asset. The accelerators only proxy
/// GitHub URLs, so anything else is fetched directly.
pub fn download_urls(raw_url: &str) -> Vec<String> {
    if !raw_url.starts_with("https://github.com/") {
        return vec![raw_url.to_string()];
    }
    candidates(raw_url)
}

/// Candidate URLs for the release API — the accelerator is tried here too.
pub fn api_urls(raw_url: &str) -> Vec<String> {
    candidates(raw_url)
}

/// The user's accelerator first, then direct access, then the built-in mirrors,
/// with duplicates removed.
fn candidates(raw_url: &str) -> Vec<String> {
    let mut urls: Vec<String> = Vec::new();
    let mut push = |prefix: &str| {
        let candidate = format!("{prefix}{raw_url}");
        if !urls.contains(&candidate) {
            urls.push(candidate);
        }
    };

    push(&settings::github_proxy());
    for prefix in GITHUB_PROXY_PREFIXES {
        push(prefix);
    }
    urls
}

/// Extract a zip, rejecting entries that would escape `dest`.
pub fn unzip(zip_path: &Path, dest: &Path) -> Result<()> {
    use std::io::Write;

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

        // Unix permission bits are meaningless on Windows; the ACLs of the
        // parent directory govern access there.
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&target, fs::Permissions::from_mode(mode));
        }
    }

    Ok(())
}

/// Collect the binaries and runtime libraries the service needs into
/// `<stage>/bin`.
pub fn stage_binaries(stage: &Path) -> Result<()> {
    let extract_root = stage.join("extract");
    let bin_stage = stage.join("bin");
    fs::create_dir_all(&bin_stage)?;

    for binary in REQUIRED_BINARIES {
        let src = find_file(&extract_root, binary)?;
        copy_file(&src, &bin_stage.join(binary), 0o755)?;
    }
    for library in REQUIRED_LIBRARIES {
        let src = find_file(&extract_root, library)?;
        copy_file(&src, &bin_stage.join(library), 0o755)?;
    }
    for library in OPTIONAL_LIBRARIES {
        if let Some(src) = find_file_optional(&extract_root, library) {
            copy_file(&src, &bin_stage.join(library), 0o755)?;
        }
    }
    Ok(())
}

/// Stage everything a first-time install has to copy into place: binaries plus
/// the generated config templates (and, on macOS, the service wrapper script).
pub fn stage_install_files(stage: &Path, tag: &str) -> Result<()> {
    stage_binaries(stage)?;

    let files = [
        ("default.conf", default_config()),
        ("service.env", String::from("EASYTIER_CONFIG_SERVER=\"\"\n")),
        ("VERSION", format!("{}\n", normalize_version(tag))),
    ];
    for (name, contents) in files {
        fs::write(stage.join(name), contents)?;
    }

    // macOS runs the core through a shell wrapper that owns logging and pid
    // tracking; Windows registers the core binary directly.
    #[cfg(target_os = "macos")]
    {
        let wrapper = stage.join("easytier-service");
        fs::write(&wrapper, crate::paths::service_wrapper())?;
        set_mode(&wrapper, 0o755)?;
    }

    Ok(())
}

pub fn find_file(root: &Path, name: &str) -> Result<PathBuf> {
    find_file_optional(root, name)
        .ok_or_else(|| Error::msg(format!("required binary not found: {name}")))
}

/// Locate a file anywhere under `root`, treating a missing file (or an
/// unreadable directory tree) as absent rather than an error.
fn find_file_optional(root: &Path, name: &str) -> Option<PathBuf> {
    for entry in WalkDir::new(root) {
        let Ok(entry) = entry else {
            continue;
        };
        if !entry.file_type().is_dir() && entry.file_name() == name {
            return Some(entry.into_path());
        }
    }
    None
}

/// EasyTier publishes one combined CLI zip per OS/arch, e.g.
/// `easytier-windows-x86_64-v2.6.4.zip` or `easytier-macos-aarch64-v2.6.4.zip`.
/// Android links the core into the APK instead of downloading it, so there is no
/// release asset to name there.
#[cfg(desktop)]
pub fn asset_name_for_version(version: &str) -> String {
    format!("easytier-{}-v{version}.zip", platform_asset_tag())
}

#[cfg(target_os = "macos")]
fn platform_asset_tag() -> &'static str {
    if std::env::consts::ARCH == "aarch64" {
        "macos-aarch64"
    } else {
        "macos-x86_64"
    }
}

#[cfg(windows)]
fn platform_asset_tag() -> &'static str {
    match std::env::consts::ARCH {
        // EasyTier names the 32-bit build `i686` and the ARM64 build `arm64`.
        "aarch64" => "windows-arm64",
        "x86" => "windows-i686",
        _ => "windows-x86_64",
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

    #[cfg(target_os = "macos")]
    #[test]
    fn builds_macos_asset_name_for_arch() {
        let expected = if std::env::consts::ARCH == "aarch64" {
            "easytier-macos-aarch64-v2.6.4.zip"
        } else {
            "easytier-macos-x86_64-v2.6.4.zip"
        };
        assert_eq!(asset_name_for_version("2.6.4"), expected);
    }

    #[cfg(windows)]
    #[test]
    fn builds_windows_asset_name_for_arch() {
        let expected = match std::env::consts::ARCH {
            "aarch64" => "easytier-windows-arm64-v2.6.4.zip",
            "x86" => "easytier-windows-i686-v2.6.4.zip",
            _ => "easytier-windows-x86_64-v2.6.4.zip",
        };
        assert_eq!(asset_name_for_version("2.6.4"), expected);
    }

    #[test]
    fn strips_only_one_leading_v() {
        assert_eq!(normalize_version(" v2.6.4 "), "2.6.4");
        assert_eq!(normalize_version("2.6.4"), "2.6.4");
        assert_eq!(normalize_version("vv2.6.4"), "v2.6.4");
    }

    /// The picker must never offer a release this platform cannot install:
    /// assets are per-OS/arch, and a draft has none at all.
    #[cfg(desktop)]
    #[test]
    fn listing_keeps_only_installable_releases() {
        let asset = asset_name_for_version("2.7.0");
        let release = |tag: &str, names: &[&str], draft: bool, prerelease: bool| GithubRelease {
            tag_name: tag.to_string(),
            assets: names
                .iter()
                .map(|name| ReleaseAsset {
                    name: name.to_string(),
                    browser_download_url: String::new(),
                })
                .collect(),
            body: None,
            published_at: Some("2026-10-06T13:21:27Z".to_string()),
            prerelease,
            draft,
        };

        let installable = core_release_from(&release("v2.7.0", &[&asset], false, false))
            .expect("matching asset is offered");
        assert_eq!(installable.version, "2.7.0");
        assert_eq!(installable.tag, "v2.7.0");
        assert_eq!(installable.asset_name, asset);
        assert_eq!(installable.published_at, "2026-10-06T13:21:27Z");
        assert!(!installable.prerelease);

        // Some other platform's zip is not ours to install.
        assert!(core_release_from(&release(
            "v2.7.0",
            &["easytier-ohos-arm64-v2.7.0.zip"],
            false,
            false
        ))
        .is_none());
        // Drafts are never listed.
        assert!(core_release_from(&release("v9.9.9", &[&asset], true, false)).is_none());

        // Pre-releases stay listed, marked, so the UI can warn instead of hide.
        let nightly_asset = asset_name_for_version("nightly");
        let nightly = core_release_from(&release("nightly", &[&nightly_asset], false, true))
            .expect("pre-release is listed");
        assert!(nightly.prerelease);
        assert_eq!(nightly.version, "nightly");
    }

    #[test]
    fn tries_an_accelerator_before_direct_access() {
        let url = "https://github.com/EasyTier/EasyTier/releases/download/v2.6.4/easytier.zip";
        let urls = download_urls(url);

        // Whatever accelerator is configured it leads, direct access follows, and
        // the built-in mirror is kept as a last resort. These hold for any
        // configured value, so the test does not race the settings global.
        assert_ne!(
            urls[0], url,
            "the accelerator must be tried before direct access"
        );
        assert_eq!(urls[1], url, "direct access must come second");
        assert!(urls
            .iter()
            .any(|candidate| candidate.starts_with("https://ghproxy.net/")));

        let mut unique = urls.clone();
        unique.dedup();
        assert_eq!(unique.len(), urls.len(), "candidates must be unique");
    }

    #[test]
    fn leaves_non_github_urls_alone() {
        assert_eq!(
            download_urls("https://example.com/easytier.zip"),
            vec!["https://example.com/easytier.zip".to_string()]
        );
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
