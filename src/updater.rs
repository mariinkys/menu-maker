//! Self-update from GitHub Releases (used on Windows).
//!
//!
//! 1. [`check`]: ask the GitHub API for the latest release of the repository
//!    in `Cargo.toml` and compare its tag (`v1.2.3`) with this build's version.
//! 2. [`download_and_verify`]: stream the release asset for this platform
//!    (`menu-maker-windows-x64.exe`, optionally with the version in the name)
//!    to the data folder and check its SHA-256. The expected hash is the
//!    `digest` GitHub computes for every uploaded asset (since June 2025), or a
//!    `<asset>.sha256` file if the release has one. No CI changes are needed.
//! 3. [`install`]: replace the running executable *under the same name*
//!    (`self-replace` renames the running file aside, which Windows allows,
//!    and deletes it once the process exits), then the caller relaunches it.
//!
//! The checksum protects against corrupted or truncated downloads. It does not
//! protect against someone who controls the GitHub account: for that, sign the
//! executable (Authenticode) and verify the signature.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, bail};
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tracing::{debug, info, warn};

use crate::paths::{self, APP_NAME};

/// Refuse downloads larger than this (the exe is ~30 MB).
const MAX_DOWNLOAD_BYTES: u64 = 300 * 1024 * 1024;

/// What we know about this build and where its releases live.
#[derive(Debug, Clone)]
pub struct UpdateSource {
    /// `owner/repo` on GitHub.
    pub repo: String,
    pub current: Version,
    /// Platform part of the asset name, e.g. `windows-x64.exe`.
    pub platform: String,
}

impl UpdateSource {
    /// Builds the source from `Cargo.toml` (`repository`, `version`) and the
    /// running OS/CPU. Returns `None` if the repository is not on GitHub.
    pub fn from_package() -> Option<Self> {
        Some(Self {
            repo: github_repo(env!("CARGO_PKG_REPOSITORY"))?,
            current: Version::parse(env!("CARGO_PKG_VERSION")).ok()?,
            platform: platform_suffix(std::env::consts::OS, std::env::consts::ARCH),
        })
    }
}

/// A newer release that can be installed.
#[derive(Debug, Clone, PartialEq)]
pub struct AvailableUpdate {
    pub version: Version,
    /// Release notes (GitHub Markdown, shown as plain text).
    pub notes: String,
    /// Release page, for users who prefer to download manually.
    pub page_url: String,
    pub asset_name: String,
    pub asset_url: String,
    pub asset_size: u64,
    /// Expected SHA-256 (lowercase hex) from GitHub's asset digest.
    pub sha256: Option<String>,
    /// Fallback: a `<asset>.sha256` file attached to the release.
    pub checksum_url: Option<String>,
}

impl AvailableUpdate {
    /// Without a checksum we never install automatically.
    pub fn can_verify(&self) -> bool {
        self.sha256.is_some() || self.checksum_url.is_some()
    }
}

/// Platform part of the release asset name, matching the CI naming:
/// `windows-x64.exe`, `windows-arm64.exe`, `linux-x64`…
pub fn platform_suffix(os: &str, arch: &str) -> String {
    let arch = match arch {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    };
    let ext = if os == "windows" { ".exe" } else { "" };
    format!("{os}-{arch}{ext}")
}

/// Whether a release asset is this app's binary for `platform`. Accepts
/// `menu-maker-windows-x64.exe` and versioned names such as
/// `menu-maker-v0.2.0-windows-x64.exe` or `menu-maker-0.2.0-windows-x64.exe`.
pub fn is_platform_asset(name: &str, platform: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.starts_with(&format!("{APP_NAME}-"))
        && name.ends_with(&format!("-{}", platform.to_ascii_lowercase()))
}

/// `https://github.com/owner/repo(.git)(/)` → `owner/repo`.
pub fn github_repo(url: &str) -> Option<String> {
    let rest = url.trim().strip_prefix("https://github.com/")?;
    let mut parts = rest
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .split('/');
    let (owner, repo) = (parts.next()?, parts.next()?);
    (!owner.is_empty() && !repo.is_empty() && parts.next().is_none())
        .then(|| format!("{owner}/{repo}"))
}

