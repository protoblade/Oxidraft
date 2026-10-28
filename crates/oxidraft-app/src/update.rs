//! Checking GitHub for a newer release and installing it in place.
//!
//! Releases are built by `.github/workflows/release.yml`, which publishes
//! `oxidraft-<version>-<target>.tar.gz` plus a `.sha256` file per platform.
//! Network work runs on a background thread; the UI only reads [`UpdateState`].

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui;
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// The GitHub repository releases come from.
pub const REPO: &str = "protoblade/Oxidraft";
/// Target triple this binary was built for (set by build.rs).
pub const TARGET: &str = env!("OXIDRAFT_TARGET");
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Set this environment variable to skip the automatic check at startup.
pub const DISABLE_ENV: &str = "OXIDRAFT_NO_UPDATE_CHECK";

const BIN_NAME: &str = if cfg!(windows) { "oxidraft.exe" } else { "oxidraft" };
const MAX_DOWNLOAD: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Release {
    pub tag_name: String,
    pub html_url: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
}

impl Release {
    pub fn version(&self) -> Option<Version> {
        Version::parse(self.tag_name.trim_start_matches('v')).ok()
    }

    /// True if this release is newer than `current`.
    pub fn is_newer_than(&self, current: &str) -> bool {
        match (self.version(), Version::parse(current)) {
            (Some(v), Ok(c)) => v > c,
            _ => false,
        }
    }

    fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }
}

/// The archive name the release workflow uses for this platform.
pub fn asset_name(version: &Version) -> String {
    format!("oxidraft-{version}-{TARGET}.tar.gz")
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(120))).build().into()
}

fn get(url: &str) -> Result<ureq::http::Response<ureq::Body>, ureq::Error> {
    agent()
        .get(url)
        .header("User-Agent", format!("oxidraft/{CURRENT_VERSION}"))
        .header("Accept", "application/vnd.github+json")
        .call()
}

/// The latest published release, or `None` if the repository has none yet
/// (or is private: GitHub then answers 404 to unauthenticated requests).
pub fn fetch_latest() -> Result<Option<Release>, String> {
    fetch_latest_from(REPO)
}

fn fetch_latest_from(repo: &str) -> Result<Option<Release>, String> {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    match get(&url) {
        Ok(mut response) => response.body_mut().read_json().map(Some).map_err(|e| format!("Unreadable reply: {e}")),
        Err(ureq::Error::StatusCode(404)) => Ok(None),
        Err(e) => Err(format!("Could not reach GitHub: {e}")),
    }
}

fn download(url: &str) -> Result<Vec<u8>, String> {
    let mut response = get(url).map_err(|e| format!("Download failed: {e}"))?;
    response.body_mut().with_config().limit(MAX_DOWNLOAD).read_to_vec().map_err(|e| format!("Download failed: {e}"))
}

/// Checks `bytes` against a `sha256sum`-style line (`<hex>  <file name>`).
pub fn verify_sha256(bytes: &[u8], checksum_file: &str) -> Result<(), String> {
    let expected = checksum_file.split_whitespace().next().unwrap_or_default().to_ascii_lowercase();
    let actual: String = Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect();
    if expected.len() == 64 && expected == actual {
        Ok(())
    } else {
        Err("The download is damaged (checksum mismatch).".into())
    }
}

/// Unpacks the app binary from a release archive into `dir` and returns its path.
pub fn extract_binary(archive: &[u8], dir: &Path) -> Result<PathBuf, String> {
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive));
    let entries = tar.entries().map_err(|e| format!("Unreadable archive: {e}"))?;
    for entry in entries {
        let mut entry = entry.map_err(|e| format!("Unreadable archive: {e}"))?;
        let is_binary = entry.path().ok().and_then(|p| p.file_name().map(|n| n == BIN_NAME)).unwrap_or(false);
        if !is_binary {
            continue;
        }
        let out = dir.join(BIN_NAME);
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).map_err(|e| format!("Unreadable archive: {e}"))?;
        std::fs::write(&out, bytes).map_err(|e| format!("Could not write the update: {e}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| format!("Could not write the update: {e}"))?;
        }
        return Ok(out);
    }
    Err(format!("The archive has no {BIN_NAME}."))
}

/// Downloads, verifies and installs `release` over the running executable.
pub fn install(release: &Release) -> Result<Version, String> {
    let version = release.version().ok_or("The release has no valid version number.")?;
    let name = asset_name(&version);
    let archive =
        release.asset(&name).ok_or(format!("Release {version} has no download for this platform ({TARGET})."))?;
    let checksum = release.asset(&format!("{name}.sha256")).ok_or("The release has no checksum file.")?;

    let bytes = download(&archive.browser_download_url)?;
    let sums = download(&checksum.browser_download_url)?;
    verify_sha256(&bytes, &String::from_utf8_lossy(&sums))?;

    let dir = std::env::temp_dir().join(format!("oxidraft-update-{version}"));
    std::fs::create_dir_all(&dir).map_err(|e| format!("Could not write the update: {e}"))?;
    let new_exe = extract_binary(&bytes, &dir)?;
    let replaced = self_replace::self_replace(&new_exe).map_err(|e| format!("Could not replace the app: {e}"));
    let _ = std::fs::remove_dir_all(&dir);
    replaced.map(|()| version)
}

