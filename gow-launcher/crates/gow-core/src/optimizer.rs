//! Progressive optimizer: the game records each new VU1 microprogram that appears while playing
//! (`GOW_VU1_CAPTURE`) and, in the background, `install.ps1` rebuilds the executable with those microprograms already
//! as native code. The game always runs from a copy (`<destination>/active/`), so the rebuild can finish at any time
//! and the new copy only takes effect on the next start. The microprograms are game data and stay on the user's PC.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::game::EXECUTABLE_NAME;
use crate::installer::ninja_progress;
use crate::rebuild;
use crate::script::{self, quote_for_powershell, ScriptOptions, ScriptRun};

pub const CHECK_INTERVAL: Duration = Duration::from_secs(30);
/// New microprograms that justify rebuilding while the game is open (after it closes, any of them does).
pub const THRESHOLD_DURING_GAME: u32 = 40;
pub const THRESHOLD_AFTER_GAME: u32 = 1;
pub const WAIT_AFTER_FAILURE: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OptimizerStatus {
    Disabled,
    Idle,
    Compiling,
    Error,
}

/// What the interface shows.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OptimizerSnapshot {
    pub configured: bool,
    pub compiling: bool,
    pub captured: u32,
    pub compiled: u32,
    /// Fraction of the microprograms already seen that are compiled (0-1).
    pub completion_ratio: f64,
    /// Progress of the rebuild in progress (0-1, -1 unknown).
    pub build_progress: f64,
    pub status: OptimizerStatus,
    /// The run in progress is the speed-profile optimization ("Optimize now": the game opens by itself, see [`Optimizer::profile_now`]).
    pub profiling: bool,
    /// The installation has a speed profile (PGO), so its builds use it.
    pub profile_ready: bool,
    /// This installation can have one: Windows, built with clang.
    pub profile_supported: bool,
}

/// GOW-Port: the speed profile (clang PGO) of an installation, trained on this PC by install.ps1 -Pgo on and never shipped.
pub fn profile_path(destination: &Path) -> PathBuf {
    destination.join("build").join("pgo").join("gow.profdata")
}

/// Whether the installation's build is made with clang (the only compiler install.ps1 trains a profile for), from its CMake
/// cache. Windows only: install.sh has no PGO.
pub fn profile_supported(destination: &Path) -> bool {
    if !cfg!(windows) {
        return false;
    }
    fs::read_to_string(destination.join("build").join("CMakeCache.txt")).is_ok_and(|cache| {
        cache.lines().any(|line| line.starts_with("CMAKE_CXX_COMPILER:") && line.to_lowercase().contains("clang"))
    })
}

/// The installer arguments of "Optimize now": the optimization run with the game opening by itself, which records the VU1
/// microcode, the IOP modules and the speed profile, then one build that uses them. `None` where the installer cannot do it.
pub fn profile_arguments(destination: &str, windows: bool) -> Option<Vec<String>> {
    windows.then(|| {
        vec![
            "-Destination".to_string(),
            quote_for_powershell(destination),
            "-NoPause".to_string(),
            "-Optimize".to_string(),
            "-Pgo".to_string(),
            "on".to_string(),
        ]
    })
}

/// How many microprograms are recorded in `<destination>/vu1_captures`.
pub fn count_captures(destination: &Path) -> u32 {
    fs::read_dir(destination.join("vu1_captures"))
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| {
                    let name = entry.file_name().to_string_lossy().to_lowercase();
                    name.starts_with("vu1_") && name.ends_with(".bin") && entry.path().is_file()
                })
                .count() as u32
        })
        .unwrap_or(0)
}

fn compiled_executable(destination: &Path) -> PathBuf {
    destination.join("build/ps2xRuntime").join(EXECUTABLE_NAME)
}

fn active_executable(destination: &Path) -> PathBuf {
    destination.join("active").join(EXECUTABLE_NAME)
}

/// Copies `source` to `target` keeping the modification time and the ELF fingerprint next to it.
fn copy_with_fingerprint(source: &Path, target: &Path) -> std::io::Result<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let _ = fs::remove_file(target);
    fs::copy(source, target)?;
    if let Ok(modified) = fs::metadata(source).and_then(|m| m.modified()) {
        if let Ok(file) = File::options().write(true).open(target) {
            let _ = file.set_modified(modified);
        }
    }
    let fingerprint = source.with_file_name("game-elf.sha256");
    if fingerprint.exists() {
        let target_fingerprint = target.with_file_name("game-elf.sha256");
        let _ = fs::remove_file(&target_fingerprint);
        let _ = fs::copy(&fingerprint, &target_fingerprint);
    }
    Ok(())
}

