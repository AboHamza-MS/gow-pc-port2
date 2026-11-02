//! The session report: a ZIP the player attaches to a bug report. It holds the launcher's logs, what the launcher knows
//! about the PC and the installation, the settings and the environment the game is started with, and the notes the
//! player wrote, all with the names of the player and the PC taken out.
//!
//! What goes in is decided by a list of file names, not by what is lying around: the game, its data, the saves, the
//! recorded microcode and the textures are never read. Redaction works on the copy that goes in the ZIP, never on the
//! files on disk (see [`scrub`]).

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::controller;
use crate::game;
use crate::gpu::GraphicsCard;
use crate::launch::{self, LaunchContext};
use crate::optimizer::OptimizerSnapshot;
use crate::paths::AppPaths;
use crate::rebuild;
use crate::settings::{DisplayInfo, Settings};

pub mod host;
pub mod scrub;
pub mod session;

pub use host::HostFacts;
pub use scrub::Scrubber;

/// Version of the layout of `report.json`.
pub const SCHEMA: u32 = 1;
/// Reports kept in `<data>/reports`; older ones are deleted when a new one is made.
pub const KEEP_REPORTS: usize = 10;
/// Largest size of a game log in the ZIP, and of every other log.
pub const GAME_LOG_CAP: usize = 4 * 1024 * 1024;
pub const OTHER_LOG_CAP: usize = 1024 * 1024;
/// Start of a capped log that is always kept: the beginning explains how the session started.
pub const LOG_HEAD: usize = 256 * 1024;

/// The only files of the logs folder that can enter a report, with the size each is cut to.
pub const ALLOWED_LOGS: [(&str, usize); 6] = [
    ("last.log", GAME_LOG_CAP),
    ("previous.log", GAME_LOG_CAP),
    ("install.log", OTHER_LOG_CAP),
    ("optimize.log", OTHER_LOG_CAP),
    ("update.log", OTHER_LOG_CAP),
    ("kit.log", OTHER_LOG_CAP),
];

/// Variables of the launcher's own environment that are listed in the report (the ones that change the game).
const ENVIRONMENT_PREFIXES: [&str; 3] = ["GOW_", "PS2X_", "PC_"];

/// What the ZIP's `README.txt` says.
const README: &str = include_str!("report/README.txt");

/// The README with Windows line breaks, so any editor shows it whatever the checkout's line endings were.
fn readme_text() -> String {
    README.replace("\r\n", "\n").replace('\n', "\r\n")
}

// ------------------------------------------------------------------------------------------------------ time

/// Unix seconds as `(year, month, day, hour, minute, second)` in UTC.
pub fn utc_parts(seconds: u64) -> (i64, u32, u32, u32, u32, u32) {
    let days = (seconds / 86_400) as i64;
    let rest = seconds % 86_400;
    // Days since 1970-01-01 to a civil date (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u32;
    let month = if shifted_month < 10 { shifted_month + 3 } else { shifted_month - 9 } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day, (rest / 3600) as u32, (rest % 3600 / 60) as u32, (rest % 60) as u32)
}

