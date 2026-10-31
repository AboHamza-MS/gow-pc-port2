//! The parity gate on the synthetic fixture: no game files needed.
//!
//! The expected tree comes from the C++ recompiler once (see `docs/PARITY.md`) and
//! is committed as a manifest per operating system, because the C++ writer is in
//! text mode: CRLF on Windows, LF elsewhere.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ps2_config::RecompilerConfig;
use ps2_parity::fixture::Fixture;
use ps2_parity::{collect_tree, compare_files, current_operating_system, sha256_hex, Manifest};
use ps2_recomp::recompiler::Recompiler;
use ps2_recomp::reporter::Reporter;

fn manifests_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/parity/manifests")
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ps2-parity-fixture-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn slashes(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

/// Run the whole Rust pipeline on the fixture and return the generated tree's directory.
fn generate(fixture: &Fixture, directory: &Path) -> PathBuf {
    let elf_path = directory.join("fixture.elf");
    let output = directory.join("out");
    std::fs::write(&elf_path, &fixture.elf).unwrap();
    let toml = fixture.config_toml(&slashes(&elf_path), &format!("{}/", slashes(&output)));
    let config_path = directory.join("recomp.toml");
    std::fs::write(&config_path, &toml).unwrap();

    let (config, report) = RecompilerConfig::load(&config_path).expect("the fixture config loads");
    let reporter = Arc::new(Reporter::new());
    let mut recompiler =
        Recompiler::initialize(config, &slashes(&config_path), &report.warnings, &fixture.elf, reporter)
            .expect("the fixture initializes");
    assert!(recompiler.recompile(), "the fixture recompiles");
    recompiler.generate_output().expect("the fixture output is written");
    output
}

#[test]
fn fixture_tree_matches_the_cpp_baseline() {
    let operating_system = current_operating_system();
    let manifest_path = manifests_directory().join(format!("fixture-{operating_system}.json"));
    let Ok(text) = std::fs::read_to_string(&manifest_path) else {
        eprintln!("no fixture baseline for {operating_system} yet ({}): skipping", manifest_path.display());
        return;
    };
    let manifest = Manifest::from_json(&text).expect("the manifest parses");

    let fixture = Fixture::build();
    assert_eq!(
        sha256_hex(&fixture.elf),
        manifest.elf_sha256,
        "the fixture ELF changed: regenerate the baseline with the C++ recompiler"
    );

    let directory = scratch("tree");
    let output = generate(&fixture, &directory);
    let files = collect_tree(&output).unwrap();
    let comparison = compare_files(&manifest.files, &files);
    assert!(
        comparison.is_identical(),
        "differs from the C++ baseline: only in manifest {:?}, only in tree {:?}, different {:?}",
        comparison.only_in_a,
        comparison.only_in_b,
        comparison.different
    );
    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn fixture_is_deterministic() {
    let fixture = Fixture::build();
    let first = generate(&fixture, &scratch("first"));
    let second = generate(&fixture, &scratch("second"));
    let comparison = compare_files(&collect_tree(&first).unwrap(), &collect_tree(&second).unwrap());
    assert!(comparison.is_identical(), "two runs differ: {:?}", comparison.different);
}

#[test]
fn the_scan_finds_the_entry_and_the_jal_targets_of_a_stripped_image() {
    use ps2_elf::ElfFile;

    let fixture = Fixture::build();
    let elf = ElfFile::parse(&fixture.elf).expect("the fixture is a valid ELF");
    let starts: Vec<u32> = ps2_recomp::scan::scan_function_starts(&elf).iter().map(|function| function.start).collect();

    let entry = elf.entry();
    let leaf = fixture.address_of("fx_leaf").expect("the fixture has a leaf");
    assert!(starts.contains(&entry), "the entry point is always a start");
    assert!(starts.contains(&leaf), "fx_flow calls the leaf with jal");
    assert!(starts.windows(2).all(|pair| pair[0] < pair[1]), "starts come sorted and unique");
    assert!(ps2_recomp::scan::scan_function_starts(&elf)
        .iter()
        .all(|function| function.end == 0 && function.name == format!("sub_{:08X}", function.start)));
}
