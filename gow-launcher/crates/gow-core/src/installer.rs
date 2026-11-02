//! Installation wizard back end: validates the destination, checks the build tools and runs `install.ps1` (Windows) or
//! `install.sh` (Linux) in the background (extract the disc, translate the ELF, build the runtime), turning its output
//! into stage and progress.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;

use crate::failure;
use crate::game::EXECUTABLE_NAME;
use crate::paths;
use crate::script::{self, quote_for_powershell, ScriptOptions, ScriptRun};
use crate::system;

pub const STAGE_COUNT: usize = 8;
pub const MINIMUM_FREE_GB: f64 = 30.0;

/// Where to find `install.ps1`: the extracted kit first, then a repository checkout above the launcher (development).
pub fn find_install_script(kit_dir: &Path, launcher_dir: &Path) -> Option<PathBuf> {
    let name = if cfg!(windows) { "install.ps1" } else { "install.sh" };
    let in_kit = kit_dir.join(name);
    if in_kit.is_file() {
        return Some(in_kit);
    }
    let mut directory = Some(launcher_dir);
    for _ in 0..6 {
        let current = directory?;
        let candidate = current.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        directory = current.parent();
    }
    None
}

/// `true` if the kit carries the VU1 microcode manifest: then the installer builds the micro memories from the ELF and
/// does not need to open the game to capture them.
pub fn has_manifest(script: &Path) -> bool {
    script.parent().is_some_and(|root| root.join("RustPortPS2/games/scus-97399/vu1.manifest").exists())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Environment {
    /// The C++ toolchain: Microsoft's compiler (Build Tools or Visual Studio) on Windows; on Linux, compiler, CMake,
    /// Ninja and the development libraries (`install.sh --check-deps` found nothing missing).
    pub vs: bool,
    /// Windows: there is a Visual Studio but no CMake that can configure the build, neither in PATH nor bundled with it (an old
    /// Visual Studio bundles an old one). The wizard then offers Visual Studio 2022 Build Tools next to it; when a usable CMake
    /// exists, the installer finds it by itself and nothing is offered.
    pub cmake_unusable: bool,
    pub git: bool,
    /// `install.ps1` / `install.sh` found.
    pub installer: bool,
    /// Linux: what `install.sh --check-deps` says is missing (programs and libraries), empty when all is there.
    pub missing: Vec<String>,
    /// Linux: the packages that provide them, for this distribution (empty if it is not a known one).
    pub missing_packages: Vec<String>,
    /// Linux: the command that installs those packages, to run in a terminal.
    pub install_command: String,
    /// Windows x86-64: the CPU features the game needs that this processor lacks (see [`REQUIRED_CPU_FEATURE`]); empty when
    /// all are there, on Linux (the build uses `-march=native` there) and on other architectures.
    pub cpu_missing: Vec<String>,
    /// The installation kit files (relative to the kit folder) that are gone and could not be restored from the bundled
    /// copy; empty when all are there. Filled by the application, which knows the kit folder.
    pub kit_missing: Vec<String>,
}

/// What `install.sh --check-deps` printed: the missing checks, their packages and the command that installs them.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DependencyReport {
    pub missing: Vec<String>,
    pub packages: Vec<String>,
    pub install_command: String,
}

pub fn parse_dependency_report(output: &str) -> DependencyReport {
    let words = |rest: &str| rest.split_whitespace().map(str::to_string).collect::<Vec<_>>();
    let mut report = DependencyReport::default();
    for line in output.lines() {
        if let Some(rest) = line.strip_prefix("[missing]") {
            report.missing = words(rest);
        } else if let Some(rest) = line.strip_prefix("[packages]") {
            report.packages = words(rest);
        } else if let Some(rest) = line.strip_prefix("[install-command]") {
            report.install_command = rest.trim().to_string();
        }
    }
    report
}

/// Linux: asks the installer what is missing. If it cannot answer, nothing is reported missing and the installation
/// itself stops with the same message.
fn linux_dependencies(script: &Path) -> DependencyReport {
    let mut command = Command::new("bash");
    command.arg(script).arg("--check-deps");
    system::run_with_timeout(command, Duration::from_secs(20))
        .map(|(_, output)| parse_dependency_report(&output))
        .unwrap_or_default()
}

