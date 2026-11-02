//! The installation kit (`install.ps1`, `RustPortPS2/native/`, `RustPortPS2/games/`...) travels inside the launcher's installer
//! and is extracted into the data folder on first run. `install.ps1` writes into its own tree (the generated
//! sources go under `RustPortPS2/native/`), so it cannot run from a read-only install folder; and the extraction folder must
//! keep the same path across versions, because the build in the user's destination folder remembers its source path.
//! A new version therefore updates the files in place and never deletes what the build generated.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::Path;

use crate::rebuild;

pub const MARKER: &str = ".kit-version";

/// The files the installers cannot run without, relative to the kit folder: the script, the two prebuilt executables and
/// the game data the recompiler and the build read. An antivirus that removes one of them leaves the version marker
/// behind, so [`sync`] checks them on every start.
pub fn essential_files() -> Vec<String> {
    let (script, exe) = if cfg!(windows) { ("install.ps1", ".exe") } else { ("install.sh", "") };
    vec![
        script.to_string(),
        format!("bin/ps2_recomp{exe}"),
        format!("bin/ps2-tools{exe}"),
        "RustPortPS2/native/CMakeLists.txt".to_string(),
        "RustPortPS2/games/scus-97399/gow-port.toml".to_string(),
        "RustPortPS2/games/scus-97399/funcmap.csv".to_string(),
        "RustPortPS2/games/scus-97399/vu1.manifest".to_string(),
    ]
}

/// The [`essential_files`] that `root` does not have, as relative paths.
pub fn missing_essentials(root: &Path) -> Vec<String> {
    essential_files().into_iter().filter(|file| !root.join(file).is_file()).collect()
}

/// The kit files missing for the installer the launcher is going to run: `script` is the `install.ps1` / `install.sh` it
/// found. When it came from a repository checkout above the launcher (development) there is no kit to check.
pub fn missing_for_script(script: Option<&Path>, kit_dir: &Path) -> Vec<String> {
    match script {
        Some(script) if !script.starts_with(kit_dir) => Vec::new(),
        _ => missing_essentials(kit_dir),
    }
}

/// Copies `source` over `target` when `version` differs from the one recorded there, or when an essential file is gone
/// even though the version matches. Files that already hold the same content are left alone, modification time included;
/// files that exist only in `target` are kept. Returns `true` if anything was extracted.
pub fn sync(source: &Path, target: &Path, version: &str) -> io::Result<bool> {
    let current = fs::read_to_string(target.join(MARKER)).is_ok_and(|current| current.trim() == version);
    // A kit whose digest could not be recorded (a file locked by an antivirus) is tried again at the next start: without
    // the digest the launcher can never tell that a build is out of date.
    if current && missing_essentials(target).is_empty() && rebuild::read_kit_digest(target).is_some() {
        return Ok(false);
    }
    // The first extraction that records a digest also needs the digest of the kit it replaces (see `rebuild`). A folder that
    // holds no kit yet (a first installation) replaces nothing: hashing it would give the digest of nothing, which every
    // build without a marker would then be compared with.
    let has_old_kit = target.join(MARKER).is_file() || missing_essentials(target).len() < essential_files().len();
    let before =
        if has_old_kit && rebuild::read_kit_digest(target).is_none() { rebuild::kit_digest(target).ok() } else { None };
    fs::create_dir_all(target)?;
    copy_tree(source, target)?;
    rebuild::record_kit(target, before);
    fs::write(target.join(MARKER), version)?;
    Ok(true)
}

fn copy_tree(source: &Path, target: &Path) -> io::Result<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let destination = target.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            fs::create_dir_all(&destination)?;
            copy_tree(&entry.path(), &destination)?;
        } else {
            copy_file(&entry.path(), &destination)?;
        }
    }
    Ok(())
}

/// Whether two files hold the same bytes.
fn same_content(a: &Path, b: &Path) -> io::Result<bool> {
    let (mut a, mut b) = (File::open(a)?, File::open(b)?);
    let (mut left, mut right) = (vec![0u8; 64 * 1024], vec![0u8; 64 * 1024]);
    loop {
        let read = a.read(&mut left)?;
        let mut filled = 0;
        while filled < read {
            let more = b.read(&mut right[filled..read])?;
            if more == 0 {
                return Ok(false);
            }
            filled += more;
        }
        if read == 0 {
            // `a` is at its end: `b` must be too.
            return Ok(b.read(&mut right[..1])? == 0);
        }
        if left[..read] != right[..read] {
            return Ok(false);
        }
    }
}

