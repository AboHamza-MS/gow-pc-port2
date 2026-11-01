//! Self-update of the launcher: which GitHub release is the newest one worth offering, how this copy of the launcher can
//! be updated, when to ask again and what blocks an update. The download, the signature check and the installer run in the
//! Tauri shell (`tauri-plugin-updater`); everything that can be decided without it lives here, so `cargo test` covers it.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

use base64::Engine;
use semver::Version;
use serde::{Deserialize, Serialize};

pub const REPOSITORY: &str = "danielalves96/gow-pc-port";
/// Release tags of the launcher (`launcher-v0.2.0`); anything else in the repository is not an update of this program.
pub const TAG_PREFIX: &str = "launcher-v";
/// The manifest `tauri-action` attaches to a release built with the signing key (`createUpdaterArtifacts`).
pub const MANIFEST_ASSET: &str = "latest.json";
/// Placeholder of `plugins.updater.pubkey` in `tauri.conf.json` until the maintainer generates the key pair.
pub const PUBKEY_PLACEHOLDER: &str = "REPLACE_WITH_THE_UPDATER_PUBLIC_KEY";

/// Time between two checks while the launcher stays open, and the wait after one that failed (offline, rate limit).
pub const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 3600);
pub const RETRY_INTERVAL: Duration = Duration::from_secs(30 * 60);

pub fn releases_url() -> String {
    format!("https://api.github.com/repos/{REPOSITORY}/releases?per_page=30")
}

// ---------------------------------------------------------------------------------------------- releases

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct GithubAsset {
    pub name: String,
    pub browser_download_url: String,
}

/// The fields of a GitHub release this module reads.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct GithubRelease {
    pub tag_name: String,
    pub draft: bool,
    pub prerelease: bool,
    pub html_url: String,
    pub assets: Vec<GithubAsset>,
}

/// A published launcher release.
#[derive(Debug, Clone, PartialEq)]
pub struct ReleaseInfo {
    pub version: Version,
    pub page_url: String,
    /// Where the signed update manifest is, when the release has one.
    pub manifest_url: Option<String>,
}

/// The version a launcher tag names: `launcher-v0.2.0` gives 0.2.0. Pre-release versions (`0.3.0-beta.1`) are rejected: a
/// stable launcher must never be offered one, even when the release was not flagged as a pre-release.
pub fn tag_version(tag: &str) -> Option<Version> {
    let version = Version::parse(tag.strip_prefix(TAG_PREFIX)?).ok()?;
    version.pre.is_empty().then_some(version)
}

/// The highest published, stable launcher release of a GitHub listing (drafts, pre-releases and other tags are ignored).
pub fn newest_release(releases: &[GithubRelease]) -> Option<ReleaseInfo> {
    releases
        .iter()
        .filter(|release| !release.draft && !release.prerelease)
        .filter_map(|release| {
            let manifest = release.assets.iter().find(|asset| asset.name == MANIFEST_ASSET);
            Some(ReleaseInfo {
                version: tag_version(&release.tag_name)?,
                page_url: release.html_url.clone(),
                manifest_url: manifest.map(|asset| asset.browser_download_url.clone()),
            })
        })
        .max_by(|a, b| a.version.cmp(&b.version))
}

/// True when `candidate` is a higher version than the running launcher. An unreadable running version offers nothing.
pub fn is_newer(current: &str, candidate: &Version) -> bool {
    Version::parse(current).is_ok_and(|current| *candidate > current)
}

/// Asks GitHub for the releases (a blocking request: call it off the UI thread). `url` is a parameter for the tests.
pub fn fetch_releases(url: &str) -> Result<Vec<GithubRelease>, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .user_agent(concat!("gow-launcher/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let mut response = agent
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|error| format!("the release list could not be fetched: {error}"))?;
    let body =
        response.body_mut().read_to_string().map_err(|error| format!("the release list could not be read: {error}"))?;
    serde_json::from_str(&body).map_err(|error| format!("unexpected release list: {error}"))
}

/// The release to offer, or none when the running launcher is already the newest one.
pub fn latest_update(url: &str, current: &str) -> Result<Option<ReleaseInfo>, String> {
    Ok(newest_release(&fetch_releases(url)?).filter(|release| is_newer(current, &release.version)))
}

// ---------------------------------------------------------------------------------------------- how this copy runs