/// `true` if the output of `cmake --version` (`cmake version 3.31.12`) is 3.21 or newer, the least the build requires.
pub fn cmake_version_is_enough(output: &str) -> bool {
    let version = output.lines().next().unwrap_or("").rsplit(' ').next().unwrap_or("");
    let mut parts = version.trim().split('.').map(|part| part.parse::<u32>().ok());
    match (parts.next().flatten(), parts.next().flatten()) {
        (Some(major), Some(minor)) => (major, minor) >= (3, 21),
        _ => false,
    }
}

/// Windows: whether this `cmake.exe` can configure the build, with the same rule as `install.ps1`'s `CmakeUsable`: 3.21 or
/// newer, and `cmcldeps.exe` next to it (a CMake from another toolchain can lack it, and the resource compiler then fails).
fn cmake_is_usable(cmake: &Path) -> bool {
    if !cmake.parent().is_some_and(|directory| directory.join("cmcldeps.exe").is_file()) {
        return false;
    }
    let mut command = Command::new(cmake);
    command.arg("--version");
    system::output_with_timeout(command, Duration::from_secs(8)).is_some_and(|out| cmake_version_is_enough(&out))
}

/// The one CPU requirement of the Windows build, named as the interface shows it. The generated code and the runtime use
/// SSE4.1 intrinsics (`_mm_extract_epi*`, `_mm_blendv_*`, `_mm_round_*`, `_mm_testz_*`...) and nothing newer: no POPCNT,
/// SSE4.2, AVX or BMI. If AVX2 ever becomes a requirement (`PS2X_VU1_AVX2`, `PS2X_FAST_MATH_AVX2`), add it here and in the
/// script's check; `install.ps1` repeats this requirement with `IsProcessorFeaturePresent`.
pub const REQUIRED_CPU_FEATURE: &str = "SSE4.1";

/// What the CPU is missing, given whether it has the feature: `None` means detection could not run, and then nothing is
/// reported (fail open: a wrong block is worse than a game that does not start).
pub fn missing_cpu_features(has_required: Option<bool>) -> Vec<String> {
    match has_required {
        Some(false) => vec![REQUIRED_CPU_FEATURE.to_string()],
        _ => Vec::new(),
    }
}

/// Windows x86-64 only: the runtime detection of the standard library; anywhere else there is nothing to check.
fn detect_required_cpu_feature() -> Option<bool> {
    #[cfg(all(windows, target_arch = "x86_64"))]
    {
        Some(std::arch::is_x86_feature_detected!("sse4.1"))
    }
    #[cfg(not(all(windows, target_arch = "x86_64")))]
    {
        None
    }
}