/// Copies build -> active if the build is newer and the active copy is not in use. Returns `true` if a new version is
/// pending (it takes effect on the next start).
pub fn sync_active_executable(destination: &Path, game_running: bool) -> bool {
    let build = compiled_executable(destination);
    let active = active_executable(destination);
    let Ok(build_meta) = fs::metadata(&build) else {
        return false;
    };
    if let Ok(active_meta) = fs::metadata(&active) {
        if active_meta.len() == build_meta.len() && active_meta.modified().ok() == build_meta.modified().ok() {
            return false;
        }
    }
    if game_running {
        return true;
    }
    copy_with_fingerprint(&build, &active).is_err()
}

/// Called when the wizard finishes: copies the newly built executable to `<destination>/active` and returns the path
/// of the copy (which is what the launcher now runs). The microprograms captured at installation count as compiled.
pub fn register_installation(compiled: &Path, destination: &Path) -> PathBuf {
    let active = active_executable(destination);
    let _ = fs::create_dir_all(destination.join("vu1_captures"));
    match copy_with_fingerprint(compiled, &active) {
        Ok(()) => active,
        Err(_) => compiled.to_path_buf(),
    }
}

struct State {
    destination: Option<PathBuf>,
    enabled: bool,
    during_game: bool,
    game_running: bool,
    captured: u32,
    compiled: u32,
    compiled_at_startup: u32,
    build_progress: f64,
    has_error: bool,
    last_failure: Option<Instant>,
    run: Option<ScriptRun>,
    /// The current run is being stopped on purpose (the game opened): not a failure.
    stopping: bool,
    /// A rebuild was stopped because the game opened; it starts again when the game closes.
    resume_after_game: bool,
    /// The current run is "Optimize now" (it opens the game by itself): a game started by the player cancels it, for good.
    profiling: bool,
}

type Notify = Arc<dyn Fn(&OptimizerSnapshot) + Send + Sync>;

#[derive(Clone)]
pub struct Optimizer {
    state: Arc<Mutex<State>>,
    script: Arc<Mutex<Option<PathBuf>>>,
    log_file: PathBuf,
    notify: Notify,
    compiled_changed: Arc<dyn Fn(u32) + Send + Sync>,
}

impl State {
    fn configured(&self) -> bool {
        self.enabled && self.destination.is_some()
    }

    fn snapshot(&self) -> OptimizerSnapshot {
        let compiling = self.run.is_some();
        OptimizerSnapshot {
            configured: self.configured(),
            compiling,
            captured: self.captured,
            compiled: self.compiled,
            completion_ratio: if self.captured == 0 {
                1.0
            } else {
                (self.compiled as f64 / self.captured as f64).min(1.0)
            },
            build_progress: self.build_progress,
            profiling: compiling && self.profiling,
            profile_ready: self.destination.as_deref().is_some_and(|d| profile_path(d).is_file()),
            profile_supported: self.destination.as_deref().is_some_and(profile_supported),
            status: if !self.configured() {
                OptimizerStatus::Disabled
            } else if compiling {
                OptimizerStatus::Compiling
            } else if self.has_error {
                OptimizerStatus::Error
            } else {
                OptimizerStatus::Idle
            },
        }
    }
}

/// The arguments of the installer for a rebuild without the optimization pass: `install.ps1`'s on Windows, `install.sh`'s
/// on Linux. `jobs` 0 leaves the number of build processes to the installer.
pub fn optimizer_arguments(destination: &str, jobs: usize, windows: bool) -> Vec<String> {
    if windows {
        vec![
            "-Destination".to_string(),
            quote_for_powershell(destination),
            "-NoPause".to_string(),
            "-NoOptimize".to_string(),
            "-Jobs".to_string(),
            jobs.to_string(),
        ]
    } else {
        let mut arguments = vec!["--destination".to_string(), destination.to_string(), "--no-optimize".to_string()];
        if jobs > 0 {
            arguments.extend(["--jobs".to_string(), jobs.to_string()]);
        }
        arguments
    }
}

