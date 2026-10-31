//! `ps2-parity`: the command line of the parity gate.
//!
//! ```text
//! ps2-parity compare A B [--show N]
//! ps2-parity extract A B <path>
//! ps2-parity manifest TREE --elf FILE --version TEXT --out FILE [--log FILE]
//! ps2-parity verify TREE MANIFEST [--show N]
//! ps2-parity fixture --out DIR
//! ```

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ps2_parity::fixture::Fixture;
use ps2_parity::{
    collect_tree, compare_files, counts_from_log, current_operating_system, first_difference, sha256_hex, Comparison,
    Manifest,
};

const USAGE: &str = "usage:
  ps2-parity compare A B [--show N]
  ps2-parity extract A B <path>
  ps2-parity manifest TREE --elf FILE --version TEXT --out FILE [--log FILE]
  ps2-parity verify TREE MANIFEST [--show N]
  ps2-parity fixture --out DIR";

fn option(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|arg| arg == name).and_then(|index| args.get(index + 1)).cloned()
}

fn show_count(args: &[String]) -> usize {
    option(args, "--show").and_then(|value| value.parse().ok()).unwrap_or(5)
}

fn print_comparison(comparison: &Comparison, show: usize, a_root: Option<&Path>, b_root: Option<&Path>) {
    println!(
        "common: {}, only in A: {}, only in B: {}, different: {}",
        comparison.common,
        comparison.only_in_a.len(),
        comparison.only_in_b.len(),
        comparison.different.len()
    );
    for path in comparison.only_in_a.iter().take(show) {
        println!("  only in A: {path}");
    }
    for path in comparison.only_in_b.iter().take(show) {
        println!("  only in B: {path}");
    }
    for path in comparison.different.iter().take(show) {
        println!("  differs: {path}");
        if let (Some(a), Some(b)) = (a_root, b_root) {
            if let (Ok(x), Ok(y)) = (std::fs::read(a.join(path)), std::fs::read(b.join(path))) {
                if let Some((line, left, right)) = first_difference(&x, &y) {
                    println!("    line {line}:\n      A: {left:?}\n      B: {right:?}");
                }
            }
        }
    }
}

fn compare(args: &[String]) -> Result<bool, String> {
    let [a, b, ..] = args else {
        return Err(USAGE.to_owned());
    };
    let (a, b) = (PathBuf::from(a), PathBuf::from(b));
    let files_a = collect_tree(&a).map_err(|error| format!("{}: {error}", a.display()))?;
    let files_b = collect_tree(&b).map_err(|error| format!("{}: {error}", b.display()))?;
    let comparison = compare_files(&files_a, &files_b);
    print_comparison(&comparison, show_count(args), Some(&a), Some(&b));
    Ok(comparison.is_identical())
}

fn extract(args: &[String]) -> Result<bool, String> {
    let [a, b, path] = args else {
        return Err(USAGE.to_owned());
    };
    let x = std::fs::read(Path::new(a).join(path)).map_err(|error| format!("{a}/{path}: {error}"))?;
    let y = std::fs::read(Path::new(b).join(path)).map_err(|error| format!("{b}/{path}: {error}"))?;
    match first_difference(&x, &y) {
        None => {
            println!("{path}: identical");
            Ok(true)
        }
        Some((line, left, right)) => {
            println!("{path}: first difference at line {line}\n  A: {left}\n  B: {right}");
            Ok(false)
        }
    }
}

fn manifest(args: &[String]) -> Result<bool, String> {
    let Some(tree) = args.first() else {
        return Err(USAGE.to_owned());
    };
    let elf = option(args, "--elf").ok_or("--elf is required")?;
    let version = option(args, "--version").ok_or("--version is required")?;
    let out = option(args, "--out").ok_or("--out is required")?;

    let elf_bytes = std::fs::read(&elf).map_err(|error| format!("{elf}: {error}"))?;
    let files = collect_tree(Path::new(tree)).map_err(|error| format!("{tree}: {error}"))?;
    let counts = match option(args, "--log") {
        Some(log) => counts_from_log(&std::fs::read_to_string(&log).map_err(|error| format!("{log}: {error}"))?),
        None => Default::default(),
    };

    let manifest = Manifest {
        generator: "ps2_recomp".to_owned(),
        generator_version: version,
        operating_system: current_operating_system().to_owned(),
        elf_sha256: sha256_hex(&elf_bytes),
        files,
        counts,
    };
    std::fs::write(&out, manifest.to_json()).map_err(|error| format!("{out}: {error}"))?;
    println!("wrote {out}: {} files, elf {}", manifest.files.len(), manifest.elf_sha256);
    Ok(true)
}

fn verify(args: &[String]) -> Result<bool, String> {
    let [tree, manifest_path, ..] = args else {
        return Err(USAGE.to_owned());
    };
    let text = std::fs::read_to_string(manifest_path).map_err(|error| format!("{manifest_path}: {error}"))?;
    let manifest = Manifest::from_json(&text)?;
    let files = collect_tree(Path::new(tree)).map_err(|error| format!("{tree}: {error}"))?;
    let comparison = compare_files(&manifest.files, &files);
    println!("manifest: {} ({}), elf {}", manifest.generator_version, manifest.operating_system, manifest.elf_sha256);
    print_comparison(&comparison, show_count(args), None, None);
    Ok(comparison.is_identical())
}

/// Write the synthetic ELF and its `recomp.toml` (output goes to `DIR/out/`).
fn fixture(args: &[String]) -> Result<bool, String> {
    let directory = PathBuf::from(option(args, "--out").ok_or("--out is required")?);
    std::fs::create_dir_all(&directory).map_err(|error| format!("{}: {error}", directory.display()))?;
    let fixture = Fixture::build();
    let slashes = |path: &Path| path.display().to_string().replace('\\', "/");
    let elf_path = directory.join("fixture.elf");
    let output = directory.join("out");
    std::fs::write(&elf_path, &fixture.elf).map_err(|error| format!("{}: {error}", elf_path.display()))?;
    let config = fixture.config_toml(&slashes(&elf_path), &format!("{}/", slashes(&output)));
    let config_path = directory.join("recomp.toml");
    std::fs::write(&config_path, config).map_err(|error| format!("{}: {error}", config_path.display()))?;
    println!("wrote {} and {}", elf_path.display(), config_path.display());
    Ok(true)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((command, rest)) = args.split_first() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };

    let result = match command.as_str() {
        "compare" => compare(rest),
        "extract" => extract(rest),
        "manifest" => manifest(rest),
        "verify" => verify(rest),
        "fixture" => fixture(rest),
        _ => Err(USAGE.to_owned()),
    };

    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    }
}