pub fn check_environment(script: Option<&Path>) -> Environment {
    let mut environment = Environment {
        vs: false,
        cmake_unusable: false,
        git: false,
        installer: script.is_some(),
        missing: Vec::new(),
        missing_packages: Vec::new(),
        install_command: String::new(),
        cpu_missing: missing_cpu_features(detect_required_cpu_feature()),
        kit_missing: Vec::new(),
    };
    if !cfg!(windows) {
        let report = script.map(linux_dependencies).unwrap_or_default();
        environment.vs = report.missing.is_empty();
        environment.git = true;
        environment.missing = report.missing;
        environment.missing_packages = report.packages;
        environment.install_command = report.install_command;
    } else {
        if let Some(program_files) = std::env::var_os("ProgramFiles(x86)") {
            let vswhere = Path::new(&program_files).join("Microsoft Visual Studio/Installer/vswhere.exe");
            if vswhere.is_file() {
                // Every installation, the newest first: a newer Visual Studio without the CMake component (issue #9) must not
                // hide the CMake of a Build Tools 2022 installed next to it, as in install.ps1.
                let mut command = Command::new(&vswhere);
                command.args([
                    "-products",
                    "*",
                    "-requires",
                    "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
                    "-property",
                    "installationPath",
                ]);
                let installations: Vec<String> = system::output_with_timeout(command, Duration::from_secs(8))
                    .map(|out| {
                        out.lines().map(|line| line.trim().to_string()).filter(|line| !line.is_empty()).collect()
                    })
                    .unwrap_or_default();
                environment.vs = !installations.is_empty();
                // GOW-Port: the installer uses the CMake in PATH or the one bundled with Visual Studio, whichever works
                // (install.ps1 explains it); only when neither does is something offered to install.
                if environment.vs {
                    let bundled = installations.iter().map(|path| {
                        Path::new(path).join(r"Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe")
                    });
                    let in_path = std::env::var_os("PATH")
                        .map(|path| {
                            std::env::split_paths(&path)
                                .map(|directory| directory.join("cmake.exe"))
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    environment.cmake_unusable = !in_path
                        .into_iter()
                        .chain(bundled)
                        .filter(|candidate| candidate.is_file())
                        .any(|cmake| cmake_is_usable(&cmake));
                }
            }
        }
        environment.git =
            system::find_in_path("git").is_some() || Path::new(r"C:\Program Files\Git\cmd\git.exe").exists();
    }
    environment
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DestinationReason {
    Empty,
    Characters,
    Space,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DestinationValidation {
    pub ok: bool,
    pub reason: Option<DestinationReason>,
    pub free_gb: f64,
}

pub fn validate_destination(directory: &str) -> DestinationValidation {
    let clean = directory.trim();
    if clean.is_empty() {
        return DestinationValidation { ok: false, reason: Some(DestinationReason::Empty), free_gb: 0.0 };
    }
    if !system::is_ascii_path(clean) {
        return DestinationValidation { ok: false, reason: Some(DestinationReason::Characters), free_gb: 0.0 };
    }
    let free_gb = system::free_space_gb(Path::new(clean));
    if free_gb < MINIMUM_FREE_GB {
        return DestinationValidation { ok: false, reason: Some(DestinationReason::Space), free_gb };
    }
    DestinationValidation { ok: true, reason: None, free_gb }
}

pub fn default_destination() -> String {
    let home = paths::default_destination();
    if system::is_ascii_path(&home.to_string_lossy()) {
        home.to_string_lossy().into_owned()
    } else {
        "C:/GodOfWar-PC".to_string()
    }
}

/// Stage titles of `install.ps1` ("==> ..."), only their ASCII fragments. Several fragments share a stage.
const STAGES: [(&str, usize); 11] = [
    ("Installing", 0),
    ("Downloading", 0),
    ("Checking tools", 0),
    ("Game data", 1),
    ("VU1 microcode from the ELF", 2),
    ("Configuring the build", 2),
    ("Building the recompiler", 3),
    ("Translating", 4),
    ("Building the game", 5),
    ("Optimization", 6),
    ("Done", 7),
];

pub fn stage_of(title: &str) -> Option<usize> {
    STAGES.iter().find(|(fragment, _)| title.contains(fragment)).map(|(_, stage)| *stage)
}

/// `[12/340] ...` -> 12/340 (what ninja prints).
pub fn ninja_progress(line: &str) -> Option<f64> {
    let rest = line.strip_prefix('[')?;
    let (done, rest) = rest.split_once('/')?;
    let (total, _) = rest.split_once(']')?;
    let (done, total): (u32, u32) = (done.parse().ok()?, total.parse().ok()?);
    (total > 0).then(|| done as f64 / total as f64)
}

/// Snapshot of the installation sent to the interface.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallProgress {
    /// 0-7, -1 not started.
    pub stage: i32,
    /// 0-1 inside the stage, -1 indeterminate.
    pub stage_progress: f64,
    /// The last six output lines.
    pub recent_lines: Vec<String>,
}

impl Default for InstallProgress {
    fn default() -> Self {
        Self { stage: -1, stage_progress: -1.0, recent_lines: Vec::new() }
    }
}

/// Folds output lines into an [`InstallProgress`] and remembers the last `[error]` line.
#[derive(Default)]
pub struct ProgressTracker {
    pub progress: InstallProgress,
    pub last_error: String,
}

impl ProgressTracker {
    /// Returns `true` if the snapshot changed.
    pub fn feed(&mut self, line: &str) -> bool {
        if let Some(error) = line.strip_prefix("[error]") {
            self.last_error = error.trim().to_string();
        }
        if line.starts_with("==> ") {
            if let Some(stage) = stage_of(line) {
                self.progress.stage = self.progress.stage.max(stage as i32);
                self.progress.stage_progress = -1.0;
            }
        }
        if let Some(fraction) = ninja_progress(line) {
            self.progress.stage_progress = fraction;
        }
        self.progress.recent_lines.push(line.to_string());
        if self.progress.recent_lines.len() > 6 {
            self.progress.recent_lines.remove(0);
        }
        true
    }

    /// Text to show when the script failed.
    pub fn failure_detail(&self, code: Option<i32>) -> String {
        if !self.last_error.is_empty() {
            return self.last_error.clone();
        }
        if !self.progress.recent_lines.is_empty() {
            return self.progress.recent_lines.join("\n");
        }
        format!("The installer finished with exit code {}.", code.map_or("?".to_string(), |c| c.to_string()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallOutcome {
    pub ok: bool,
    pub cancelled: bool,
    pub error: String,
    pub elf: String,
    pub executable: String,
    /// The interface string that explains a recognized failure (see [`failure::FailureKind`]), shown above the raw text.
    pub failure_kind: Option<String>,
}

/// Paths the installer leaves behind inside `destination`.
pub fn installed_paths(destination: &str) -> (PathBuf, PathBuf) {
    let root = Path::new(destination);
    (root.join("game/SCUS_973.99"), root.join("build/ps2xRuntime").join(EXECUTABLE_NAME))
}

fn outcome_for(destination: &str, code: Option<i32>, cancelled: bool, tracker: &ProgressTracker) -> InstallOutcome {
    let failure = |error: String| InstallOutcome {
        ok: false,
        cancelled: false,
        error,
        elf: String::new(),
        executable: String::new(),
        failure_kind: None,
    };
    if cancelled {
        return InstallOutcome { cancelled: true, ..failure(String::new()) };
    }
    let (elf, executable) = installed_paths(destination);
    if code == Some(0) {
        if elf.is_file() && executable.is_file() {
            return InstallOutcome {
                ok: true,
                cancelled: false,
                error: String::new(),
                elf: elf.to_string_lossy().into_owned(),
                executable: executable.to_string_lossy().into_owned(),
                failure_kind: None,
            };
        }
        return failure(format!(
            "The installation finished, but the expected files were not found:\n{}\n{}",
            elf.display(),
            executable.display()
        ));
    }
    InstallOutcome {
        failure_kind: failure::classify_install_failure(&tracker.last_error, &tracker.progress.recent_lines)
            .map(|kind| kind.key().to_string()),
        ..failure(tracker.failure_detail(code))
    }
}

/// A running installation.
pub struct Installation {
    run: ScriptRun,
    cancelled: Arc<Mutex<bool>>,
}

impl Installation {
    pub fn cancel(&self) {
        *self.cancelled.lock().unwrap() = true;
        self.run.kill_tree();
    }
}

pub struct InstallRequest {
    pub script: PathBuf,
    pub iso: String,
    pub destination: String,
    pub install_tools: bool,
    /// Maximum RAM profile for the installer and the VU1 AOT generator (4 or 8 GiB).
    pub vu1_memory_limit_gb: u8,
    pub log_file: PathBuf,
}

/// The arguments of the installer: `install.ps1`'s on Windows, `install.sh`'s on Linux (which has no RAM profile: it
/// takes the memory of the machine into account by itself).
pub fn install_arguments(request: &InstallRequest, destination: &str, windows: bool) -> Vec<String> {
    if windows {
        let mut arguments = vec![
            "-Iso".to_string(),
            quote_for_powershell(request.iso.trim()),
            "-Destination".to_string(),
            quote_for_powershell(destination),
            "-NoPause".to_string(),
            "-Vu1MemoryLimitGB".to_string(),
            request.vu1_memory_limit_gb.clamp(4, 8).to_string(),
        ];
        if request.install_tools {
            arguments.push("-InstallTools".to_string());
        }
        arguments
    } else {
        vec![request.iso.trim().to_string(), "--destination".to_string(), destination.to_string()]
    }
}

/// Starts `install.ps1` / `install.sh`. `on_progress` is called for every output line; `on_done` once, when the script
/// ends.
pub fn start(
    request: InstallRequest,
    on_progress: impl Fn(&InstallProgress) + Send + 'static,
    on_done: impl FnOnce(InstallOutcome) + Send + 'static,
) -> std::io::Result<Installation> {
    let destination = if cfg!(windows) {
        request.destination.trim().replace('/', "\\")
    } else {
        request.destination.trim().to_string()
    };
    let arguments = install_arguments(&request, &destination, cfg!(windows));
    if let Some(parent) = request.log_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let log = Arc::new(Mutex::new(std::fs::File::create(&request.log_file)?));
    let tracker = Arc::new(Mutex::new(ProgressTracker::default()));
    let cancelled = Arc::new(Mutex::new(false));

    let (line_tracker, line_log) = (Arc::clone(&tracker), Arc::clone(&log));
    let (exit_tracker, exit_cancelled) = (Arc::clone(&tracker), Arc::clone(&cancelled));
    let run = script::run(
        ScriptOptions { script: request.script, arguments, below_normal_priority: false },
        move |line| {
            use std::io::Write;
            let _ = writeln!(line_log.lock().unwrap(), "{line}");
            let mut tracker = line_tracker.lock().unwrap();
            if tracker.feed(line) {
                on_progress(&tracker.progress);
            }
        },
        move |code| {
            let outcome =
                outcome_for(&destination, code, *exit_cancelled.lock().unwrap(), &exit_tracker.lock().unwrap());
            on_done(outcome);
        },
    )?;
    Ok(Installation { run, cancelled })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cpu_without_the_required_feature_is_reported_and_detection_failing_reports_nothing() {
        assert_eq!(missing_cpu_features(Some(false)), ["SSE4.1"]);
        assert!(missing_cpu_features(Some(true)).is_empty());
        // Fail open: no answer means no block.
        assert!(missing_cpu_features(None).is_empty());
    }

    #[test]
    fn the_dependency_report_of_install_sh_is_read() {
        let report = parse_dependency_report(
            "Checking tools\n[missing] ninja alsa\n[packages] ninja-build libasound2-dev\n[install-command] sudo apt install ninja-build libasound2-dev\n",
        );
        assert_eq!(report.missing, ["ninja", "alsa"]);
        assert_eq!(report.packages, ["ninja-build", "libasound2-dev"]);
        assert_eq!(report.install_command, "sudo apt install ninja-build libasound2-dev");
        // An unknown distribution reports what is missing but no packages.
        let unknown = parse_dependency_report("[missing] g++\n");
        assert_eq!((unknown.missing.len(), unknown.packages.len(), unknown.install_command.as_str()), (1, 0, ""));
        assert_eq!(parse_dependency_report(""), DependencyReport::default());
    }

    #[test]
    fn the_build_needs_cmake_3_21_or_newer() {
        assert!(cmake_version_is_enough(
            "cmake version 3.31.12\r\n\r\nCMake suite maintained and supported by Kitware"
        ));
        assert!(cmake_version_is_enough("cmake version 3.21.0"));
        assert!(cmake_version_is_enough("cmake version 4.4.4"));
        assert!(cmake_version_is_enough("cmake version 4.0.0-rc1"));
        assert!(!cmake_version_is_enough("cmake version 3.20.6"));
        assert!(!cmake_version_is_enough("cmake version 3.12.18081601-MSVC_2"));
        assert!(!cmake_version_is_enough(""));
        assert!(!cmake_version_is_enough("garbage"));
    }

    #[test]
    fn each_platform_gets_the_arguments_of_its_installer() {
        let request = InstallRequest {
            script: PathBuf::from("install"),
            iso: " /home/me/God of War.iso ".into(),
            destination: String::new(),
            install_tools: true,
            vu1_memory_limit_gb: 12,
            log_file: PathBuf::new(),
        };
        assert_eq!(
            install_arguments(&request, r"C:\GodOfWar-PC", true),
            [
                "-Iso",
                "'/home/me/God of War.iso'",
                "-Destination",
                r"'C:\GodOfWar-PC'",
                "-NoPause",
                "-Vu1MemoryLimitGB",
                "8",
                "-InstallTools"
            ]
        );
        assert_eq!(
            install_arguments(&request, "/home/me/gow", false),
            ["/home/me/God of War.iso", "--destination", "/home/me/gow"]
        );
    }

    #[test]
    fn titles_map_to_stages() {
        assert_eq!(stage_of("==> Checking tools"), Some(0));
        assert_eq!(stage_of("==> Game data: extracting the disc"), Some(1));
        assert_eq!(stage_of("==> Configuring the build"), Some(2));
        assert_eq!(stage_of("==> Building the recompiler"), Some(3));
        assert_eq!(stage_of("==> Translating the ELF"), Some(4));
        assert_eq!(stage_of("==> Building the game"), Some(5));
        assert_eq!(stage_of("==> Optimization (first run)"), Some(6));
        assert_eq!(stage_of("==> Done"), Some(7));
        assert_eq!(stage_of("==> Something else"), None);
    }

    #[test]
    fn ninja_lines_give_a_fraction() {
        assert_eq!(ninja_progress("[50/200] Building CXX object x.o"), Some(0.25));
        assert_eq!(ninja_progress("[0/0] x"), None);
        assert_eq!(ninja_progress("[a/2] x"), None);
        assert_eq!(ninja_progress("no brackets"), None);
    }

    #[test]
    fn the_tracker_only_moves_the_stage_forward() {
        let mut tracker = ProgressTracker::default();
        tracker.feed("==> Building the game");
        assert_eq!(tracker.progress.stage, 5);
        tracker.feed("[10/100] CXX");
        assert_eq!(tracker.progress.stage_progress, 0.1);
        tracker.feed("==> Game data");
        assert_eq!(tracker.progress.stage, 5);
        tracker.feed("==> Optimization");
        assert_eq!((tracker.progress.stage, tracker.progress.stage_progress), (6, -1.0));
    }

    #[test]
    fn only_the_last_six_lines_are_kept() {
        let mut tracker = ProgressTracker::default();
        for index in 0..10 {
            tracker.feed(&format!("line {index}"));
        }
        assert_eq!(tracker.progress.recent_lines.len(), 6);
        assert_eq!(tracker.progress.recent_lines[0], "line 4");
    }

    #[test]
    fn the_failure_detail_prefers_the_error_line() {
        let mut tracker = ProgressTracker::default();
        assert_eq!(tracker.failure_detail(Some(2)), "The installer finished with exit code 2.");
        tracker.feed("something");
        assert_eq!(tracker.failure_detail(Some(1)), "something");
        tracker.feed("[error] cmake not found");
        assert_eq!(tracker.failure_detail(Some(1)), "cmake not found");
    }

    #[test]
    fn destination_rules() {
        assert_eq!(validate_destination("  ").reason, Some(DestinationReason::Empty));
        assert_eq!(validate_destination(r"C:\Usuários\x").reason, Some(DestinationReason::Characters));
        let directory = tempfile::tempdir().unwrap();
        let result = validate_destination(&directory.path().join("game").to_string_lossy());
        assert!(result.free_gb > 0.0);
        assert_eq!(result.ok, result.free_gb >= MINIMUM_FREE_GB);
    }

    #[test]
    fn the_outcome_requires_the_expected_files() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().to_string_lossy().into_owned();
        let tracker = ProgressTracker::default();
        assert!(outcome_for(&destination, None, true, &tracker).cancelled);
        let missing = outcome_for(&destination, Some(0), false, &tracker);
        assert!(!missing.ok && missing.error.contains("expected files"));
        let (elf, executable) = installed_paths(&destination);
        std::fs::create_dir_all(elf.parent().unwrap()).unwrap();
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        std::fs::write(&elf, b"x").unwrap();
        std::fs::write(&executable, b"x").unwrap();
        let done = outcome_for(&destination, Some(0), false, &tracker);
        assert!(done.ok && done.elf.ends_with("SCUS_973.99"));
        assert!(!outcome_for(&destination, Some(1), false, &tracker).ok);
    }

    #[test]
    fn a_recognized_failure_carries_its_hint_and_an_unknown_one_does_not() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().to_string_lossy().into_owned();
        let mut tracker = ProgressTracker::default();
        tracker.feed("[error] ninja ended with code 1");
        assert_eq!(outcome_for(&destination, Some(1), false, &tracker).failure_kind, None);
        tracker
            .feed("[error] Could not download https://example.org/x.zip (timed out). Check the Internet connection.");
        let outcome = outcome_for(&destination, Some(1), false, &tracker);
        assert_eq!(outcome.failure_kind.as_deref(), Some("failureDownload"));
        // The raw text stays.
        assert!(outcome.error.starts_with("Could not download"));
    }

    #[test]
    fn the_script_is_found_in_the_kit_or_above_the_launcher() {
        let root = tempfile::tempdir().unwrap();
        let name = if cfg!(windows) { "install.ps1" } else { "install.sh" };
        let launcher = root.path().join("repo/gow-launcher/target");
        std::fs::create_dir_all(&launcher).unwrap();
        let kit = root.path().join("kit");
        assert_eq!(find_install_script(&kit, &launcher), None);
        std::fs::write(root.path().join("repo").join(name), "").unwrap();
        assert_eq!(find_install_script(&kit, &launcher), Some(root.path().join("repo").join(name)));
        std::fs::create_dir_all(&kit).unwrap();
        std::fs::write(kit.join(name), "").unwrap();
        assert_eq!(find_install_script(&kit, &launcher), Some(kit.join(name)));
        assert!(!has_manifest(&kit.join(name)));
        std::fs::create_dir_all(kit.join("RustPortPS2/games/scus-97399")).unwrap();
        std::fs::write(kit.join("RustPortPS2/games/scus-97399/vu1.manifest"), "").unwrap();
        assert!(has_manifest(&kit.join(name)));
    }
}