/// Whether a rebuild should start now. Pure, so the policy can be tested.
pub fn should_rebuild(pending: u32, game_running: bool, during_game: bool, since_failure: Option<Duration>) -> bool {
    let threshold = if game_running { THRESHOLD_DURING_GAME } else { THRESHOLD_AFTER_GAME };
    let waiting = since_failure.is_some_and(|elapsed| elapsed < WAIT_AFTER_FAILURE);
    pending >= threshold && (!game_running || during_game) && !waiting
}

impl Optimizer {
    pub fn new(
        log_file: PathBuf,
        notify: impl Fn(&OptimizerSnapshot) + Send + Sync + 'static,
        compiled_changed: impl Fn(u32) + Send + Sync + 'static,
    ) -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                destination: None,
                enabled: false,
                during_game: true,
                game_running: false,
                captured: 0,
                compiled: 0,
                compiled_at_startup: 0,
                build_progress: -1.0,
                has_error: false,
                last_failure: None,
                run: None,
                stopping: false,
                resume_after_game: false,
                profiling: false,
            })),
            script: Arc::new(Mutex::new(None)),
            log_file,
            notify: Arc::new(notify),
            compiled_changed: Arc::new(compiled_changed),
        }
    }

    /// Where `install.ps1` is (the kit); without it a rebuild reports an error.
    pub fn set_script(&self, script: Option<PathBuf>) {
        *self.script.lock().unwrap() = script;
    }

    pub fn snapshot(&self) -> OptimizerSnapshot {
        self.state.lock().unwrap().snapshot()
    }

    fn changed(&self) {
        (self.notify)(&self.snapshot());
    }

    /// An empty destination or `enabled = false` turns everything off. `compiled`: how many microprograms the last
    /// rebuild included.
    pub fn configure(&self, destination: &str, enabled: bool, during_game: bool, compiled: u32) {
        {
            let mut state = self.state.lock().unwrap();
            state.destination = (!destination.trim().is_empty()).then(|| PathBuf::from(destination.trim()));
            state.enabled = enabled && state.destination.is_some();
            state.during_game = during_game;
            if state.run.is_none() {
                state.compiled = compiled;
            }
            state.captured = state.destination.as_deref().map_or(0, count_captures);
            if let Some(directory) = state.destination.as_ref().filter(|_| state.enabled) {
                let _ = fs::create_dir_all(directory.join("vu1_captures"));
            }
        }
        self.changed();
    }

    /// Folder where the game records the new microprograms; empty if the optimizer is off.
    pub fn capture_directory(&self) -> String {
        let state = self.state.lock().unwrap();
        match (&state.destination, state.configured()) {
            (Some(destination), true) => destination.join("vu1_captures").to_string_lossy().into_owned(),
            _ => String::new(),
        }
    }

    /// Before opening the game: if a rebuild finished since the last start, updates the active copy.
    ///
    /// A rebuild started while the game was closed keeps using every core the game needs, so it is stopped here, before the game
    /// process exists, and resumes when the game closes (unless the user asked for rebuilds during play).
    pub fn prepare_to_play(&self) {
        let destination = self.state.lock().unwrap().destination.clone();
        if let Some(destination) = destination {
            sync_active_executable(&destination, false);
        }
        self.pause_for_game();
    }

    pub fn game_state_changed(&self, running: bool) {
        {
            self.state.lock().unwrap().game_running = running;
        }
        if running {
            self.pause_for_game();
        } else {
            self.resume_after_game();
        }
        self.changed();
    }

    /// Stops a rebuild that would compete with the game. Only when the user did not opt into rebuilds during play: a rebuild
    /// that started while the game was closed would otherwise keep compiling for minutes with the game open (the game loses
    /// the CPU, stops entering its frame function and the picture freezes).
    fn pause_for_game(&self) {
        let mut state = self.state.lock().unwrap();
        if state.run.is_none() {
            return;
        }
        if state.profiling {
            // GOW-Port: "Optimize now" opens its own game window and must never share the PC with the player's game: it is
            // cancelled (its game included, a child of the installer) and not started again by itself, so a game the player did
            // not ask for never appears after they close theirs. The offer stays in the interface.
            state.stopping = true;
            state.resume_after_game = false;
            if let Some(run) = state.run.as_ref() {
                run.kill_tree();
            }
            return;
        }
        if state.during_game {
            return;
        }
        state.stopping = true;
        state.resume_after_game = true;
        if let Some(run) = state.run.as_ref() {
            run.kill_tree();
        }
    }

    /// Starts the rebuild that was stopped when the game opened. Only with the game closed and nothing running.
    fn resume_after_game(&self) {
        let due = {
            let mut state = self.state.lock().unwrap();
            if state.resume_after_game && !state.game_running && state.run.is_none() {
                state.resume_after_game = false;
                true
            } else {
                false
            }
        };
        if due {
            self.start();
        }
    }

    /// Registers the installation (see [`register_installation`]) and counts what was captured as compiled.
    pub fn registered(&self, destination: &str) {
        {
            let mut state = self.state.lock().unwrap();
            state.destination = Some(PathBuf::from(destination.trim()));
            state.enabled = true;
            let captured = count_captures(state.destination.as_deref().unwrap());
            state.captured = captured;
            state.compiled = captured;
            state.has_error = false;
        }
        let compiled = self.state.lock().unwrap().compiled;
        (self.compiled_changed)(compiled);
        self.changed();
    }

    pub fn optimize_now(&self) {
        let ready = {
            let mut state = self.state.lock().unwrap();
            // One rebuild at a time, and none with the game open unless the user asked for it.
            let ready = state.configured() && state.run.is_none() && (!state.game_running || state.during_game);
            if ready {
                state.last_failure = None;
            }
            ready
        };
        if ready {
            self.start();
        }
    }

    /// The player asked to rebuild the game from the kit's current sources (see `rebuild`). Unlike [`Self::optimize_now`] it
    /// does not need the background optimization to be switched on: the installation folder is all it needs. Returns `false`
    /// when it did not start (no installation folder, a rebuild already running, or the game open without the opt-in to
    /// rebuild while playing: the interface refuses that case first, this is the same rule held here).
    pub fn rebuild_now(&self) -> bool {
        let ready = {
            let mut state = self.state.lock().unwrap();
            let ready =
                state.destination.is_some() && state.run.is_none() && (!state.game_running || state.during_game);
            if ready {
                state.last_failure = None;
                state.captured = state.destination.as_deref().map_or(0, count_captures);
            }
            ready
        };
        if ready {
            self.start();
        }
        ready
    }

    /// GOW-Port: "Optimize now" (the speed profile): only with the game closed, nothing else running and a clang build on
    /// Windows. The game opens by itself for a few minutes, then the game is built with the profile. Returns `false` when it did
    /// not start.
    pub fn profile_now(&self) -> bool {
        let ready = {
            let mut state = self.state.lock().unwrap();
            let ready = state.run.is_none()
                && !state.game_running
                && state.destination.as_deref().is_some_and(profile_supported);
            if ready {
                state.last_failure = None;
                state.captured = state.destination.as_deref().map_or(0, count_captures);
            }
            ready
        };
        if ready {
            self.start_run(true);
        }
        ready
    }

    /// One tick of the 30-second timer.
    pub fn check(&self) {
        let start = {
            let mut state = self.state.lock().unwrap();
            if !state.configured() {
                return;
            }
            state.captured = state.destination.as_deref().map_or(0, count_captures);
            if state.run.is_some() {
                false
            } else {
                if !state.game_running {
                    if let Some(destination) = state.destination.clone() {
                        sync_active_executable(&destination, false);
                    }
                }
                let pending = state.captured.saturating_sub(state.compiled);
                // GOW-Port (0.3.0): a build of 0.2.x is only redone when the player asks (it is deleted and built from
                // scratch, and the interface warns about that first).
                let legacy = state.destination.as_deref().is_some_and(rebuild::legacy_build);
                !legacy
                    && should_rebuild(
                        pending,
                        state.game_running,
                        state.during_game,
                        state.last_failure.map(|t| t.elapsed()),
                    )
            }
        };
        self.changed();
        if start {
            self.start();
        }
    }

    /// Spawns the timer thread: a check every [`CHECK_INTERVAL`] while the optimizer lives.
    pub fn spawn_timer(&self) {
        let optimizer = self.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(CHECK_INTERVAL);
            optimizer.check();
        });
    }

    fn fail(&self) {
        {
            let mut state = self.state.lock().unwrap();
            state.has_error = true;
        }
        self.changed();
    }

    fn start(&self) {
        self.start_run(false);
    }

    fn start_run(&self, profile: bool) {
        if self.state.lock().unwrap().run.is_some() {
            return; // one rebuild at a time
        }
        let Some(script) = self.script.lock().unwrap().clone() else {
            self.fail();
            return;
        };
        let (destination, game_running) = {
            let mut state = self.state.lock().unwrap();
            let Some(destination) = state.destination.clone() else { return };
            state.compiled_at_startup = state.captured;
            state.build_progress = -1.0;
            state.has_error = false;
            (destination, state.game_running)
        };
        if let Some(parent) = self.log_file.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let log = match File::create(&self.log_file) {
            Ok(file) => Arc::new(Mutex::new(file)),
            Err(_) => {
                self.fail();
                return;
            }
        };
        let kit = script.parent().map(Path::to_path_buf);
        if let Some(kit) = &kit {
            let _ = writeln!(log.lock().unwrap(), "{}", rebuild::log_line(kit, &destination));
        }
        // GOW-Port (0.3.0): a build of 0.2.x (Visual Studio) is redone from scratch: it is moved aside first and only deleted
        // once the new build succeeds (see `finished`). The saves are not in it.
        if !profile {
            match rebuild::set_legacy_aside(&destination) {
                Ok(true) => {
                    let _ = writeln!(
                        log.lock().unwrap(),
                        "[rebuild] build of 0.2.x moved to {}: building from scratch",
                        rebuild::LEGACY_FOLDER
                    );
                }
                Ok(false) => {}
                Err(error) => {
                    let _ = writeln!(log.lock().unwrap(), "[rebuild] could not move the old build aside: {error}");
                    self.fail();
                    return;
                }
            }
        }
        // With the game open, few build processes and low priority; without it, the installer chooses.
        let jobs = if game_running {
            if std::thread::available_parallelism().map_or(1, |n| n.get()) >= 8 {
                2
            } else {
                1
            }
        } else {
            0
        };
        let mut arguments = if profile {
            match profile_arguments(&destination.to_string_lossy(), cfg!(windows)) {
                Some(arguments) => arguments,
                None => {
                    self.fail();
                    return;
                }
            }
        } else {
            optimizer_arguments(&destination.to_string_lossy(), jobs, cfg!(windows))
        };
        // Diagnostic: extra install.ps1 arguments (for example -ToolsExe) to test without the final environment.
        if let Ok(extra) = std::env::var("GOW_OPTIMIZER_EXTRA") {
            if !extra.is_empty() {
                arguments.push(extra);
            }
        }
        let on_line = self.clone();
        let on_exit = self.clone();
        let result = script::run(
            ScriptOptions { script, arguments, below_normal_priority: true },
            move |line| {
                let _ = writeln!(log.lock().unwrap(), "{line}");
                if let Some(fraction) = ninja_progress(line) {
                    let publish = {
                        let mut state = on_line.state.lock().unwrap();
                        let moved = fraction - state.build_progress >= 0.01 || fraction < state.build_progress;
                        if moved {
                            state.build_progress = fraction;
                        }
                        moved
                    };
                    if publish {
                        on_line.changed();
                    }
                }
            },
            move |code| on_exit.finished(code, &destination, kit.as_deref()),
        );
        match result {
            Ok(run) => {
                {
                    let mut state = self.state.lock().unwrap();
                    state.run = Some(run);
                    state.profiling = profile;
                }
                self.changed();
            }
            Err(_) => self.fail(),
        }
    }

    fn finished(&self, code: Option<i32>, destination: &Path, kit: Option<&Path>) {
        let stopped = {
            let mut state = self.state.lock().unwrap();
            let stopped = state.stopping;
            state.stopping = false;
            state.profiling = false;
            stopped
        };
        if stopped {
            // Stopped on purpose because the game opened: not a failure, and the same pending work resumes afterwards. The
            // microprograms captured before the stop stay on disk, so nothing is lost.
            {
                let mut state = self.state.lock().unwrap();
                state.run = None;
                state.build_progress = -1.0;
            }
            self.changed();
            self.resume_after_game();
            return;
        }
        let succeeded = code == Some(0) && compiled_executable(destination).is_file();
        let compiled = {
            let mut state = self.state.lock().unwrap();
            state.run = None;
            state.build_progress = -1.0;
            if succeeded {
                state.compiled = state.compiled_at_startup;
                state.has_error = false;
                state.last_failure = None;
            } else {
                state.has_error = true;
                state.last_failure = Some(Instant::now());
            }
            succeeded.then_some(state.compiled)
        };
        if compiled.is_none() {
            // GOW-Port (0.3.0): a clean rebuild of a 0.2.x build that failed puts the old build back, as it was.
            let _ = rebuild::restore_legacy(destination);
        }
        if let Some(compiled) = compiled {
            // GOW-Port (0.3.0): the old build of a clean rebuild is no longer needed; deleting several GB takes a while.
            let old = destination.to_path_buf();
            std::thread::spawn(move || {
                let _ = rebuild::drop_legacy(&old);
            });
            // The build now holds the kit's sources: say so, so the launcher stops offering a rebuild.
            if let Some(kit) = kit {
                rebuild::record_build(kit, destination);
            }
            (self.compiled_changed)(compiled);
            sync_active_executable(destination, self.state.lock().unwrap().game_running);
        }
        self.changed();
        // If new microprograms arrived during the rebuild, reevaluate right away.
        let again = self.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(2));
            again.check();
        });
    }

    /// Stops a rebuild in progress (the launcher is closing).
    pub fn shutdown(&self) {
        if let Some(run) = self.state.lock().unwrap().run {
            run.kill_tree();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quiet(directory: &Path) -> Optimizer {
        Optimizer::new(directory.join("optimize.log"), |_| {}, |_| {})
    }

    #[test]
    fn each_platform_gets_the_arguments_of_its_installer() {
        assert_eq!(
            optimizer_arguments(r"C:\GodOfWar-PC", 2, true),
            ["-Destination", r"'C:\GodOfWar-PC'", "-NoPause", "-NoOptimize", "-Jobs", "2"]
        );
        assert_eq!(
            optimizer_arguments("/home/me/gow", 2, false),
            ["--destination", "/home/me/gow", "--no-optimize", "--jobs", "2"]
        );
        // install.sh would run zero build processes: with 0 jobs the flag is left out and the script chooses.
        assert_eq!(optimizer_arguments("/home/me/gow", 0, false), ["--destination", "/home/me/gow", "--no-optimize"]);
    }

    #[test]
    fn the_rebuild_policy() {
        let minute = Duration::from_secs(60);
        assert!(!should_rebuild(0, false, true, None));
        assert!(should_rebuild(1, false, true, None));
        assert!(!should_rebuild(39, true, true, None));
        assert!(should_rebuild(40, true, true, None));
        assert!(!should_rebuild(500, true, false, None)); // not during the game if the user said no
        assert!(!should_rebuild(5, false, true, Some(minute))); // cooling down after a failure
        assert!(should_rebuild(5, false, true, Some(WAIT_AFTER_FAILURE + minute)));
    }

    #[test]
    fn captures_are_counted_by_name() {
        let directory = tempfile::tempdir().unwrap();
        assert_eq!(count_captures(directory.path()), 0);
        let captures = directory.path().join("vu1_captures");
        fs::create_dir_all(&captures).unwrap();
        for name in ["vu1_0001.bin", "VU1_0002.BIN", "other.bin", "vu1_0003.txt"] {
            fs::write(captures.join(name), b"x").unwrap();
        }
        assert_eq!(count_captures(directory.path()), 2);
    }

    #[test]
    fn registering_copies_the_runner_and_its_fingerprint() {
        let directory = tempfile::tempdir().unwrap();
        let build = directory.path().join("build/ps2xRuntime");
        fs::create_dir_all(&build).unwrap();
        fs::write(build.join(EXECUTABLE_NAME), b"runner").unwrap();
        fs::write(build.join("game-elf.sha256"), b"abc").unwrap();
        let active = register_installation(&build.join(EXECUTABLE_NAME), directory.path());
        assert_eq!(active, directory.path().join("active").join(EXECUTABLE_NAME));
        assert_eq!(fs::read(&active).unwrap(), b"runner");
        assert_eq!(fs::read(directory.path().join("active/game-elf.sha256")).unwrap(), b"abc");
        // Same size and time: nothing pending.
        assert!(!sync_active_executable(directory.path(), false));
    }

    #[test]
    fn a_newer_build_is_pending_while_the_game_runs_and_copied_afterwards() {
        let directory = tempfile::tempdir().unwrap();
        let build = directory.path().join("build/ps2xRuntime");
        fs::create_dir_all(&build).unwrap();
        fs::write(build.join(EXECUTABLE_NAME), b"v1").unwrap();
        register_installation(&build.join(EXECUTABLE_NAME), directory.path());
        fs::write(build.join(EXECUTABLE_NAME), b"version two").unwrap();
        assert!(sync_active_executable(directory.path(), true));
        assert_eq!(fs::read(directory.path().join("active").join(EXECUTABLE_NAME)).unwrap(), b"v1");
        assert!(!sync_active_executable(directory.path(), false));
        assert_eq!(fs::read(directory.path().join("active").join(EXECUTABLE_NAME)).unwrap(), b"version two");
    }

    #[test]
    fn no_rebuild_is_started_while_the_game_is_open() {
        let directory = tempfile::tempdir().unwrap();
        let optimizer = quiet(directory.path());
        optimizer.configure(&directory.path().to_string_lossy(), true, false, 0);
        optimizer.game_state_changed(true);
        optimizer.optimize_now();
        // `quiet` has no script, so a rebuild would report an error: refusing must leave it idle.
        assert!(!optimizer.snapshot().compiling);
        assert_eq!(optimizer.snapshot().status, OptimizerStatus::Idle);
        let state = optimizer.state.lock().unwrap();
        assert!(state.run.is_none());
        assert!(!state.resume_after_game);
    }

    #[test]
    fn the_rebuild_button_does_not_start_with_the_game_open_unless_opted_in() {
        let directory = tempfile::tempdir().unwrap();
        let optimizer = quiet(directory.path());
        optimizer.configure(&directory.path().to_string_lossy(), true, false, 0);
        optimizer.game_state_changed(true);
        // Refused before anything starts: no error state either (the `quiet` optimizer has no script, so starting would fail).
        assert!(!optimizer.rebuild_now());
        assert_eq!(optimizer.snapshot().status, OptimizerStatus::Idle);
        assert!(optimizer.state.lock().unwrap().run.is_none());
        // With the game closed it goes on to start (and fails only because the test has no script).
        optimizer.game_state_changed(false);
        assert!(optimizer.rebuild_now());
        assert_eq!(optimizer.snapshot().status, OptimizerStatus::Error);
    }

    #[test]
    fn a_rebuild_is_allowed_during_the_game_when_the_user_asked_for_it() {
        let directory = tempfile::tempdir().unwrap();
        let optimizer = quiet(directory.path());
        optimizer.configure(&directory.path().to_string_lossy(), true, true, 0);
        optimizer.game_state_changed(true);
        optimizer.optimize_now();
        // The opt-in path does try to start (and fails only because the test has no script).
        assert_eq!(optimizer.snapshot().status, OptimizerStatus::Error);
    }

    #[test]
    fn status_follows_the_configuration() {
        let directory = tempfile::tempdir().unwrap();
        let optimizer = quiet(directory.path());
        assert_eq!(optimizer.snapshot().status, OptimizerStatus::Disabled);
        assert_eq!(optimizer.capture_directory(), "");
        optimizer.configure(&directory.path().to_string_lossy(), true, true, 3);
        let snapshot = optimizer.snapshot();
        assert_eq!((snapshot.status, snapshot.compiled, snapshot.captured), (OptimizerStatus::Idle, 3, 0));
        assert_eq!(snapshot.completion_ratio, 1.0);
        assert!(optimizer.capture_directory().ends_with("vu1_captures"));
        optimizer.configure(&directory.path().to_string_lossy(), false, true, 3);
        assert_eq!(optimizer.snapshot().status, OptimizerStatus::Disabled);
    }

    #[test]
    fn a_rebuild_on_request_needs_only_the_installation_folder() {
        let directory = tempfile::tempdir().unwrap();
        let optimizer = quiet(directory.path());
        // No installation folder known: nothing starts.
        assert!(!optimizer.rebuild_now());
        // With the background optimization off it still starts (here it fails for lack of a kit, which proves it tried).
        optimizer.configure(&directory.path().to_string_lossy(), false, true, 0);
        assert_eq!(optimizer.snapshot().status, OptimizerStatus::Disabled);
        assert!(optimizer.rebuild_now());
        assert!(optimizer.state.lock().unwrap().has_error);
    }

    /// A build folder whose CMake cache names the given C++ compiler.
    fn fake_build(directory: &Path, compiler: &str) {
        let build = directory.join("build");
        fs::create_dir_all(&build).unwrap();
        fs::write(
            build.join("CMakeCache.txt"),
            format!("CMAKE_BUILD_TYPE:STRING=Release\nCMAKE_CXX_COMPILER:FILEPATH={compiler}\n"),
        )
        .unwrap();
    }

    #[test]
    fn optimize_now_runs_the_installer_with_the_profile_run() {
        assert_eq!(
            profile_arguments(r"C:\GodOfWar-PC", true).unwrap(),
            ["-Destination", r"'C:\GodOfWar-PC'", "-NoPause", "-Optimize", "-Pgo", "on"]
        );
        assert!(profile_arguments("/home/me/gow", false).is_none(), "install.sh has no speed profile");
    }

    #[test]
    fn the_speed_profile_needs_a_clang_build_on_windows() {
        let directory = tempfile::tempdir().unwrap();
        assert!(!profile_supported(directory.path()), "no build yet");
        fake_build(
            directory.path(),
            "C:/Program Files/Microsoft Visual Studio/2022/BuildTools/VC/Tools/MSVC/14.44/bin/cl.exe",
        );
        assert!(!profile_supported(directory.path()), "an MSVC build");
        fake_build(directory.path(), "D:/GoW/toolchain/llvm-mingw-20261020/bin/clang++.exe");
        assert_eq!(profile_supported(directory.path()), cfg!(windows));
    }

    #[test]
    fn optimize_now_never_starts_with_the_game_open() {
        let directory = tempfile::tempdir().unwrap();
        fake_build(directory.path(), "D:/GoW/toolchain/llvm-mingw-20261020/bin/clang++.exe");
        let optimizer = quiet(directory.path());
        // Even with rebuilds during play allowed: this one opens the game by itself.
        optimizer.configure(&directory.path().to_string_lossy(), true, true, 0);
        optimizer.game_state_changed(true);
        assert!(!optimizer.profile_now());
        assert!(optimizer.state.lock().unwrap().run.is_none());
        optimizer.game_state_changed(false);
        // With the game closed it goes on to start on Windows (and fails only because the test has no kit).
        assert_eq!(optimizer.profile_now(), cfg!(windows));
    }

    #[test]
    fn optimize_now_is_not_offered_for_an_msvc_build() {
        let directory = tempfile::tempdir().unwrap();
        fake_build(directory.path(), "C:/VS/cl.exe");
        let optimizer = quiet(directory.path());
        optimizer.configure(&directory.path().to_string_lossy(), true, true, 0);
        assert!(!optimizer.profile_now());
        let snapshot = optimizer.snapshot();
        assert!(!snapshot.profile_supported && !snapshot.profile_ready && !snapshot.profiling);
    }

    #[test]
    fn the_snapshot_says_whether_the_profile_exists() {
        let directory = tempfile::tempdir().unwrap();
        let optimizer = quiet(directory.path());
        optimizer.configure(&directory.path().to_string_lossy(), true, true, 0);
        assert!(!optimizer.snapshot().profile_ready);
        fs::create_dir_all(directory.path().join("build/pgo")).unwrap();
        fs::write(profile_path(directory.path()), b"profile").unwrap();
        assert!(optimizer.snapshot().profile_ready);
    }

    #[test]
    fn a_rebuild_without_the_kit_is_an_error() {
        let directory = tempfile::tempdir().unwrap();
        let optimizer = quiet(directory.path());
        optimizer.configure(&directory.path().to_string_lossy(), true, true, 0);
        optimizer.optimize_now();
        assert_eq!(optimizer.snapshot().status, OptimizerStatus::Error);
    }
}