/// `2026-11-02T01:23:45Z`.
pub fn iso8601(seconds: u64) -> String {
    let (y, mo, d, h, mi, s) = utc_parts(seconds);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// `20261102-012345`, the part of the file name that sorts by time.
pub fn file_stamp(seconds: u64) -> String {
    let (y, mo, d, h, mi, s) = utc_parts(seconds);
    format!("{y:04}{mo:02}{d:02}-{h:02}{mi:02}{s:02}")
}

pub fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

// ------------------------------------------------------------------------------------------------------ logs

/// A log cut to `cap` bytes: the first [`LOG_HEAD`] bytes, a line saying how much was left out, and the end. The cuts
/// fall on line breaks. A log that fits is returned whole.
pub fn cap_log(bytes: &[u8], cap: usize) -> Vec<u8> {
    if bytes.len() <= cap {
        return bytes.to_vec();
    }
    let head_end =
        bytes[..LOG_HEAD.min(cap / 2)].iter().rposition(|b| *b == b'\n').map_or(LOG_HEAD.min(cap / 2), |p| p + 1);
    let tail_from = bytes.len() - (cap - head_end.min(cap));
    let tail_start = bytes[tail_from..].iter().position(|b| *b == b'\n').map_or(tail_from, |p| tail_from + p + 1);
    let omitted = tail_start.saturating_sub(head_end);
    let mut out = Vec::with_capacity(cap + 64);
    out.extend_from_slice(&bytes[..head_end]);
    out.extend_from_slice(format!("[... {omitted} bytes omitted ...]\n").as_bytes());
    out.extend_from_slice(&bytes[tail_start..]);
    out
}

fn read_log(path: &Path, cap: usize, scrubber: &Scrubber) -> Option<Vec<u8>> {
    let bytes = fs::read(path).ok()?;
    let text = drop_login_lines(&String::from_utf8_lossy(&cap_log(&bytes, cap)));
    Some(scrubber.scrub(&text).into_bytes())
}

// ------------------------------------------------------------------------------------------------------ redaction

/// The folders and names this PC wants hidden, for a report about `settings`.
pub fn scrubber_for(paths: &AppPaths, settings: &Settings, host: &HostFacts) -> Scrubber {
    let parent = |path: &str| Path::new(path).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    let text = |path: &Path| path.to_string_lossy().into_owned();
    let iso_dir = parent(&settings.iso);
    let elf_dir = parent(&settings.elf);
    let runner_dir = parent(&settings.executable);
    let mut scrubber = Scrubber::new(cfg!(windows))
        .folder(settings.optimization_destination.trim(), "<INSTALL>")
        .folder(&text(paths.data_dir()), "<DATA>")
        .folder(&text(&host.temp), "<TEMP>")
        .folder(&iso_dir, "<ISO_DIR>")
        .folder(&elf_dir, "<GAME>")
        .folder(&runner_dir, "<RUNNER>");
    if let Some(home) = &host.home {
        scrubber = scrubber.folder(&text(home), "<HOME>");
    }
    if let Some(launcher) = &host.launcher_dir {
        scrubber = scrubber.folder(&text(launcher), "<LAUNCHER>");
    }
    if let Some(user) = &host.user_name {
        scrubber = scrubber.name(user, "<USER>");
    }
    if let Some(computer) = &host.computer_name {
        scrubber = scrubber.name(computer, "<PC>");
    }
    // The RetroAchievements handle is a public profile name: it must not travel in a report, and the game's own log can
    // carry it (the client logs the login), so it is a scrubbed name like the others.
    scrubber = scrubber.name(&settings.retro_achievements_user, "<RA_USER>");
    scrubber
}

/// The RetroAchievements client logs who logs in (`Attempting to log in <name>`, `<name> logged in successfully`) at its
/// default level. The name is scrubbed when the settings know it, but a handle that differs in case, or one typed in the
/// game, would pass, so these two kinds of line are dropped whole.
fn drop_login_lines(text: &str) -> String {
    text.split_inclusive('\n')
        .map(|line| {
            if line.contains("Attempting to log in") || line.contains("logged in successfully") {
                if line.ends_with('\n') {
                    "[login line removed]\n"
                } else {
                    "[login line removed]"
                }
            } else {
                line
            }
        })
        .collect()
}

/// The settings as the report shows them: every text goes through the scrubber (paths, mostly), and the
/// RetroAchievements user is only said to be set or not.
pub fn redact_settings(settings: &Settings, scrubber: &Scrubber) -> Value {
    let mut value = serde_json::to_value(settings).unwrap_or(Value::Null);
    if let Value::Object(map) = &mut value {
        for (key, entry) in map.iter_mut() {
            if key == "retroAchievementsUser" {
                let set = entry.as_str().is_some_and(|user| !user.trim().is_empty());
                *entry = Value::String(if set { "<set>" } else { "" }.into());
            } else if let Value::String(text) = entry {
                *text = scrubber.scrub(text);
            }
        }
    }
    value
}

/// Values that are never shown: the RetroAchievements account and anything named like a secret.
fn is_secret_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper == "PC_RA_USERNAME" || ["PASSWORD", "TOKEN", "SECRET", "CREDENTIAL"].iter().any(|word| upper.contains(word))
}

pub fn redact_environment_value(name: &str, value: &str, scrubber: &Scrubber) -> String {
    if is_secret_name(name) {
        "<redacted>".into()
    } else {
        scrubber.scrub(value)
    }
}

// ------------------------------------------------------------------------------------------------------ report.json

