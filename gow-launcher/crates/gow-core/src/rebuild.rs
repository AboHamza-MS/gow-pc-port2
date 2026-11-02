//! Tells the player when their game build is older than the runtime sources the launcher carries.
//!
//! The kit holds the C++ runtime sources and the two prebuilt Rust tools, and [`crate::kit::sync`] copies them over the
//! player's kit folder when the launcher updates, but nothing rebuilds the game from them: a runtime fix reaches an
//! existing installation only when the build is redone. This module keeps two facts apart so the launcher can notice:
//! - the *kit digest*: a SHA-256 over the files that decide what a build contains, stored in the kit folder when it changes;
//! - the *built-from marker* (`<destination>/build/.built-from-kit`): the kit digest a build was made from, written by the
//!   launcher when an installation or a rebuild succeeds. `install.ps1` does not write it.
//!
//! A build without a marker predates this feature. The launcher cannot know what it was made from, and a surprise rebuild
//! of an hour is worse than a missed fix, so it adopts the current digest silently, with one exception: when the update
//! that introduced the digest found a *different* runtime in the kit than the one it replaced, any build without a marker is
//! older than the new kit by construction, and the player is asked (see [`PRIOR_MARKER`]).

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::game::EXECUTABLE_NAME;

/// In the kit folder, next to `.kit-version`: the digest of the kit's runtime-affecting files, as of the last extraction.
pub const DIGEST_MARKER: &str = ".kit-runtime-digest";
/// In the kit folder: the digest of the kit that the first extraction with digests replaced, written only when it differs
/// from the new one. A build with no marker is older than the kit it was built from, which is at best that older kit.
pub const PRIOR_MARKER: &str = ".kit-runtime-prior";
/// In `<destination>/build/`: the kit digest the build was made from.
pub const BUILT_FROM_MARKER: &str = ".built-from-kit";

/// The folders of the kit whose files end up in a build.
const SOURCE_FOLDERS: [&str; 2] = ["RustPortPS2/native", "RustPortPS2/games"];

/// Whether `relative` (a path inside the kit, with `/`) is generated output: the installer writes it, so it must not count.
fn is_generated(relative: &str) -> bool {
    let parts: Vec<&str> = relative.split('/').collect();
    let Some((file, folders)) = parts.split_last() else { return false };
    folders
        .iter()
        .any(|part| matches!(*part, "runner" | "vu1_generated" | "vu1n_generated") || part.starts_with("build"))
        || (file.starts_with("iopn_") && relative.contains("ps2xIOP/src/native/"))
}

fn hash_file(path: &Path) -> io::Result<[u8; 32]> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().into())
}

fn collect(root: &Path, folder: &Path, files: &mut Vec<(String, PathBuf)>) -> io::Result<()> {
    let Ok(entries) = fs::read_dir(folder) else { return Ok(()) };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let relative = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
        if entry.file_type()?.is_dir() {
            // A generated folder is skipped whole, however deep its content.
            if !is_generated(&format!("{relative}/x")) {
                collect(root, &path, files)?;
            }
        } else if !is_generated(&relative) {
            files.push((relative, path));
        }
    }
    Ok(())
}

