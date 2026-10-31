//! `ps2_recomp <config.toml>`: the Rust twin of `PS2Recomp/ps2xRecomp/src/runner/main.cpp`.

use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

use ps2_config::RecompilerConfig;
use ps2_recomp::recompiler::Recompiler;
use ps2_recomp::reporter::Reporter;

fn print_usage() {
    println!("PS2Recomp - A static recompiler for PlayStation 2 ELF files");
    println!("Usage: ps2recomp <config.toml>");
    println!("  config.toml: Configuration file for the recompiler");
}

fn run(config_path: &str) -> ExitCode {
    let reporter = Arc::new(Reporter::new());

    let (config, load_report) = match RecompilerConfig::load(Path::new(config_path)) {
        Ok(loaded) => loaded,
        Err(error) => {
            reporter.error("config", &format!("Error parsing configuration file: {error}"));
            reporter.error("initialize", &error.to_string());
            eprintln!("Failed to initialize recompiler");
            reporter.print_summary();
            return ExitCode::FAILURE;
        }
    };

    let elf_bytes = match std::fs::read(&config.input_path) {
        Ok(bytes) => bytes,
        Err(_) => {
            reporter.error("elf", &format!("Could not load ELF file: {}", config.input_path));
            reporter.error("elf", &format!("Failed to parse ELF file: {}", config.input_path));
            eprintln!("Failed to initialize recompiler");
            reporter.print_summary();
            return ExitCode::FAILURE;
        }
    };

    let mut recompiler =
        match Recompiler::initialize(config, config_path, &load_report.warnings, &elf_bytes, reporter.clone()) {
            Ok(recompiler) => recompiler,
            Err(error) => {
                reporter.error("initialize", &error.to_string());
                eprintln!("Failed to initialize recompiler");
                reporter.print_summary();
                return ExitCode::FAILURE;
            }
        };

    if !recompiler.recompile() {
        eprintln!("Recompilation failed");
        recompiler.print_report();
        return ExitCode::FAILURE;
    }

    if let Err(error) = recompiler.generate_output() {
        eprintln!("Error: {error}");
        return ExitCode::FAILURE;
    }
    recompiler.print_report();

    println!("Recompilation completed successfully");
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(config_path) = args.next() else {
        print_usage();
        return ExitCode::FAILURE;
    };
    run(&config_path)
}