/// What a report needs from the running launcher.
pub struct ReportRequest<'a> {
    pub paths: &'a AppPaths,
    pub settings: &'a Settings,
    pub launcher_version: &'a str,
    pub display: DisplayInfo,
    pub cards: &'a [GraphicsCard],
    pub optimizer: &'a OptimizerSnapshot,
    pub game_running: bool,
    /// Where the optimizer has the game record microcode (empty when off), as the launch would pass it.
    pub vu1_capture: &'a str,
    /// Unix seconds.
    pub now: u64,
}

fn sha256_of(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Some(game::hex(&hasher.finalize()))
}

/// The runner's size, date and checksum. The runner is our own build, not game data.
fn runner_facts(executable: &str) -> Value {
    let path = Path::new(executable);
    let Ok(meta) = fs::metadata(path) else { return json!({ "exists": false }) };
    let modified = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| iso8601(d.as_secs()));
    json!({ "exists": meta.is_file(), "sizeBytes": meta.len(), "modified": modified, "sha256": sha256_of(path) })
}

fn installation_facts(request: &ReportRequest<'_>) -> Value {
    let settings = request.settings;
    let game_state = game::check_game(&settings.elf, &settings.executable);
    let data = game::check_data(&settings.elf, &settings.iso);
    let kit_digest = rebuild::read_kit_digest(&request.paths.kit_dir());
    let built_from = (!settings.optimization_destination.trim().is_empty())
        .then(|| rebuild::read_built_from(Path::new(settings.optimization_destination.trim())))
        .flatten();
    json!({
        "runner": runner_facts(&settings.executable),
        "gameFile": {
            "exists": game_state.exists,
            "crc": game_state.crc,
            "valid": game_state.valid,
            "modified": game_state.modified,
        },
        "dataExtracted": data.extracted,
        "isoChosen": !settings.iso.is_empty(),
        "kitDigest": kit_digest.as_deref().map(rebuild::short),
        "builtFromKitDigest": built_from.as_deref().map(rebuild::short),
    })
}

/// The environment the launcher would give the game now (never reads the RetroAchievements password).
fn environment_plan(request: &ReportRequest<'_>, scrubber: &Scrubber, env: &[(String, String)]) -> Value {
    let settings = request.settings;
    let ctx = LaunchContext {
        settings,
        scale: settings.selected_scale(),
        vu1_capture: request.vu1_capture.to_string(),
        gpu: settings.gpu.clone(),
        icon: Some(request.paths.icon()),
        shader_cache: request.paths.shader_cache(),
        pad_devices_file: Some(request.paths.pad_devices_file()),
        retro_achievements_password: None,
    };
    let plan =
        launch::build_environment(&ctx, |name| env.iter().find(|(key, _)| key == name).map(|(_, value)| value.clone()));
    let set: BTreeMap<&String, String> =
        plan.set.iter().map(|(name, value)| (name, redact_environment_value(name, value, scrubber))).collect();
    json!({ "set": set, "removed": plan.remove })
}

/// The `GOW_`, `PS2X_` and `PC_` variables the launcher itself was started with.
fn inherited_environment(scrubber: &Scrubber, env: &[(String, String)]) -> BTreeMap<String, String> {
    env.iter()
        .filter(|(name, _)| ENVIRONMENT_PREFIXES.iter().any(|prefix| name.starts_with(prefix)))
        .map(|(name, value)| (name.clone(), redact_environment_value(name, value, scrubber)))
        .collect()
}

/// The joysticks the game saw the last time it ran (the controller picker's list), with the names scrubbed: a Bluetooth pad is
/// often called after its owner. The GUIDs only name a model.
fn controller_facts(request: &ReportRequest<'_>, scrubber: &Scrubber) -> Value {
    let Some(list) = controller::read(&request.paths.pad_devices_file()) else { return Value::Null };
    let devices: Vec<Value> = list
        .devices
        .iter()
        .map(|device| {
            json!({
                "index": device.index,
                "name": scrubber.scrub(&device.name),
                "guid": device.guid,
                "mapped": device.mapped,
                "buttons": device.buttons,
                "hats": device.hats,
                "axes": device.axes,
            })
        })
        .collect();
    json!({
        "seenAt": (list.seen_at > 0).then(|| iso8601(list.seen_at)),
        "choice": list.choice,
        "active": list.active,
        "chosenGuid": request.settings.pad_device,
        "devices": devices,
    })
}