// ---------------------------------------------------------------------------
// 1. Check
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    size: u64,
    /// `sha256:<hex>`, computed by GitHub for assets uploaded since June 2025.
    #[serde(default)]
    digest: Option<String>,
}

fn agent() -> ureq::Agent {
    use ureq::tls::{RootCerts, TlsConfig};
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(60)))
        // Trust the OS certificate store (on Windows, the one Edge/Chrome use)
        // instead of a built-in list, so antivirus HTTPS scanning and corporate
        // proxies with their own root certificate keep working.
        .tls_config(
            TlsConfig::builder()
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        )
        .build()
        .into()
}

fn user_agent() -> String {
    // GitHub rejects API requests without a User-Agent.
    format!("{APP_NAME}/{}", env!("CARGO_PKG_VERSION"))
}

/// Returns the latest release if it is newer than the running version.
pub fn check(source: &UpdateSource) -> anyhow::Result<Option<AvailableUpdate>> {
    let url = format!(
        "https://api.github.com/repos/{}/releases/latest",
        source.repo
    );
    debug!(%url, "checking for updates");
    let release: Release = agent()
        .get(&url)
        .header("User-Agent", &user_agent())
        .header("Accept", "application/vnd.github+json")
        .call()
        .with_context(|| format!("GET {url}"))?
        .body_mut()
        .read_json()
        .context("unexpected response from GitHub")?;
    let update = evaluate(source, release)?;
    match &update {
        Some(u) => info!(current = %source.current, latest = %u.version, "update available"),
        None => info!(current = %source.current, "no update available"),
    }
    Ok(update)
}

/// Pure decision logic, separated from the HTTP call so it can be tested.
fn evaluate(source: &UpdateSource, release: Release) -> anyhow::Result<Option<AvailableUpdate>> {
    if release.draft || release.prerelease {
        return Ok(None);
    }
    let tag = release.tag_name.trim().trim_start_matches(['v', 'V']);
    let version = Version::parse(tag)
        .with_context(|| format!("release tag `{}` is not a version", release.tag_name))?;
    if version <= source.current {
        return Ok(None);
    }
    let Some(asset) = release
        .assets
        .iter()
        .find(|a| is_platform_asset(&a.name, &source.platform))
    else {
        warn!(platform = %source.platform, %version, "newer release has no asset for this platform");
        return Ok(None);
    };
    let checksum_name = format!("{}.sha256", asset.name);
    let sha256 = asset
        .digest
        .as_deref()
        .and_then(|d| d.strip_prefix("sha256:"))
        .and_then(|hex| parse_checksum(hex).ok());
    Ok(Some(AvailableUpdate {
        version,
        notes: release.body.unwrap_or_default().trim().to_string(),
        page_url: release.html_url,
        asset_name: asset.name.clone(),
        asset_url: asset.browser_download_url.clone(),
        asset_size: asset.size,
        sha256,
        checksum_url: release
            .assets
            .iter()
            .find(|a| a.name == checksum_name)
            .map(|a| a.browser_download_url.clone()),
    }))
}

// ---------------------------------------------------------------------------
// 2. Download + verify
// ---------------------------------------------------------------------------

/// Whether the running executable can replace itself. `self-replace` needs to
/// create and rename files next to the exe, which fails e.g. under
/// `C:\Program Files` without admin rights.
pub fn can_self_replace() -> bool {
    let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
    else {
        return false;
    };
    let probe = dir.join(format!(".{APP_NAME}-write-test"));
    let ok = File::create(&probe)
        .and_then(|mut f| f.write_all(b"ok"))
        .is_ok();
    let _ = std::fs::remove_file(&probe);
    debug!(dir = %dir.display(), writable = ok, "self-replace possible?");
    ok
}

