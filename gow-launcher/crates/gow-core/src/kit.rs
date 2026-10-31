//! The installation kit (`install.ps1`, `RustPortPS2/native/`, `RustPortPS2/games/`...) travels inside the launcher's installer
//! and is extracted into the data folder on first run. `install.ps1` writes into its own tree (the generated
//! sources go under `RustPortPS2/native/`), so it cannot run from a read-only install folder; and the extraction folder must
//! keep the same path across versions, because the build in the user's destination folder remembers its source path.
//! A new version therefore updates the files in place and never deletes what the build generated.

use std::fs::{self, File};
use std::io;
use std::path::Path;

pub const MARKER: &str = ".kit-version";

/// Copies `source` over `target` when `version` differs from the one recorded there. Files that are already up to date
/// (same size and modification time) are left alone; files that exist only in `target` are kept. Returns `true` if
/// anything was extracted.
pub fn sync(source: &Path, target: &Path, version: &str) -> io::Result<bool> {
    if fs::read_to_string(target.join(MARKER)).is_ok_and(|current| current.trim() == version) {
        return Ok(false);
    }
    fs::create_dir_all(target)?;
    copy_tree(source, target)?;
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

fn copy_file(source: &Path, destination: &Path) -> io::Result<()> {
    let (from, to) = (fs::metadata(source)?, fs::metadata(destination));
    if let Ok(to) = &to {
        if to.len() == from.len() && to.modified().ok() == from.modified().ok() {
            return Ok(());
        }
    }
    // A read-only copy left by an earlier extraction must not block the update.
    if to.is_ok() {
        let _ = fs::remove_file(destination);
    }
    fs::copy(source, destination)?;
    if let Ok(modified) = from.modified() {
        if let Ok(file) = File::options().write(true).open(destination) {
            let _ = file.set_modified(modified);
        }
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

    #[test]
    fn the_same_version_is_not_extracted_again() {
        let root = tempfile::tempdir().unwrap();
        let (source, target) = (root.path().join("kit"), root.path().join("data"));
        write(&source.join("install.ps1"), "script");
        assert!(sync(&source, &target, "1.0.0").unwrap());
        write(&target.join("install.ps1"), "edited by hand");
        assert!(!sync(&source, &target, "1.0.0").unwrap());
        assert_eq!(fs::read_to_string(target.join("install.ps1")).unwrap(), "edited by hand");
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

    #[test]
    fn a_missing_source_is_an_error_and_records_nothing() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("data");
        assert!(sync(&root.path().join("nope"), &target, "1").is_err());
        assert!(!target.join(MARKER).exists());
    }
}
