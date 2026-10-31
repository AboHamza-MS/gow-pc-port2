//! Command-line companion of the launcher. Replaces the diagnostic flags of the former C++ launcher
//! (`--validate-iso`, `--install-test`, `--optimizer-test`) and adds `env`, which prints the environment the game
//! would be started with for a settings file.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use gow_core::installer::{self, InstallRequest};
use gow_core::launch::{self, LaunchContext};
use gow_core::optimizer::{self, Optimizer};
use gow_core::paths::AppPaths;
use gow_core::settings::{DisplayInfo, Settings};
use gow_core::{elf_check, game, iso};

const USAGE: &str = "gow-cli <command>

  validate-iso <file>                 check a disc image (ok, reason, hash)
  check-game <elf> [runner]           CRC / fingerprint of the ELF
  elf-report <elf> [funcmap.csv]      structure of the ELF (entry, sections, map coverage)
  env [settings.json]                 environment the game would get (default: the launcher's own settings file)
  install-test <iso> <destination>    run the wizard's installation without a window
  optimizer-test <destination>        register the build and run one optimizer rebuild
";

fn script_path(paths: &AppPaths) -> Option<PathBuf> {
    let launcher_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    installer::find_install_script(&paths.kit_dir(), &launcher_dir)
}

fn validate_iso(file: &str) -> ExitCode {
    let result = iso::validate_iso(file);
    println!("{}", serde_json::to_string(&result).unwrap());
    if result.ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn check_game(elf: &str, runner: &str) -> ExitCode {
    let state = game::check_game(elf, runner);
    println!("exists={} valid={} modified={} crc={}", state.exists, state.valid, state.modified, state.crc);
    if state.valid {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn elf_report(elf: &str, funcmap: Option<&str>) -> ExitCode {
    let report = elf_check::inspect(Path::new(elf), funcmap.map(Path::new));
    println!("{}", serde_json::to_string(&report).unwrap());
    if report.ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn print_env(file: Option<&str>, paths: &AppPaths) -> ExitCode {
    let path = file.map(PathBuf::from).unwrap_or_else(|| paths.settings_file());
    let mut settings = Settings::load(&path);
    settings.migrate(false, &DisplayInfo::default());
    let ctx = LaunchContext {
        settings: &settings,
        scale: settings.selected_scale(),
        vu1_capture: String::new(),
        gpu: settings.gpu.clone(),
        icon: None,
        shader_cache: paths.shader_cache(),
        retro_achievements_password: None,
    };
    let plan = launch::build_environment(&ctx, |name| std::env::var(name).ok());
    for (name, value) in &plan.set {
        let shown = if name.ends_with("_PASSWORD") { "***" } else { value.as_str() };
        println!("{name}={shown}");
    }
    for name in &plan.remove {
        println!("-{name}");
    }
    ExitCode::SUCCESS
}

fn install_test(iso_path: &str, destination: &str, paths: &AppPaths) -> ExitCode {
    let Some(script) = script_path(paths) else {
        eprintln!("install script not found");
        return ExitCode::from(4);
    };
    let environment = installer::check_environment(Some(&script));
    let destination_check = installer::validate_destination(destination);
    println!(
        "iso_valid={} destination_ok={} vs={} git={} installer={} manifest={}",
        iso::validate_iso(iso_path).ok,
        destination_check.ok,
        environment.vs,
        environment.git,
        environment.installer,
        installer::has_manifest(&script)
    );
    let (done_sender, done_receiver) = mpsc::channel();
    // Progress is printed in steps of 5 %, once per (stage, step).
    let printed = std::sync::Arc::new(std::sync::atomic::AtomicI32::new(i32::MIN));
    let progress_printed = printed.clone();
    let request = InstallRequest {
        script,
        iso: iso_path.to_string(),
        destination: destination.to_string(),
        install_tools: false,
        vu1_memory_limit_gb: 4,
        log_file: paths.logs_dir().join("install.log"),
    };
    let installation = installer::start(
        request,
        move |progress| {
            let pct = ((progress.stage_progress * 20.0).round() as i32) * 5;
            let key = progress.stage * 1000 + pct;
            if progress_printed.swap(key, std::sync::atomic::Ordering::Relaxed) != key {
                println!("stage={} stage_progress={}%", progress.stage, pct);
            }
        },
        move |outcome| {
            let _ = done_sender.send(outcome);
        },
    );
    if let Err(error) = installation {
        eprintln!("could not start: {error}");
        return ExitCode::from(4);
    }
    let outcome = done_receiver.recv().expect("installer thread ended without a result");
    println!("completed ok={} error=[{}] elf={} exe={}", outcome.ok, outcome.error, outcome.elf, outcome.executable);
    if outcome.cancelled {
        return ExitCode::from(3);
    }
    if outcome.ok {
        let active = optimizer::register_installation(Path::new(&outcome.executable), Path::new(destination));
        println!("enabled={} captured={}", active.display(), optimizer::count_captures(Path::new(destination)));
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn optimizer_test(destination: &str, paths: &AppPaths) -> ExitCode {
    let runner = Path::new(destination).join("build/ps2xRuntime").join(game::EXECUTABLE_NAME);
    let active = optimizer::register_installation(&runner, Path::new(destination));
    println!(
        "enabled={} exists={} captured={}",
        active.display(),
        active.exists(),
        optimizer::count_captures(Path::new(destination))
    );
    let (sender, receiver) = mpsc::channel();
    let optimizer = Optimizer::new(
        paths.logs_dir().join("optimize.log"),
        move |snapshot| {
            println!(
                "status={:?} captured={} compiled={} pct={} progress={}",
                snapshot.status,
                snapshot.captured,
                snapshot.compiled,
                (snapshot.completion_ratio * 100.0).round(),
                (snapshot.build_progress * 100.0).round()
            );
            let _ = sender.send(snapshot.clone());
        },
        |_| {},
    );
    optimizer.set_script(script_path(paths));
    optimizer.configure(destination, true, false, 0); // pretends nothing was built: there are pending items
    optimizer.optimize_now();
    let started = Instant::now();
    let mut saw_compiling = false;
    loop {
        match receiver.recv_timeout(Duration::from_secs(1)) {
            Ok(snapshot) => saw_compiling |= snapshot.compiling,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        let snapshot = optimizer.snapshot();
        if (saw_compiling && !snapshot.compiling) || started.elapsed() > Duration::from_secs(1500) {
            println!(
                "end: status={:?} compiled={} seconds={}",
                snapshot.status,
                snapshot.compiled,
                started.elapsed().as_secs()
            );
            return if started.elapsed() > Duration::from_secs(1500) { ExitCode::from(2) } else { ExitCode::SUCCESS };
        }
    }
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let paths = AppPaths::system();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["validate-iso", file] => validate_iso(file),
        ["check-game", elf] => check_game(elf, ""),
        ["check-game", elf, runner] => check_game(elf, runner),
        ["elf-report", elf] => elf_report(elf, None),
        ["elf-report", elf, funcmap] => elf_report(elf, Some(funcmap)),
        ["env"] => print_env(None, &paths),
        ["env", file] => print_env(Some(file), &paths),
        ["install-test", iso_path, destination] => install_test(iso_path, destination, &paths),
        ["optimizer-test", destination] => optimizer_test(destination, &paths),
        _ => {
            eprint!("{USAGE}");
            ExitCode::from(64)
        }
    }
}