/// How the running launcher got onto the disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallKind {
    /// Windows setup program (the NSIS installer leaves `uninstall.exe` beside the program) or a Linux AppImage: the
    /// updater can replace it in place.
    Installed,
    /// Windows copy run from the portable zip (or from a build folder): nothing to replace it with but a new download.
    Portable,
    /// Linux without an AppImage (the `.deb`, a build folder): updating needs the package manager.
    Package,
}

pub fn install_kind_for(windows: bool, has_uninstaller: bool, appimage: bool) -> InstallKind {
    match (windows, has_uninstaller, appimage) {
        (true, true, _) | (false, _, true) => InstallKind::Installed,
        (true, false, _) => InstallKind::Portable,
        (false, _, false) => InstallKind::Package,
    }
}

pub fn install_kind() -> InstallKind {
    let has_uninstaller = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|folder| folder.join("uninstall.exe").is_file()))
        .unwrap_or(false);
    install_kind_for(cfg!(windows), has_uninstaller, std::env::var_os("APPIMAGE").is_some_and(|path| !path.is_empty()))
}

/// What the notice's button does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Downloads, installs and restarts.
    Automatic,
    /// The portable zip: opens the release page (the interface says the zip cannot update itself).
    Portable,
    /// Opens the release page: a package install, or a release without a signed manifest.
    #[default]
    Manual,
}

pub fn mode_for(kind: InstallKind, has_manifest: bool, key_usable: bool) -> Mode {
    match kind {
        InstallKind::Installed if has_manifest && key_usable => Mode::Automatic,
        InstallKind::Portable => Mode::Portable,
        _ => Mode::Manual,
    }
}

/// Whether `pubkey` (the `plugins.updater.pubkey` of the config: the base64 of a minisign public key file) can verify
/// anything. The placeholder and an empty or damaged value turn the in-place update off instead of failing at download time.
pub fn pubkey_usable(pubkey: &str) -> bool {
    let pubkey = pubkey.trim();
    if pubkey.is_empty() || pubkey == PUBKEY_PLACEHOLDER {
        return false;
    }
    base64::engine::general_purpose::STANDARD
        .decode(pubkey)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .is_some_and(|text| text.starts_with("untrusted comment:") && text.lines().count() >= 2)
}

/// Only GitHub's own hosts may be asked for the manifest, whatever a release listing says.
pub fn trusted_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    !host.contains('@')
        && (host == "github.com" || host == "api.github.com" || host.ends_with(".githubusercontent.com"))
}

// ---------------------------------------------------------------------------------------------- state shown to the player

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    /// No newer release, or the check failed: the interface shows nothing.
    #[default]
    Idle,
    Available,
    Downloading,
    Installing,
    Error,
}

/// What the interface draws (event `update-state`, command `update_state`).
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateState {
    pub phase: Phase,
    pub mode: Mode,
    pub version: String,
    pub page_url: String,
    /// Bytes downloaded and announced size (0 while unknown).
    pub done: u64,
    pub total: u64,
    /// English detail of the last failure, for the log and the tooltip.
    pub error: String,
}

// ---------------------------------------------------------------------------------------------- when it is safe

/// What the launcher is doing that an update must not interrupt.
#[derive(Debug, Clone, Copy, Default)]
pub struct Activity {
    pub game: bool,
    pub installation: bool,
    pub optimizer: bool,
    pub texture_pack: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blocker {
    Game,
    Installation,
    Optimizer,
    TexturePack,
}

impl Blocker {
    /// Stable code the interface turns into a translated reason.
    pub fn code(self) -> &'static str {
        match self {
            Self::Game => "game",
            Self::Installation => "installation",
            Self::Optimizer => "optimizer",
            Self::TexturePack => "texturePack",
        }
    }
}

impl Activity {
    pub fn blocker(&self) -> Option<Blocker> {
        if self.game {
            Some(Blocker::Game)
        } else if self.installation {
            Some(Blocker::Installation)
        } else if self.optimizer {
            Some(Blocker::Optimizer)
        } else if self.texture_pack {
            Some(Blocker::TexturePack)
        } else {
            None
        }
    }
}

/// True when the next check is due (never checked, or the wait is over).
pub fn check_due(next_check: Option<Instant>, now: Instant) -> bool {
    next_check.is_none_or(|at| now >= at)
}

// ---------------------------------------------------------------------------------------------- log