/// `lastSession` of the report. A file that says the game is running while it is not (the launcher was closed or killed
/// during the session) is reported as interrupted.
fn last_session_facts(request: &ReportRequest<'_>, scrubber: &Scrubber) -> Value {
    let Some(mut session) = session::read_session(request.paths) else { return Value::Null };
    let interrupted = session.running && !request.game_running;
    session.running = session.running && request.game_running;
    session.summary = session.summary.iter().map(|line| scrubber.scrub(line)).collect();
    let mut value = serde_json::to_value(&session).unwrap_or(Value::Null);
    if let Value::Object(map) = &mut value {
        map.insert("interrupted".into(), Value::Bool(interrupted));
        for key in ["startedAt", "endedAt"] {
            if let Some(at) = map.get(key).and_then(Value::as_u64).filter(|at| *at > 0) {
                map.insert(format!("{key}Iso"), Value::String(iso8601(at)));
            }
        }
    }
    value
}

pub fn report_json(
    request: &ReportRequest<'_>,
    host: &HostFacts,
    env: &[(String, String)],
    scrubber: &Scrubber,
) -> Value {
    json!({
        "schema": SCHEMA,
        "createdAt": iso8601(request.now),
        "launcherVersion": request.launcher_version,
        "platform": if cfg!(windows) { "windows" } else { "linux" },
        "partial": request.game_running,
        "os": host.os,
        "cpu": { "name": host.cpu, "logicalCores": host.logical_cores },
        "ramMb": host.ram_mb,
        "display": request.display,
        "graphicsCards": request.cards,
        "adapters": host.adapters.iter().map(|adapter| serde_json::to_value(adapter).unwrap_or(Value::Null)).collect::<Vec<_>>(),
        "settings": redact_settings(request.settings, scrubber),
        "installation": installation_facts(request),
        "optimizer": request.optimizer,
        "gameEnvironment": environment_plan(request, scrubber, env),
        "launcherEnvironment": inherited_environment(scrubber, env),
        "controllers": controller_facts(request, scrubber),
        "lastSession": last_session_facts(request, scrubber),
    })
}

// ------------------------------------------------------------------------------------------------------ markers.txt

fn markers_text(paths: &AppPaths, scrubber: &Scrubber) -> String {
    let markers = session::read_markers(paths);
    if markers.is_empty() {
        return "(no markers)\r\n".into();
    }
    markers
        .iter()
        .map(|marker| {
            let into = marker.since_game_start.map(|s| format!(" (+{s} s into the game)")).unwrap_or_default();
            format!("[{}]{} {}\r\n", iso8601(marker.at), into, scrubber.scrub(&marker.text))
        })
        .collect()
}

// ------------------------------------------------------------------------------------------------------ the ZIP

/// Whether a name may be written to the ZIP: the three fixed files and the allowed logs.
pub fn entry_allowed(name: &str) -> bool {
    matches!(name, "report.json" | "markers.txt" | "README.txt")
        || name.strip_prefix("logs/").is_some_and(|file| ALLOWED_LOGS.iter().any(|(allowed, _)| *allowed == file))
}

/// All the files of a report, as `(name in the ZIP, content)`.
pub fn build_files(request: &ReportRequest<'_>, host: &HostFacts, env: &[(String, String)]) -> Vec<(String, Vec<u8>)> {
    let scrubber = scrubber_for(request.paths, request.settings, host);
    let report = report_json(request, host, env, &scrubber);
    let mut files = vec![
        ("README.txt".to_string(), readme_text().into_bytes()),
        ("report.json".to_string(), serde_json::to_vec_pretty(&report).unwrap_or_default()),
        ("markers.txt".to_string(), markers_text(request.paths, &scrubber).into_bytes()),
    ];
    for (name, cap) in ALLOWED_LOGS {
        if let Some(content) = read_log(&request.paths.logs_dir().join(name), cap, &scrubber) {
            files.push((format!("logs/{name}"), content));
        }
    }
    files
}

/// Writes `files` to `path` (through a `.part` file, so a failed export leaves no broken ZIP). Refuses a name that is not
/// on the allowed list.
pub fn write_zip(path: &Path, files: &[(String, Vec<u8>)]) -> io::Result<()> {
    if let Some((name, _)) = files.iter().find(|(name, _)| !entry_allowed(name)) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("{name} is not allowed in a report")));
    }
    let part = path.with_extension("zip.part");
    let result = (|| {
        let mut writer = zip::ZipWriter::new(File::create(&part)?);
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, content) in files {
            writer.start_file(name, options).map_err(io::Error::other)?;
            writer.write_all(content)?;
        }
        writer.finish().map_err(io::Error::other)?.sync_all()?;
        fs::rename(&part, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&part);
    }
    result
}