/// Downloads the new executable to `<data dir>/updates/`, reporting
/// `(downloaded, total)` bytes, and verifies its SHA-256.
pub fn download_and_verify(
    update: &AvailableUpdate,
    mut progress: impl FnMut(u64, u64),
) -> anyhow::Result<PathBuf> {
    let expected = match (&update.sha256, &update.checksum_url) {
        (Some(digest), _) => digest.clone(),
        (None, Some(url)) => parse_checksum(&download_text(url)?)?,
        (None, None) => bail!(
            "the release asset has no SHA-256 digest; refusing to install without verification"
        ),
    };

    let dir = paths::data_dir().join("updates");
    std::fs::create_dir_all(&dir).with_context(|| format!("could not create {}", dir.display()))?;
    let target = dir.join(format!("{APP_NAME}-{}.exe", update.version));
    info!(url = %update.asset_url, to = %target.display(), "downloading update");

    let response = agent()
        .get(&update.asset_url)
        .header("User-Agent", &user_agent())
        .call()
        .with_context(|| format!("GET {}", update.asset_url))?;
    let mut reader = response
        .into_body()
        .into_with_config()
        .limit(MAX_DOWNLOAD_BYTES)
        .reader();
    let mut file =
        File::create(&target).with_context(|| format!("could not create {}", target.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut downloaded = 0u64;
    loop {
        let n = reader.read(&mut buffer).context("download interrupted")?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
        file.write_all(&buffer[..n])?;
        downloaded += n as u64;
        progress(downloaded, update.asset_size.max(downloaded));
    }
    file.sync_all()?;
    drop(file);

    let actual = hex(&hasher.finalize());
    if actual != expected {
        let _ = std::fs::remove_file(&target);
        bail!("checksum mismatch: expected {expected}, got {actual}");
    }
    info!(bytes = downloaded, sha256 = %actual, "update downloaded and verified");
    Ok(target)
}

fn download_text(url: &str) -> anyhow::Result<String> {
    let mut response = agent()
        .get(url)
        .header("User-Agent", &user_agent())
        .call()
        .with_context(|| format!("GET {url}"))?;
    Ok(response
        .body_mut()
        .with_config()
        .limit(64 * 1024)
        .read_to_string()?)
}

/// Accepts `sha256sum` output (`<hex>  file.exe`) or a bare hex digest,
/// as produced by `sha256sum` or PowerShell's `Get-FileHash`.
pub fn parse_checksum(text: &str) -> anyhow::Result<String> {
    let digest = text
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if digest.len() == 64 && digest.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(digest)
    } else {
        bail!("invalid .sha256 file")
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ---------------------------------------------------------------------------
// 3. Install
// ---------------------------------------------------------------------------

/// Replaces the running executable with `new_exe`, keeping its path and name.
/// Returns that path so the caller can relaunch it. The old file is removed
/// automatically after this process exits.
pub fn install(new_exe: &Path) -> anyhow::Result<PathBuf> {
    // Capture the path *before* replacing: afterwards the running image has
    // been renamed to a temporary name.
    let exe = std::env::current_exe().context("cannot locate the running executable")?;
    self_replace::self_replace(new_exe).context("could not replace the executable")?;
    let _ = std::fs::remove_file(new_exe);
    info!(exe = %exe.display(), "executable replaced");
    Ok(exe)
}

/// Starts a fresh instance of the (updated) executable.
pub fn relaunch(exe: &Path) -> anyhow::Result<()> {
    std::process::Command::new(exe)
        .spawn()
        .with_context(|| format!("could not start {}", exe.display()))?;
    info!(exe = %exe.display(), "relaunched updated executable");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXE: &str = "menu-maker-windows-x64.exe";
    const HEX: &str = "2151b604e3429bff440b9fbc03eb3617bc2603cda96c95b9bb05277f9ddba255";

    fn source() -> UpdateSource {
        UpdateSource {
            repo: "mariinkys/menu-maker".into(),
            current: Version::parse("0.1.0").unwrap(),
            platform: platform_suffix("windows", "x86_64"),
        }
    }

    fn asset(name: &str, digest: Option<&str>) -> Asset {
        Asset {
            name: name.into(),
            browser_download_url: format!("https://example.com/{name}"),
            size: 10,
            digest: digest.map(str::to_string),
        }
    }

    fn release(tag: &str, assets: Vec<Asset>) -> Release {
        Release {
            tag_name: tag.into(),
            html_url: "https://github.com/mariinkys/menu-maker/releases/tag/v0.2.0".into(),
            body: Some("## Novedades".into()),
            draft: false,
            prerelease: false,
            assets,
        }
    }

    #[test]
    fn names_and_repo() {
        assert_eq!(platform_suffix("windows", "x86_64"), "windows-x64.exe");
        assert_eq!(platform_suffix("linux", "aarch64"), "linux-arm64");
        // Current CI names and versioned variants all match…
        for name in [
            EXE,
            "menu-maker-v0.2.0-windows-x64.exe",
            "Menu-Maker-0.2.0-Windows-x64.EXE",
        ] {
            assert!(is_platform_asset(name, "windows-x64.exe"), "{name}");
        }
        // …but not other platforms or packages.
        for name in [
            "menu-maker-linux-x64.deb",
            "menu-maker-linux-x64.AppImage",
            "other-windows-x64.exe",
            "menu-maker-windows-arm64.exe",
        ] {
            assert!(!is_platform_asset(name, "windows-x64.exe"), "{name}");
        }
        assert_eq!(
            github_repo("https://github.com/mariinkys/menu-maker").as_deref(),
            Some("mariinkys/menu-maker")
        );
        assert_eq!(
            github_repo("https://github.com/a/b.git/").as_deref(),
            Some("a/b")
        );
        assert_eq!(github_repo("https://gitlab.com/a/b"), None);
        assert_eq!(github_repo("https://github.com/a/b/tree/main"), None);
        assert!(
            UpdateSource::from_package().is_some(),
            "Cargo.toml repository must be on GitHub"
        );
    }

    #[test]
    fn github_digest_is_used_for_verification() {
        let assets = vec![
            asset("menu-maker-linux-x64.deb", None),
            asset(EXE, Some(&format!("sha256:{HEX}"))),
        ];
        let u = evaluate(&source(), release("v0.2.0", assets))
            .unwrap()
            .unwrap();
        assert_eq!(u.version, Version::parse("0.2.0").unwrap());
        assert_eq!(u.asset_name, EXE);
        assert_eq!(u.sha256.as_deref(), Some(HEX));
        assert!(u.can_verify());
    }

    #[test]
    fn sha256_file_is_a_fallback_and_no_checksum_blocks_install() {
        let u = evaluate(
            &source(),
            release(
                "0.2.0",
                vec![asset(EXE, None), asset(&format!("{EXE}.sha256"), None)],
            ),
        )
        .unwrap()
        .unwrap();
        assert!(u.sha256.is_none() && u.checksum_url.is_some() && u.can_verify());
        let u = evaluate(&source(), release("v0.2.0", vec![asset(EXE, None)]))
            .unwrap()
            .unwrap();
        assert!(!u.can_verify());
    }

    #[test]
    fn same_older_draft_prerelease_or_missing_asset_are_ignored() {
        let exe = || vec![asset(EXE, None)];
        assert_eq!(evaluate(&source(), release("v0.1.0", exe())).unwrap(), None);
        assert_eq!(evaluate(&source(), release("0.0.9", exe())).unwrap(), None);
        assert_eq!(
            evaluate(
                &source(),
                release("v0.2.0", vec![asset("menu-maker-linux-x64.AppImage", None)])
            )
            .unwrap(),
            None
        );
        let mut pre = release("v0.2.0", exe());
        pre.prerelease = true;
        assert_eq!(evaluate(&source(), pre).unwrap(), None);
        assert!(evaluate(&source(), release("latest", exe())).is_err());
    }

    #[test]
    fn checksum_formats() {
        let upper = "A".repeat(64);
        assert_eq!(
            parse_checksum(&format!("{upper}  {EXE}\n")).unwrap(),
            "a".repeat(64)
        );
        assert_eq!(parse_checksum(&upper).unwrap(), "a".repeat(64));
        assert!(parse_checksum("not a hash").is_err());
        assert_eq!(
            hex(&Sha256::digest(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