/// Appends a line to `update.log` (best effort: a failed check must never get in the player's way).
pub fn log(file: &Path, message: &str) {
    if let Some(folder) = file.parent() {
        let _ = std::fs::create_dir_all(folder);
    }
    let seconds = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    if let Ok(mut handle) = OpenOptions::new().create(true).append(true).open(file) {
        let _ = writeln!(handle, "[{seconds}] {message}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::TcpListener;

    fn release(tag: &str) -> GithubRelease {
        GithubRelease {
            tag_name: tag.into(),
            html_url: format!("https://github.com/o/r/releases/tag/{tag}"),
            ..GithubRelease::default()
        }
    }

    fn version(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    #[test]
    fn only_stable_launcher_tags_name_a_version() {
        assert_eq!(tag_version("launcher-v0.2.0"), Some(version("0.2.0")));
        assert_eq!(tag_version("launcher-v1.10.3"), Some(version("1.10.3")));
        assert_eq!(tag_version("launcher-v0.3.0-beta.1"), None);
        assert_eq!(tag_version("v0.2.0"), None);
        assert_eq!(tag_version("launcher-v"), None);
        assert_eq!(tag_version("launcher-vNEXT"), None);
        assert_eq!(tag_version("launcher-v1.2"), None);
    }

    #[test]
    fn the_newest_is_chosen_by_semver_not_by_text_or_order() {
        let releases = [release("launcher-v0.9.0"), release("launcher-v0.10.0"), release("launcher-v0.2.0")];
        assert_eq!(newest_release(&releases).unwrap().version, version("0.10.0"));
    }

    #[test]
    fn drafts_prereleases_and_other_tags_are_ignored() {
        let releases = [
            GithubRelease { draft: true, ..release("launcher-v9.0.0") },
            GithubRelease { prerelease: true, ..release("launcher-v8.0.0") },
            release("launcher-v7.0.0-rc.1"),
            release("v6.0.0"),
            release("launcher-v0.2.0"),
        ];
        assert_eq!(newest_release(&releases).unwrap().version, version("0.2.0"));
        assert_eq!(newest_release(&[release("v1.0.0")]), None);
        assert_eq!(newest_release(&[]), None);
    }

    #[test]
    fn the_manifest_is_found_among_the_assets() {
        let mut with = release("launcher-v0.2.0");
        with.assets = vec![
            GithubAsset { name: "setup.exe".into(), browser_download_url: "https://github.com/o/r/a".into() },
            GithubAsset { name: MANIFEST_ASSET.into(), browser_download_url: "https://github.com/o/r/m".into() },
        ];
        let found = newest_release(&[with]).unwrap();
        assert_eq!(found.manifest_url.as_deref(), Some("https://github.com/o/r/m"));
        assert_eq!(newest_release(&[release("launcher-v0.2.0")]).unwrap().manifest_url, None);
    }

    #[test]
    fn newer_means_a_higher_version_only() {
        assert!(is_newer("0.1.0", &version("0.1.1")));
        assert!(is_newer("0.9.0", &version("0.10.0")));
        assert!(!is_newer("0.2.0", &version("0.2.0")));
        assert!(!is_newer("0.3.0", &version("0.2.9")));
        assert!(!is_newer("not-a-version", &version("9.9.9")));
    }

    #[test]
    fn the_install_kind_follows_what_is_next_to_the_program() {
        assert_eq!(install_kind_for(true, true, false), InstallKind::Installed);
        assert_eq!(install_kind_for(true, false, false), InstallKind::Portable);
        assert_eq!(install_kind_for(false, false, true), InstallKind::Installed);
        assert_eq!(install_kind_for(false, false, false), InstallKind::Package);
        // A stray uninstall.exe or APPIMAGE on the other system means nothing.
        assert_eq!(install_kind_for(false, true, false), InstallKind::Package);
        assert_eq!(install_kind_for(true, false, true), InstallKind::Portable);
    }

    #[test]
    fn only_an_installed_copy_with_a_manifest_and_a_key_updates_itself() {
        assert_eq!(mode_for(InstallKind::Installed, true, true), Mode::Automatic);
        assert_eq!(mode_for(InstallKind::Installed, false, true), Mode::Manual);
        assert_eq!(mode_for(InstallKind::Installed, true, false), Mode::Manual);
        assert_eq!(mode_for(InstallKind::Portable, true, true), Mode::Portable);
        assert_eq!(mode_for(InstallKind::Package, true, true), Mode::Manual);
    }

    #[test]
    fn the_public_key_must_be_a_real_one() {
        let engine = base64::engine::general_purpose::STANDARD;
        let key = engine
            .encode("untrusted comment: minisign public key: 1234ABCD\nRWQabcdefghijklmnopqrstuvwxyz0123456789ABCD\n");
        assert!(pubkey_usable(&key));
        assert!(pubkey_usable(&format!("  {key}\n")));
        assert!(!pubkey_usable(""));
        assert!(!pubkey_usable(PUBKEY_PLACEHOLDER));
        assert!(!pubkey_usable("not base64 at all!"));
        assert!(!pubkey_usable(&engine.encode("just some text")));
    }

    #[test]
    fn only_github_hosts_are_trusted_for_the_manifest() {
        assert!(trusted_url("https://github.com/o/r/releases/download/launcher-v0.2.0/latest.json"));
        assert!(trusted_url("https://objects.githubusercontent.com/x"));
        assert!(!trusted_url("http://github.com/o/r/latest.json"));
        assert!(!trusted_url("https://github.com.evil.example/latest.json"));
        assert!(!trusted_url("https://github.com@evil.example/latest.json"));
        assert!(!trusted_url("https://evilgithubusercontent.com/x"));
        assert!(!trusted_url("file:///etc/passwd"));
    }

    #[test]
    fn the_first_blocker_in_priority_order_is_reported() {
        assert_eq!(Activity::default().blocker(), None);
        assert_eq!(
            Activity { optimizer: true, texture_pack: true, ..Activity::default() }.blocker(),
            Some(Blocker::Optimizer)
        );
        assert_eq!(Activity { game: true, installation: true, ..Activity::default() }.blocker(), Some(Blocker::Game));
        assert_eq!(
            Activity { installation: true, ..Activity::default() }.blocker().map(Blocker::code),
            Some("installation")
        );
        assert_eq!(
            Activity { texture_pack: true, ..Activity::default() }.blocker().map(Blocker::code),
            Some("texturePack")
        );
    }

    #[test]
    fn checks_are_spaced_out() {
        let now = Instant::now();
        assert!(check_due(None, now));
        assert!(!check_due(Some(now + CHECK_INTERVAL), now));
        assert!(check_due(Some(now), now));
        assert!(check_due(Some(now), now + Duration::from_secs(1)));
        assert!(RETRY_INTERVAL < CHECK_INTERVAL);
    }

    /// Answers one request with `body` as JSON and returns the address.
    fn serve_once(status: &'static str, body: String) -> String {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 2048];
            let _ = stream.read(&mut request);
            let reply = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(reply.as_bytes());
        });
        format!("http://{address}/releases")
    }

    #[test]
    fn a_listing_gives_the_release_to_offer() {
        let body = r#"[
            {"tag_name":"launcher-v0.2.0","draft":false,"prerelease":false,"html_url":"https://github.com/o/r/releases/tag/launcher-v0.2.0",
             "assets":[{"name":"latest.json","browser_download_url":"https://github.com/o/r/releases/download/launcher-v0.2.0/latest.json","size":1}],"author":{}},
            {"tag_name":"launcher-v0.1.0","draft":false,"prerelease":false,"html_url":"https://github.com/o/r/releases/tag/launcher-v0.1.0","assets":[]}
        ]"#;
        let found = latest_update(&serve_once("200 OK", body.into()), "0.1.0").unwrap().unwrap();
        assert_eq!(found.version, version("0.2.0"));
        assert!(found.manifest_url.unwrap().ends_with("launcher-v0.2.0/latest.json"));
        assert_eq!(latest_update(&serve_once("200 OK", body.into()), "0.2.0").unwrap(), None);
    }

    #[test]
    fn a_failing_check_is_an_error_not_a_panic() {
        assert!(latest_update(
            &serve_once("403 Forbidden", r#"{"message":"API rate limit exceeded"}"#.into()),
            "0.1.0"
        )
        .is_err());
        assert!(latest_update(&serve_once("200 OK", "not json".into()), "0.1.0").is_err());
        assert!(latest_update("http://127.0.0.1:1/none", "0.1.0").is_err());
    }

    #[test]
    fn the_log_appends_lines() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("logs/update.log");
        log(&file, "first");
        log(&file, "second");
        let text = std::fs::read_to_string(&file).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(text.lines().next().unwrap().ends_with("] first"));
    }
}