/// Starts the (now updated) executable again with the same arguments.
pub fn restart() -> std::io::Result<()> {
    std::process::Command::new(std::env::current_exe()?).args(std::env::args_os().skip(1)).spawn().map(|_| ())
}

/// True when running from a Cargo `target` directory: replacing that binary
/// would fight with the next `cargo build`, so we point to `git pull` instead.
pub fn is_dev_build() -> bool {
    std::env::current_exe()
        .is_ok_and(|exe| exe.components().any(|c| matches!(c.as_os_str().to_str(), Some("target" | "target.nosync"))))
}

#[derive(Debug, Clone, PartialEq)]
pub enum UpdateState {
    Idle,
    Checking {
        manual: bool,
    },
    UpToDate,
    Available(Release),
    Installing(Release),
    /// Installed; takes effect after a restart.
    Installed(Version),
    Failed(String),
}

/// Runs update checks and installs in the background.
#[derive(Clone)]
pub struct Updater {
    state: Arc<Mutex<UpdateState>>,
}

impl Default for Updater {
    fn default() -> Self {
        Self { state: Arc::new(Mutex::new(UpdateState::Idle)) }
    }
}

impl Updater {
    pub fn state(&self) -> UpdateState {
        self.state.lock().map(|s| s.clone()).unwrap_or(UpdateState::Idle)
    }

    pub fn set(&self, state: UpdateState) {
        if let Ok(mut s) = self.state.lock() {
            *s = state;
        }
    }

    /// Looks for a newer release. A `manual` check reports "up to date" and
    /// errors; the automatic one at startup stays quiet unless there's an update.
    pub fn check(&self, ctx: &egui::Context, manual: bool) {
        if matches!(self.state(), UpdateState::Checking { .. } | UpdateState::Installing(_)) {
            return;
        }
        self.set(UpdateState::Checking { manual });
        let (this, ctx) = (self.clone(), ctx.clone());
        std::thread::spawn(move || {
            let next = match fetch_latest() {
                Ok(Some(r)) if r.is_newer_than(CURRENT_VERSION) => UpdateState::Available(r),
                Ok(_) if manual => UpdateState::UpToDate,
                Err(e) if manual => UpdateState::Failed(e),
                _ => UpdateState::Idle,
            };
            this.set(next);
            ctx.request_repaint();
        });
    }

    pub fn install(&self, ctx: &egui::Context, release: Release) {
        self.set(UpdateState::Installing(release.clone()));
        let (this, ctx) = (self.clone(), ctx.clone());
        std::thread::spawn(move || {
            this.set(match install(&release) {
                Ok(v) => UpdateState::Installed(v),
                Err(e) => UpdateState::Failed(e),
            });
            ctx.request_repaint();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str) -> Release {
        Release { tag_name: tag.into(), html_url: String::new(), body: None, assets: Vec::new() }
    }

    /// Talks to GitHub: `cargo test -p oxidraft-app -- --ignored`.
    #[test]
    #[ignore = "needs network access"]
    fn fetches_a_real_release() {
        let release = fetch_latest_from("emilk/egui").unwrap().expect("egui has releases");
        assert!(release.version().is_some(), "{}", release.tag_name);
        assert!(!release.assets.is_empty() || release.body.is_some());
        assert_eq!(fetch_latest_from("protoblade/this-repo-does-not-exist-x9"), Ok(None));
    }

    #[test]
    fn compares_versions() {
        assert!(release("v99.0.0").is_newer_than("0.1.0"));
        assert!(!release("v0.1.0").is_newer_than("0.1.0"));
        assert!(!release("v0.0.9").is_newer_than("0.1.0"));
        assert!(!release("nightly").is_newer_than("0.1.0"));
    }

    #[test]
    fn parses_github_release_json() {
        let json = r#"{"tag_name":"v0.2.0","html_url":"https://github.com/x","body":"notes","assets":[
            {"name":"oxidraft-0.2.0-aarch64-apple-darwin.tar.gz","browser_download_url":"https://dl/a","size":1}]}"#;
        let r: Release = serde_json::from_str(json).unwrap();
        assert_eq!(r.version(), Some(Version::new(0, 2, 0)));
        assert_eq!(r.assets[0].browser_download_url, "https://dl/a");
    }

    #[test]
    fn asset_name_matches_the_release_workflow() {
        assert_eq!(asset_name(&Version::new(1, 2, 3)), format!("oxidraft-1.2.3-{TARGET}.tar.gz"));
    }

    #[test]
    fn verifies_checksums() {
        // sha256("abc")
        let sum = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  file.tar.gz\n";
        assert!(verify_sha256(b"abc", sum).is_ok());
        assert!(verify_sha256(b"abd", sum).is_err());
        assert!(verify_sha256(b"abc", "").is_err());
    }

    #[test]
    fn extracts_the_binary_from_an_archive() {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
        for (name, data) in [("README.md", &b"readme"[..]), (BIN_NAME, &b"new binary"[..])] {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder.append_data(&mut header, format!("./{name}"), data).unwrap();
        }
        let archive = builder.into_inner().unwrap().finish().unwrap();

        let dir = std::env::temp_dir().join(format!("oxidraft-update-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = extract_binary(&archive, &dir).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new binary");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