/// Copies one kit file. A file whose content is already there is left alone, including its modification time: the build
/// folder's objects are older or newer than their sources by those times, and a release stamps every file it carries with the
/// time it was staged, so copying an unchanged file would make the build recompile everything that includes it. A file that
/// changed is copied and gets the time of the copy (not the source's: a source older than the objects would be ignored).
fn copy_file(source: &Path, destination: &Path) -> io::Result<()> {
    let from = fs::metadata(source)?;
    let existing = fs::metadata(destination);
    if let Ok(to) = &existing {
        if to.len() == from.len() && same_content(source, destination).unwrap_or(false) {
            return Ok(());
        }
    }
    // A read-only copy left by an earlier extraction must not block the update.
    if existing.is_ok() {
        let _ = fs::remove_file(destination);
    }
    fs::copy(source, destination)?;
    if let Ok(file) = File::options().write(true).open(destination) {
        let _ = file.set_modified(std::time::SystemTime::now());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn the_first_run_extracts_everything() {
        let root = tempfile::tempdir().unwrap();
        let (source, target) = (root.path().join("resources/kit"), root.path().join("data/kit"));
        write(&source.join("install.ps1"), "script");
        write(&source.join("RustPortPS2/native/a/b.cpp"), "code");
        assert!(sync(&source, &target, "1.0.0").unwrap());
        assert_eq!(fs::read_to_string(target.join("install.ps1")).unwrap(), "script");
        assert_eq!(fs::read_to_string(target.join("RustPortPS2/native/a/b.cpp")).unwrap(), "code");
        assert_eq!(fs::read_to_string(target.join(MARKER)).unwrap(), "1.0.0");
    }

    fn write_essentials(root: &Path) {
        for file in essential_files() {
            write(&root.join(file), "x");
        }
    }

    #[test]
    fn missing_essentials_lists_relative_paths() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(missing_essentials(root.path()), essential_files());
        write_essentials(root.path());
        assert!(missing_essentials(root.path()).is_empty());
        fs::remove_file(root.path().join("RustPortPS2/games/scus-97399/funcmap.csv")).unwrap();
        assert_eq!(missing_essentials(root.path()), ["RustPortPS2/games/scus-97399/funcmap.csv"]);
    }

    #[test]
    fn a_repository_checkout_has_no_kit_to_check() {
        let root = tempfile::tempdir().unwrap();
        let kit = root.path().join("kit");
        write_essentials(&kit);
        let script = kit.join(essential_files().remove(0));
        assert!(missing_for_script(Some(&script), &kit).is_empty());
        fs::remove_file(&script).unwrap();
        // No script found at all: the kit is what is missing.
        assert_eq!(missing_for_script(None, &kit), [essential_files().remove(0)]);
        // A script found elsewhere (a checkout): nothing to report about the kit folder.
        assert!(missing_for_script(Some(&root.path().join("repo/install.ps1")), &kit).is_empty());
    }

    #[test]
    fn a_missing_essential_file_is_restored_even_if_the_version_matches() {
        let root = tempfile::tempdir().unwrap();
        let (source, target) = (root.path().join("kit"), root.path().join("data"));
        write_essentials(&source);
        assert!(sync(&source, &target, "1.0.0").unwrap());
        // Something else the player edited, and a generated file, stay as they are.
        write(&target.join("install.cmd"), "edited by hand");
        write(&target.join("RustPortPS2/native/ps2xRuntime/src/runner/gen_0001.cpp"), "generated");
        let script = target.join(essential_files().remove(0));
        fs::remove_file(&script).unwrap();
        assert!(sync(&source, &target, "1.0.0").unwrap());
        assert!(script.is_file());
        assert!(missing_essentials(&target).is_empty());
        assert_eq!(fs::read_to_string(target.join("install.cmd")).unwrap(), "edited by hand");
        assert_eq!(
            fs::read_to_string(target.join("RustPortPS2/native/ps2xRuntime/src/runner/gen_0001.cpp")).unwrap(),
            "generated"
        );
        // Nothing is missing any more: the next start does nothing.
        assert!(!sync(&source, &target, "1.0.0").unwrap());
    }

    #[test]
    fn sync_writes_the_digest_with_the_version_marker() {
        let root = tempfile::tempdir().unwrap();
        let (source, target) = (root.path().join("kit"), root.path().join("data"));
        write_essentials(&source);
        write(&source.join("RustPortPS2/native/ps2xRuntime/src/a.cpp"), "int a;");
        assert!(sync(&source, &target, "1.0.0").unwrap());
        let first = rebuild::read_kit_digest(&target).expect("the digest is written next to the marker");
        assert_eq!(first, rebuild::kit_digest(&target).unwrap());
        // A version whose runtime files are the same keeps the digest; a changed runtime file changes it.
        assert!(sync(&source, &target, "1.0.1").unwrap());
        assert_eq!(rebuild::read_kit_digest(&target).unwrap(), first);
        std::thread::sleep(std::time::Duration::from_millis(20));
        write(&source.join("RustPortPS2/native/ps2xRuntime/src/a.cpp"), "int fixed;");
        assert!(sync(&source, &target, "1.0.2").unwrap());
        assert_ne!(rebuild::read_kit_digest(&target).unwrap(), first);
    }

    #[test]
    fn the_first_digest_over_an_older_kit_remembers_the_digest_it_replaced() {
        let root = tempfile::tempdir().unwrap();
        let (source, target) = (root.path().join("kit"), root.path().join("data"));
        write_essentials(&source);
        write(&source.join("RustPortPS2/native/ps2xRuntime/src/a.cpp"), "int fixed;");
        // A kit extracted by a launcher that kept no digest, with the runtime of the previous release.
        write_essentials(&target);
        write(&target.join("RustPortPS2/native/ps2xRuntime/src/a.cpp"), "int old;");
        fs::write(target.join(MARKER), "0.9.0").unwrap();
        let old = rebuild::kit_digest(&target).unwrap();
        assert!(sync(&source, &target, "1.0.0").unwrap());
        assert_eq!(fs::read_to_string(target.join(rebuild::PRIOR_MARKER)).unwrap(), old);
    }

    #[test]
    fn the_same_version_is_not_extracted_again() {
        let root = tempfile::tempdir().unwrap();
        let (source, target) = (root.path().join("kit"), root.path().join("data"));
        write_essentials(&source);
        assert!(sync(&source, &target, "1.0.0").unwrap());
        let script = target.join(essential_files().remove(0));
        write(&script, "edited by hand");
        assert!(!sync(&source, &target, "1.0.0").unwrap());
        assert_eq!(fs::read_to_string(script).unwrap(), "edited by hand");
    }

    #[test]
    fn a_new_version_updates_in_place_and_keeps_generated_files() {
        let root = tempfile::tempdir().unwrap();
        let (source, target) = (root.path().join("kit"), root.path().join("data"));
        write(&source.join("install.ps1"), "v1");
        write(&source.join("config/recomp.toml"), "same");
        sync(&source, &target, "1.0.0").unwrap();
        // The installer generated sources into the kit's tree.
        write(&target.join("RustPortPS2/native/ps2xRuntime/src/runner/gen_0001.cpp"), "generated");

        std::thread::sleep(std::time::Duration::from_millis(20));
        write(&source.join("install.ps1"), "version two");
        assert!(sync(&source, &target, "1.1.0").unwrap());
        assert_eq!(fs::read_to_string(target.join("install.ps1")).unwrap(), "version two");
        assert_eq!(fs::read_to_string(target.join("config/recomp.toml")).unwrap(), "same");
        assert_eq!(
            fs::read_to_string(target.join("RustPortPS2/native/ps2xRuntime/src/runner/gen_0001.cpp")).unwrap(),
            "generated"
        );
        assert_eq!(fs::read_to_string(target.join(MARKER)).unwrap(), "1.1.0");
    }

    #[test]
    fn a_read_only_old_copy_does_not_block_the_update() {
        let root = tempfile::tempdir().unwrap();
        let (source, target) = (root.path().join("kit"), root.path().join("data"));
        write(&source.join("a.txt"), "old");
        sync(&source, &target, "1").unwrap();
        let mut permissions = fs::metadata(target.join("a.txt")).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(target.join("a.txt"), permissions).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        write(&source.join("a.txt"), "newer content");
        assert!(sync(&source, &target, "2").unwrap());
        assert_eq!(fs::read_to_string(target.join("a.txt")).unwrap(), "newer content");
    }

    fn modified(path: &Path) -> std::time::SystemTime {
        fs::metadata(path).unwrap().modified().unwrap()
    }

    #[test]
    fn an_unchanged_file_keeps_its_modification_time_across_versions() {
        let root = tempfile::tempdir().unwrap();
        let (source, target) = (root.path().join("kit"), root.path().join("data"));
        write_essentials(&source);
        write(&source.join("RustPortPS2/native/ps2xRuntime/include/ps2_runtime.h"), "header");
        sync(&source, &target, "1.0.0").unwrap();
        let header = target.join("RustPortPS2/native/ps2xRuntime/include/ps2_runtime.h");
        let installed = modified(&header);
        // A new release stages every file again with a new time, and changes one other file.
        std::thread::sleep(std::time::Duration::from_millis(50));
        write(&source.join("RustPortPS2/native/ps2xRuntime/include/ps2_runtime.h"), "header");
        write(&source.join("RustPortPS2/native/ps2xRuntime/src/a.cpp"), "int fixed;");
        assert!(sync(&source, &target, "1.0.1").unwrap());
        assert_eq!(modified(&header), installed, "the build would recompile everything that includes the header");
        assert_eq!(fs::read_to_string(target.join("RustPortPS2/native/ps2xRuntime/src/a.cpp")).unwrap(), "int fixed;");
    }

    #[test]
    fn a_changed_file_gets_the_time_of_the_copy_not_the_one_of_its_source() {
        let root = tempfile::tempdir().unwrap();
        let (source, target) = (root.path().join("kit"), root.path().join("data"));
        write_essentials(&source);
        write(&source.join("a.cpp"), "old");
        sync(&source, &target, "1.0.0").unwrap();
        write(&source.join("a.cpp"), "new and longer");
        // The source is much older than the build's objects: ninja would ignore the new content if the copy kept this time.
        let long_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(10 * 24 * 3600);
        File::options().write(true).open(source.join("a.cpp")).unwrap().set_modified(long_ago).unwrap();
        let before = std::time::SystemTime::now() - std::time::Duration::from_secs(5);
        assert!(sync(&source, &target, "1.0.1").unwrap());
        assert_eq!(fs::read_to_string(target.join("a.cpp")).unwrap(), "new and longer");
        assert!(modified(&target.join("a.cpp")) > before);
    }

    #[test]
    fn same_content_compares_whole_files() {
        let root = tempfile::tempdir().unwrap();
        let (a, b, c) = (root.path().join("a"), root.path().join("b"), root.path().join("c"));
        let big: Vec<u8> = (0..200_000u32).map(|n| (n % 251) as u8).collect();
        fs::write(&a, &big).unwrap();
        fs::write(&b, &big).unwrap();
        let mut other = big.clone();
        *other.last_mut().unwrap() ^= 1; // only the last byte differs
        fs::write(&c, &other).unwrap();
        assert!(same_content(&a, &b).unwrap());
        assert!(!same_content(&a, &c).unwrap());
        fs::write(&c, &big[..big.len() - 1]).unwrap();
        assert!(!same_content(&a, &c).unwrap());
        assert!(!same_content(&c, &a).unwrap());
    }

    #[test]
    fn a_first_installation_remembers_no_prior_digest() {
        let root = tempfile::tempdir().unwrap();
        let (source, target) = (root.path().join("kit"), root.path().join("data"));
        write_essentials(&source);
        write(&source.join("RustPortPS2/native/ps2xRuntime/src/a.cpp"), "int a;");
        assert!(sync(&source, &target, "1.0.0").unwrap());
        assert!(!target.join(rebuild::PRIOR_MARKER).exists(), "an empty folder replaces no kit");
    }

    #[test]
    fn a_kit_without_a_digest_records_it_at_the_next_start() {
        let root = tempfile::tempdir().unwrap();
        let (source, target) = (root.path().join("kit"), root.path().join("data"));
        write_essentials(&source);
        sync(&source, &target, "1.0.0").unwrap();
        fs::remove_file(target.join(rebuild::DIGEST_MARKER)).unwrap(); // the recording failed once (a locked file)
        assert!(sync(&source, &target, "1.0.0").unwrap());
        assert!(rebuild::read_kit_digest(&target).is_some());
        assert!(!sync(&source, &target, "1.0.0").unwrap());
    }

    #[test]
    fn a_missing_source_is_an_error_and_records_nothing() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("data");
        assert!(sync(&root.path().join("nope"), &target, "1").is_err());
        assert!(!target.join(MARKER).exists());
    }
}