/// The digest of the kit at `kit`: the relative path and the content hash of every file under the source folders (without
/// generated output), in sorted order, plus the two prebuilt tools. Touching a file without changing it, or the order the
/// files are visited in, does not change it.
pub fn kit_digest(kit: &Path) -> io::Result<String> {
    let mut files = Vec::new();
    for folder in SOURCE_FOLDERS {
        collect(kit, &kit.join(folder), &mut files)?;
    }
    let exe = if cfg!(windows) { ".exe" } else { "" };
    for tool in ["ps2_recomp", "ps2-tools"] {
        let relative = format!("bin/{tool}{exe}");
        if kit.join(&relative).is_file() {
            files.push((relative.clone(), kit.join(relative)));
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let mut outer = Sha256::new();
    for (relative, path) in &files {
        outer.update(relative.as_bytes());
        outer.update([0]);
        outer.update(hash_file(path)?);
    }
    Ok(hex(&outer.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The first characters of a digest, for the log and the interface.
pub fn short(digest: &str) -> &str {
    &digest[..digest.len().min(12)]
}

fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|text| text.trim().to_string()).filter(|text| !text.is_empty())
}

/// The digest recorded in the kit folder, if any.
pub fn read_kit_digest(kit: &Path) -> Option<String> {
    read_trimmed(&kit.join(DIGEST_MARKER))
}

fn built_directory(destination: &Path) -> PathBuf {
    destination.join("build")
}

/// What the build in `destination` was made from, if it recorded it.
pub fn read_built_from(destination: &Path) -> Option<String> {
    read_trimmed(&built_directory(destination).join(BUILT_FROM_MARKER))
}

/// `true` if `destination` holds a built game (the runtime executable the build produces).
fn has_build(destination: &Path) -> bool {
    built_directory(destination).join("ps2xRuntime").join(EXECUTABLE_NAME).is_file()
}

/// Called after an extraction that changed the kit: records the new digest and, the first time a digest appears over a kit
/// that did not have one, the digest of the kit it replaced (`before`) when that differs. Best effort: a kit that cannot
/// be hashed leaves the player with no prompt, never with a failed start.
pub fn record_kit(kit: &Path, before: Option<String>) {
    let Ok(digest) = kit_digest(kit) else { return };
    let _ = fs::write(kit.join(DIGEST_MARKER), &digest);
    if let Some(before) = before.filter(|before| *before != digest) {
        let _ = fs::write(kit.join(PRIOR_MARKER), before);
    }
}

/// Writes the built-from marker after a successful installation or rebuild, with the kit digest as it is now. Never called
/// for a failure: a failed rebuild keeps the old marker. Does nothing when the kit has no digest (a repository checkout).
pub fn record_build(kit: &Path, destination: &Path) {
    let (Some(digest), true) = (read_kit_digest(kit), built_directory(destination).is_dir()) else { return };
    let _ = fs::write(built_directory(destination).join(BUILT_FROM_MARKER), digest);
}

/// What to do about a build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// The build was made from this kit.
    UpToDate,
    /// No record of what the build was made from, and nothing says it is stale: remember the current digest, ask nothing.
    Adopt,
    /// The build was made from other sources than the kit holds now.
    Needed,
}

/// `built` is the build's marker, `prior` the digest the kit had before the first extraction that recorded digests.
pub fn rebuild_needed(kit: &str, built: Option<&str>, prior: Option<&str>) -> Decision {
    match built {
        Some(built) if built == kit => Decision::UpToDate,
        Some(_) => Decision::Needed,
        None => match prior {
            Some(prior) if prior != kit => Decision::Needed,
            _ => Decision::Adopt,
        },
    }
}

/// What the interface needs to know.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RebuildState {
    pub needed: bool,
    /// The kit digest (short), empty when unknown.
    pub kit: String,
    /// The digest the build was made from (short), empty when it did not record one.
    pub built: String,
    /// `true` the first time this kit digest is reported as needing a rebuild: the interface shows a toast then, once per
    /// digest. Filled by [`announce`], not by [`state`].
    pub announce: bool,
}

/// Compares the kit with the build in `destination`, adopting the kit's digest for a build that predates the marker.
pub fn state(kit: &Path, destination: &Path) -> RebuildState {
    let Some(digest) = read_kit_digest(kit) else { return RebuildState::default() };
    if !has_build(destination) {
        return RebuildState { kit: short(&digest).to_string(), ..RebuildState::default() };
    }
    let built = read_built_from(destination);
    let prior = read_trimmed(&kit.join(PRIOR_MARKER));
    let decision = rebuild_needed(&digest, built.as_deref(), prior.as_deref());
    if decision == Decision::Adopt {
        let _ = fs::write(built_directory(destination).join(BUILT_FROM_MARKER), &digest);
    }
    RebuildState {
        needed: decision == Decision::Needed,
        kit: short(&digest).to_string(),
        built: built.as_deref().map(short).unwrap_or_default().to_string(),
        announce: false,
    }
}

/// Marks `state` as announced when it is the first time this digest needs a rebuild, remembering it in `data` (the launcher's
/// data folder) so the toast appears once per digest and not at every start. "Later" is the toast being left alone: the note
/// in the Performance tab stays until a rebuild succeeds.
pub fn announce(data: &Path, mut state: RebuildState) -> RebuildState {
    let file = data.join("rebuild-announced");
    if state.needed && read_trimmed(&file).as_deref() != Some(state.kit.as_str()) {
        let _ = fs::write(&file, &state.kit);
        state.announce = true;
    }
    state
}

/// The line `logs/optimize.log` gets when a rebuild starts.
pub fn log_line(kit: &Path, destination: &Path) -> String {
    let kit_digest = read_kit_digest(kit);
    format!(
        "[rebuild] kit digest {} vs built {}",
        kit_digest.as_deref().map_or("none", short),
        read_built_from(destination).as_deref().map_or("none", short)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn kit_with_sources(root: &Path) {
        write(&root.join("RustPortPS2/native/ps2xRuntime/src/a.cpp"), "int a;");
        write(&root.join("RustPortPS2/native/CMakeLists.txt"), "project(x)");
        write(&root.join("RustPortPS2/games/scus-97399/funcmap.csv"), "1,2");
    }

    fn built_game(destination: &Path) {
        write(&destination.join("build/ps2xRuntime").join(EXECUTABLE_NAME), "exe");
    }

    #[test]
    fn digest_ignores_generated_output_and_is_stable() {
        let root = tempfile::tempdir().unwrap();
        kit_with_sources(root.path());
        let first = kit_digest(root.path()).unwrap();
        assert_eq!(first, kit_digest(root.path()).unwrap());
        // Rewriting a file with the same content (a new modification time) changes nothing.
        write(&root.path().join("RustPortPS2/native/CMakeLists.txt"), "project(x)");
        assert_eq!(first, kit_digest(root.path()).unwrap());
        // Generated output does not count, wherever it is.
        write(&root.path().join("RustPortPS2/native/ps2xRuntime/src/runner/gen_0001.cpp"), "generated");
        write(&root.path().join("RustPortPS2/native/ps2xIOP/src/native/iopn_0001.cpp"), "generated");
        write(&root.path().join("RustPortPS2/native/build/CMakeCache.txt"), "cache");
        write(&root.path().join("RustPortPS2/native/tools/vu1_generated/x.cpp"), "generated");
        write(&root.path().join("RustPortPS2/native/tools/vu1n_generated/x.cpp"), "generated");
        assert_eq!(first, kit_digest(root.path()).unwrap());
        // A source file that is not generated does: a change, an addition, and a prebuilt tool.
        write(&root.path().join("RustPortPS2/native/ps2xRuntime/src/a.cpp"), "int b;");
        let changed = kit_digest(root.path()).unwrap();
        assert_ne!(first, changed);
        write(&root.path().join("RustPortPS2/native/ps2xIOP/src/native/real.cpp"), "x");
        let added = kit_digest(root.path()).unwrap();
        assert_ne!(changed, added);
        write(&root.path().join(if cfg!(windows) { "bin/ps2-tools.exe" } else { "bin/ps2-tools" }), "tool");
        assert_ne!(added, kit_digest(root.path()).unwrap());
        // Another file the build never reads is not part of it.
        let with_tool = kit_digest(root.path()).unwrap();
        write(&root.path().join("install.cmd"), "echo");
        assert_eq!(with_tool, kit_digest(root.path()).unwrap());
    }

    #[test]
    fn a_missing_marker_adopts_the_current_digest_without_prompting() {
        let root = tempfile::tempdir().unwrap();
        let (kit, destination) = (root.path().join("kit"), root.path().join("game"));
        kit_with_sources(&kit);
        record_kit(&kit, None);
        built_game(&destination);
        assert_eq!(rebuild_needed("d", None, None), Decision::Adopt);
        let first = state(&kit, &destination);
        assert!(!first.needed);
        // The digest was remembered: the build now counts as made from this kit...
        assert_eq!(read_built_from(&destination), read_kit_digest(&kit));
        // ...so the next kit that differs asks.
        write(&kit.join("RustPortPS2/native/ps2xRuntime/src/a.cpp"), "int fixed;");
        record_kit(&kit, None);
        assert!(state(&kit, &destination).needed);
    }

    #[test]
    fn a_changed_digest_asks_and_a_successful_build_clears_it() {
        let root = tempfile::tempdir().unwrap();
        let (kit, destination) = (root.path().join("kit"), root.path().join("game"));
        kit_with_sources(&kit);
        record_kit(&kit, None);
        built_game(&destination);
        // An installation recorded what it was made from.
        record_build(&kit, &destination);
        assert_eq!(rebuild_needed("a", Some("a"), None), Decision::UpToDate);
        assert!(!state(&kit, &destination).needed);
        // A launcher update with different runtime files.
        write(&kit.join("RustPortPS2/native/ps2xRuntime/src/a.cpp"), "int fixed;");
        record_kit(&kit, None);
        let asked = state(&kit, &destination);
        assert!(asked.needed && !asked.kit.is_empty() && !asked.built.is_empty() && asked.kit != asked.built);
        // Asking again changes nothing, and a failed rebuild (which writes no marker) leaves it asking.
        assert!(state(&kit, &destination).needed);
        // A successful rebuild clears it.
        record_build(&kit, &destination);
        let cleared = state(&kit, &destination);
        assert!(!cleared.needed && cleared.kit == cleared.built);
    }

    #[test]
    fn an_update_that_changes_the_runtime_asks_players_whose_build_has_no_marker() {
        let root = tempfile::tempdir().unwrap();
        let (kit, destination) = (root.path().join("kit"), root.path().join("game"));
        kit_with_sources(&kit);
        built_game(&destination);
        // The first launcher with digests replaces a kit whose runtime differs: its digest is the prior one.
        let before = kit_digest(&kit).ok();
        write(&kit.join("RustPortPS2/native/ps2xRuntime/src/a.cpp"), "int fixed;");
        record_kit(&kit, before);
        assert!(state(&kit, &destination).needed);
        // When the runtime did not change, nothing is asked and the build is adopted.
        let (kit, destination) = (root.path().join("kit2"), root.path().join("game2"));
        kit_with_sources(&kit);
        built_game(&destination);
        record_kit(&kit, kit_digest(&kit).ok());
        assert!(!kit.join(PRIOR_MARKER).exists());
        assert!(!state(&kit, &destination).needed);
    }

    #[test]
    fn no_build_or_no_digest_means_nothing_to_ask() {
        let root = tempfile::tempdir().unwrap();
        let (kit, destination) = (root.path().join("kit"), root.path().join("game"));
        kit_with_sources(&kit);
        assert_eq!(state(&kit, &destination), RebuildState::default());
        record_kit(&kit, None);
        // A digest but no game built there: not an installation.
        assert!(!state(&kit, &destination).needed);
        assert!(read_built_from(&destination).is_none());
        // Recording a build where there is no build folder writes nothing.
        record_build(&kit, &destination);
        assert!(!destination.exists());
    }

    #[test]
    fn the_toast_is_announced_once_per_digest() {
        let data = tempfile::tempdir().unwrap();
        let asked = RebuildState { needed: true, kit: "aaa".into(), built: "bbb".into(), announce: false };
        assert!(announce(data.path(), asked.clone()).announce);
        assert!(!announce(data.path(), asked.clone()).announce);
        // A newer kit digest announces again; nothing needed announces nothing.
        assert!(announce(data.path(), RebuildState { kit: "ccc".into(), ..asked.clone() }).announce);
        assert!(!announce(data.path(), RebuildState::default()).announce);
    }

    #[test]
    fn the_log_line_names_both_digests() {
        let root = tempfile::tempdir().unwrap();
        let (kit, destination) = (root.path().join("kit"), root.path().join("game"));
        kit_with_sources(&kit);
        assert_eq!(log_line(&kit, &destination), "[rebuild] kit digest none vs built none");
        record_kit(&kit, None);
        built_game(&destination);
        record_build(&kit, &destination);
        let digest = read_kit_digest(&kit).unwrap();
        assert_eq!(log_line(&kit, &destination), format!("[rebuild] kit digest {0} vs built {0}", short(&digest)));
    }
}