pub fn reports_dir(paths: &AppPaths) -> PathBuf {
    paths.data_dir().join("reports")
}

/// Deletes all but the newest `keep` reports (`gow-report-*.zip`, which sort by time).
pub fn prune_reports(directory: &Path, keep: usize) {
    let Ok(entries) = fs::read_dir(directory) else { return };
    let mut names: Vec<String> = entries
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("gow-report-") && name.ends_with(".zip"))
        .collect();
    names.sort();
    let surplus = names.len().saturating_sub(keep);
    for name in &names[..surplus] {
        let _ = fs::remove_file(directory.join(name));
    }
}

/// A report that was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exported {
    pub path: PathBuf,
    pub name: String,
}

/// Makes the report, prunes the old ones and returns where it is.
pub fn export(request: &ReportRequest<'_>, host: &HostFacts, env: &[(String, String)]) -> io::Result<Exported> {
    let directory = reports_dir(request.paths);
    fs::create_dir_all(&directory)?;
    let files = build_files(request, host, env);
    let stamp = file_stamp(request.now);
    let mut name = format!("gow-report-{stamp}.zip");
    let mut suffix = 2;
    while directory.join(&name).exists() {
        name = format!("gow-report-{stamp}-{suffix}.zip");
        suffix += 1;
    }
    let path = directory.join(&name);
    write_zip(&path, &files)?;
    prune_reports(&directory, KEEP_REPORTS);
    Ok(Exported { path, name })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::optimizer::OptimizerStatus;
    use std::io::Read;

    struct World {
        _directory: tempfile::TempDir,
        paths: AppPaths,
        settings: Settings,
        host: HostFacts,
        root: PathBuf,
    }

    /// A fake PC for user "Quentin" (computer "DESKTOP-9K2"), with the data, game and disc folders under the temp folder.
    fn world() -> World {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().to_path_buf();
        let home = root.join("home").join("Quentin");
        let data = home.join("AppData").join("Launcher");
        let install = home.join("GodOfWar-PC");
        let game_dir = install.join("game");
        let active = install.join("active");
        let iso_dir = root.join("Isos");
        for folder in [&data.join("logs"), &game_dir, &active, &iso_dir] {
            fs::create_dir_all(folder).unwrap();
        }
        let elf = game_dir.join("SCUS_973.99");
        fs::write(&elf, b"\x7FELFnot a game").unwrap();
        let executable = active.join(game::EXECUTABLE_NAME);
        fs::write(&executable, b"runner bytes").unwrap();
        let settings = Settings {
            elf: elf.to_string_lossy().into_owned(),
            executable: executable.to_string_lossy().into_owned(),
            iso: iso_dir.join("God of War.iso").to_string_lossy().into_owned(),
            optimization_destination: install.to_string_lossy().into_owned(),
            retro_achievements_enabled: true,
            retro_achievements_user: "QuentinRetro".into(),
            ..Settings::default()
        };
        let cache = home.join("cache");
        let host = HostFacts {
            os: Some("Windows 11 Pro".into()),
            cpu: Some("Test CPU".into()),
            logical_cores: Some(8),
            ram_mb: Some(16_000),
            adapters: Vec::new(),
            user_name: Some("Quentin".into()),
            computer_name: Some("DESKTOP-9K2".into()),
            home: Some(home),
            temp: root.join("Temp"),
            launcher_dir: Some(root.join("Launcher")),
        };
        World { paths: AppPaths::new(data, cache), _directory: directory, settings, host, root }
    }

    fn optimizer() -> OptimizerSnapshot {
        OptimizerSnapshot {
            configured: true,
            compiling: false,
            captured: 3,
            compiled: 2,
            completion_ratio: 0.5,
            build_progress: -1.0,
            status: OptimizerStatus::Idle,
        }
    }

    fn request<'a>(world: &'a World, optimizer: &'a OptimizerSnapshot, game_running: bool) -> ReportRequest<'a> {
        ReportRequest {
            paths: &world.paths,
            settings: &world.settings,
            launcher_version: "9.9.9",
            display: DisplayInfo::default(),
            cards: &[],
            optimizer,
            game_running,
            vu1_capture: "",
            now: 1_700_000_000,
        }
    }

    fn zip_names(path: &Path) -> Vec<String> {
        let mut archive = zip::ZipArchive::new(File::open(path).unwrap()).unwrap();
        (0..archive.len()).map(|i| archive.by_index(i).unwrap().name().unwrap().to_string()).collect()
    }

    fn zip_text(path: &Path) -> String {
        let mut archive = zip::ZipArchive::new(File::open(path).unwrap()).unwrap();
        let mut all = String::new();
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).unwrap();
            all.push_str(&format!("=== {}\n", entry.name().unwrap()));
            entry.read_to_string(&mut all).unwrap();
        }
        all
    }

    #[test]
    fn time_is_formatted_in_utc() {
        assert_eq!(iso8601(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(iso8601(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601(951_782_400 + 86_399), "2000-02-29T23:59:59Z");
        assert_eq!(file_stamp(1_700_000_000), "20231114-221320");
    }

    #[test]
    fn logs_are_capped_with_a_marker_line() {
        let line = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\n";
        let log: Vec<u8> = line.repeat(40_000).into_bytes(); // 2.6 MB
        assert_eq!(cap_log(&log, 4 * 1024 * 1024), log, "a log that fits is untouched");
        let capped = cap_log(&log, 1024 * 1024);
        let text = String::from_utf8(capped.clone()).unwrap();
        assert!(capped.len() <= 1024 * 1024 + 64, "{}", capped.len());
        assert!(text.starts_with(line), "the beginning is kept");
        assert!(text.ends_with(line), "the end is kept");
        let marker = text.lines().find(|l| l.starts_with("[... ")).expect("marker line");
        let omitted: usize = marker.trim_start_matches("[... ").split(' ').next().unwrap().parse().unwrap();
        assert_eq!(omitted + (text.len() - marker.len() - 1), log.len(), "kept plus omitted is the whole log");
        // Every kept line is a whole one.
        assert!(text.lines().filter(|l| !l.starts_with("[... ")).all(|l| l.len() == line.len() - 1));
    }

    #[test]
    fn a_log_without_line_breaks_is_still_capped() {
        let log = vec![b'x'; 3 * 1024 * 1024];
        let capped = cap_log(&log, 1024 * 1024);
        assert!(capped.len() < 1024 * 1024 + 64);
        assert!(String::from_utf8_lossy(&capped).contains("bytes omitted"));
    }

    #[test]
    fn settings_are_redacted() {
        let world = world();
        let scrubber = scrubber_for(&world.paths, &world.settings, &world.host);
        let text = redact_settings(&world.settings, &scrubber).to_string();
        let home = world.host.home.as_ref().unwrap().to_string_lossy().into_owned();
        assert!(!text.contains("Quentin"), "{text}");
        assert!(!text.replace("\\\\", "\\").contains(&home));
        assert!(text.contains("<INSTALL>"));
        assert!(text.contains("\"retroAchievementsUser\":\"<set>\""));
        let none = Settings::default();
        assert_eq!(redact_settings(&none, &scrubber)["retroAchievementsUser"], "");
    }

    #[test]
    fn the_retroachievements_user_does_not_survive_in_a_log() {
        let world = world();
        let scrubber = scrubber_for(&world.paths, &world.settings, &world.host);
        // The user of the settings is scrubbed wherever it appears; the client's login lines are dropped whole, which also
        // covers a handle that differs in case or was typed in the game.
        let log = "[ra] Attempting to log in QuentinRetro (with password)\n[ra] someotherhandle logged in successfully\n[ra] points: QuentinRetro 12\nlast line";
        let scrubbed = scrubber.scrub(&drop_login_lines(log));
        assert!(!scrubbed.contains("QuentinRetro"), "{scrubbed}");
        assert!(!scrubbed.contains("someotherhandle"), "{scrubbed}");
        assert!(scrubbed.contains("[login line removed]\n[login line removed]\n"), "{scrubbed}");
        assert!(scrubbed.contains("<RA_USER> 12"), "{scrubbed}");
        assert!(scrubbed.ends_with("last line"), "{scrubbed}");
    }

    #[test]
    fn environment_is_redacted() {
        let world = world();
        let scrubber = scrubber_for(&world.paths, &world.settings, &world.host);
        assert_eq!(redact_environment_value("PC_RA_PASSWORD", "hunter2", &scrubber), "<redacted>");
        assert_eq!(redact_environment_value("PC_RA_USERNAME", "QuentinRetro", &scrubber), "<redacted>");
        assert_eq!(redact_environment_value("SOME_API_TOKEN", "abc", &scrubber), "<redacted>");
        assert_eq!(redact_environment_value("GOW_SCALE", "2", &scrubber), "2");
        let shader = world.paths.shader_cache().to_string_lossy().into_owned();
        assert!(!redact_environment_value("PS2X_GS_SHADER_CACHE_DIR", &shader, &scrubber).contains("Quentin"));
        let env = vec![
            ("PC_RA_PASSWORD".to_string(), "hunter2".to_string()),
            ("PS2X_GS_WORKER".to_string(), "1".to_string()),
            ("PATH".to_string(), "C:\\Users\\Quentin".to_string()),
        ];
        let inherited = inherited_environment(&scrubber, &env);
        assert_eq!(inherited.get("PC_RA_PASSWORD").map(String::as_str), Some("<redacted>"));
        assert_eq!(inherited.get("PS2X_GS_WORKER").map(String::as_str), Some("1"));
        assert!(!inherited.contains_key("PATH"), "only the game's variables are listed");
    }

    #[test]
    fn zip_contains_only_allowed_names() {
        let world = world();
        let logs = world.paths.logs_dir();
        fs::write(logs.join("last.log"), "game log\n").unwrap();
        fs::write(logs.join("install.log"), "install log\n").unwrap();
        // Everything that must never travel, in the places a careless collector would look.
        fs::write(logs.join("Mcd001.ps2"), "card").unwrap();
        fs::write(logs.join("foo.iso"), "disc").unwrap();
        fs::write(logs.join("SCUS_973.99"), "elf").unwrap();
        fs::create_dir_all(logs.join("vu1_captures")).unwrap();
        fs::write(logs.join("vu1_captures").join("a.bin"), "microcode").unwrap();
        fs::write(world.paths.data_dir().join("Mcd001.ps2"), "card").unwrap();
        let snapshot = optimizer();
        let exported = export(&request(&world, &snapshot, false), &world.host, &[]).unwrap();
        let names = zip_names(&exported.path);
        assert!(names.contains(&"report.json".to_string()) && names.contains(&"logs/last.log".to_string()));
        for name in &names {
            assert!(entry_allowed(name), "{name}");
        }
        for forbidden in ["Mcd001", "iso", "SCUS", "vu1_captures", ".bin", ".ps2", ".elf", ".pak", ".irx"] {
            assert!(!names.iter().any(|name| name.contains(forbidden)), "{forbidden}: {names:?}");
        }
        // The writer itself refuses a name outside the list.
        let outside = world.root.join("x.zip");
        assert!(write_zip(&outside, &[("logs/Mcd001.ps2".into(), vec![])]).is_err());
        assert!(!outside.exists() && !outside.with_extension("zip.part").exists());
        assert!(write_zip(&outside, &[("../escape.txt".into(), vec![])]).is_err());
    }

    #[test]
    fn the_report_holds_neither_the_user_nor_the_pc_nor_the_folders() {
        let world = world();
        let logs = world.paths.logs_dir();
        let home = world.host.home.as_ref().unwrap().to_string_lossy().into_owned();
        let elf = world.settings.elf.clone();
        fs::write(
            logs.join("last.log"),
            format!("[gow] loading {elf}\n[gow] card in {home}\\saves on DESKTOP-9K2 for Quentin\n\\\\?\\{home}\\x\n"),
        )
        .unwrap();
        session::add_marker(&world.paths, "fell through the floor, Quentin said", 1_700_000_000, false).unwrap();
        session::session_started(&world.paths, 1_699_999_000);
        let snapshot = optimizer();
        let env = vec![("GOW_AUDIO_DEVICE".to_string(), format!("{home}\\audio"))];
        let exported = export(&request(&world, &snapshot, true), &world.host, &env).unwrap();
        let text = zip_text(&exported.path);
        let lower = text.to_lowercase();
        assert!(!lower.contains("quentin"), "the user name leaked:\n{text}");
        assert!(!lower.contains("desktop-9k2"), "the computer name leaked");
        let root = world.root.to_string_lossy().replace('\\', "/").to_lowercase();
        let flat = lower.replace("\\\\", "/").replace('\\', "/");
        assert!(!flat.contains(&root), "a folder leaked:\n{text}");
        assert!(text.contains("<GAME>") && text.contains("<USER>") && text.contains("<PC>"));
        assert!(text.contains("\"partial\": true"));
    }

    #[test]
    fn the_report_states_the_installation_and_the_session() {
        let world = world();
        let mut snapshot = optimizer();
        snapshot.compiled = 7;
        session::session_started(&world.paths, 1_699_999_000);
        let running = report_json(
            &request(&world, &snapshot, true),
            &world.host,
            &[],
            &scrubber_for(&world.paths, &world.settings, &world.host),
        );
        assert_eq!(running["schema"], 1);
        assert_eq!(running["createdAt"], "2023-11-14T22:13:20Z");
        assert_eq!(running["lastSession"]["running"], true);
        assert_eq!(running["lastSession"]["interrupted"], false);
        assert_eq!(running["optimizer"]["compiled"], 7);
        assert_eq!(running["installation"]["runner"]["sizeBytes"], 12);
        assert_eq!(running["installation"]["runner"]["sha256"].as_str().map(str::len), Some(64));
        assert_eq!(running["installation"]["gameFile"]["exists"], true);
        assert_eq!(running["installation"]["gameFile"]["valid"], false);
        assert_eq!(running["cpu"]["logicalCores"], 8);
        assert_eq!(running["gameEnvironment"]["set"]["PS2X_GS_SCALE"], "2.0");
        // The same file seen while the game is not running: the launcher was closed during the session.
        let stale = report_json(
            &request(&world, &snapshot, false),
            &world.host,
            &[],
            &scrubber_for(&world.paths, &world.settings, &world.host),
        );
        assert_eq!(stale["lastSession"]["running"], false);
        assert_eq!(stale["lastSession"]["interrupted"], true);
        assert_eq!(stale["partial"], false);
    }

    #[test]
    fn the_controllers_the_game_saw_are_listed_with_their_names_scrubbed() {
        let world = world();
        let file = world.paths.pad_devices_file();
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(
            &file,
            r#"{"schema":1,"preferred":"","choice":"automatic","active":0,"devices":[
                {"index":0,"name":"Quentin's Xbox Controller","guid":"0300dddd5e040000a102000000010000","mapped":false,"buttons":15,"hats":1,"axes":6}]}"#,
        )
        .unwrap();
        let snapshot = optimizer();
        let scrubber = scrubber_for(&world.paths, &world.settings, &world.host);
        let json = report_json(&request(&world, &snapshot, false), &world.host, &[], &scrubber);
        let device = &json["controllers"]["devices"][0];
        assert_eq!(device["name"], "<USER>'s Xbox Controller");
        assert_eq!(device["mapped"], false);
        assert_eq!(device["axes"], 6);
        assert_eq!(json["controllers"]["choice"], "automatic");
        // No list yet: null, not an error.
        fs::remove_file(&file).unwrap();
        let none = report_json(&request(&world, &snapshot, false), &world.host, &[], &scrubber);
        assert!(none["controllers"].is_null());
    }

    #[test]
    fn old_reports_are_pruned_to_ten() {
        let directory = tempfile::tempdir().unwrap();
        for index in 0..14 {
            fs::write(directory.path().join(format!("gow-report-20260101-0000{index:02}.zip")), "x").unwrap();
        }
        fs::write(directory.path().join("notes.txt"), "keep me").unwrap();
        prune_reports(directory.path(), KEEP_REPORTS);
        let mut left: Vec<String> = fs::read_dir(directory.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left.len(), 11);
        assert_eq!(left.first().map(String::as_str), Some("gow-report-20260101-000004.zip"));
        assert!(left.contains(&"notes.txt".to_string()));
    }

    #[test]
    fn two_exports_in_the_same_second_both_survive() {
        let world = world();
        let snapshot = optimizer();
        let first = export(&request(&world, &snapshot, false), &world.host, &[]).unwrap();
        let second = export(&request(&world, &snapshot, false), &world.host, &[]).unwrap();
        assert_ne!(first.name, second.name);
        assert!(first.path.exists() && second.path.exists());
        assert_eq!(first.name, "gow-report-20231114-221320.zip");
    }

    #[test]
    fn exporting_without_any_log_still_works() {
        let world = world();
        let snapshot = optimizer();
        let exported = export(&request(&world, &snapshot, false), &world.host, &[]).unwrap();
        let names = zip_names(&exported.path);
        assert_eq!(names, vec!["README.txt", "report.json", "markers.txt"]);
        assert!(zip_text(&exported.path).contains("(no markers)"));
    }
}
