//! GOW-Port (0.3.0): an xdelta3 / VCDIFF patch (a translation, an undub, a data mod) applied to the original ISO in the wizard.
//!
//! The player picks the original USA ISO, ticks "apply a patch" and chooses the `.xdelta` file. The decoder is the
//! installer's own (`ps2-tools`, no external program). The patched ISO is written next to the original under the patch's
//! name, the original is only read, and the new image goes through the same validation as any ISO: a patch that changes
//! the game's code is refused and its output deleted.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::iso::{self, IsoValidation};
use crate::system;

/// Why a patch could not be applied, as the interface names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PatchError {
    /// The patch file cannot be opened.
    PatchUnreadable,
    /// Not an xdelta3 / VCDIFF file.
    NotXdelta,
    /// Made with secondary compression (`xdelta3 -S`), which the decoder does not read.
    Compressed,
    /// Made for another ISO: the first block does not match.
    WrongIso,
    /// The patch is cut or damaged.
    Damaged,
    /// Not enough free space next to the ISO for the patched copy.
    Space,
    /// The result changes the game's executable (a code patch: widescreen hack, cheats, fixes): refused and deleted.
    ChangesCode,
    /// The player cancelled it.
    Cancelled,
    /// The patched ISO could not be written.
    Write,
}

impl PatchError {
    /// Sorts a message of the decoder into the reason the interface explains.
    fn from_decoder(message: &str) -> Self {
        if message == ps2_tools::xdelta::CANCELLED {
            Self::Cancelled
        } else if message.contains("secondary compression") || message.contains("compresses its sections") {
            Self::Compressed
        } else if message.contains("not an xdelta3") {
            Self::NotXdelta
        } else if message.contains("different ISO") {
            Self::WrongIso
        } else if message.contains("damaged") {
            Self::Damaged
        } else {
            Self::Write
        }
    }
}

/// What applying a patch produced.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchOutcome {
    pub ok: bool,
    /// The patched ISO (empty on failure).
    pub path: String,
    /// Its validation (a translation when it worked).
    pub validation: Option<IsoValidation>,
    pub error: Option<PatchError>,
    /// The decoder's own message, in English, for the log and a tooltip.
    pub detail: String,
}

impl PatchOutcome {
    fn failed(error: PatchError, detail: impl Into<String>) -> Self {
        Self { ok: false, path: String::new(), validation: None, error: Some(error), detail: detail.into() }
    }
}

/// Checks a chosen patch from its header alone, so the wizard answers at once.
pub fn check_patch(patch: &Path) -> Result<(), PatchError> {
    if !patch.is_file() {
        return Err(PatchError::PatchUnreadable);
    }
    ps2_tools::xdelta::check_header(patch).map_err(|message| match PatchError::from_decoder(&message) {
        PatchError::Write => PatchError::PatchUnreadable,
        other => other,
    })
}

/// Where the patched ISO goes: next to the original, named after the patch ("God of War PT-BR.xdelta" gives
/// "God of War PT-BR.iso"), with " (2)", " (3)"... when that name is taken. Never the original's own path.
pub fn output_path(iso: &Path, patch: &Path) -> PathBuf {
    let folder = iso.parent().map(Path::to_path_buf).unwrap_or_default();
    let stem = patch
        .file_stem()
        .map(|stem| stem.to_string_lossy().trim().to_string())
        .filter(|stem| !stem.is_empty())
        .unwrap_or_else(|| {
            let original = iso.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
            format!("{original} (patched)")
        });
    let taken = |candidate: &Path| {
        candidate.exists() || candidate.to_string_lossy().eq_ignore_ascii_case(&iso.to_string_lossy())
    };
    let mut candidate = folder.join(format!("{stem}.iso"));
    let mut number = 2;
    while taken(&candidate) {
        candidate = folder.join(format!("{stem} ({number}).iso"));
        number += 1;
    }
    candidate
}

/// Applies `patch` to `original` (the validated original ISO), writing the patched ISO next to it. `progress` gets a
/// fraction (estimated from the original's size, as a patched disc is about as big) and returns `false` to cancel.
pub fn apply(original: &Path, patch: &Path, progress: impl FnMut(f64) -> bool) -> PatchOutcome {
    apply_with(original, patch, progress, |path| iso::validate_iso(&path.to_string_lossy()))
}

/// [`apply`] with the validation of the result given, for the tests.
pub fn apply_with(
    original: &Path,
    patch: &Path,
    mut progress: impl FnMut(f64) -> bool,
    validate: impl Fn(&Path) -> IsoValidation,
) -> PatchOutcome {
    if let Err(error) = check_patch(patch) {
        return PatchOutcome::failed(error, format!("{}", patch.display()));
    }
    let size = std::fs::metadata(original).map(|meta| meta.len()).unwrap_or(0);
    let output = output_path(original, patch);
    let folder = output.parent().map(Path::to_path_buf).unwrap_or_default();
    // The copy is about as big as the original; 256 MB more for the file system.
    let needed_gb = (size as f64 + 256.0 * 1024.0 * 1024.0) / 1024f64.powi(3);
    if size > 0 && system::free_space_gb(&folder) < needed_gb {
        return PatchOutcome::failed(PatchError::Space, format!("{needed_gb:.1} GB needed in {}", folder.display()));
    }
    let total = size.max(1) as f64;
    let decoded = ps2_tools::xdelta::apply_cancellable(patch, original, &output, |bytes| {
        progress((bytes as f64 / total).min(0.99))
    });
    if let Err(message) = decoded {
        return PatchOutcome::failed(PatchError::from_decoder(&message), message);
    }
    let validation = validate(&output);
    if !validation.ok {
        // A patch that changes the code is a mod the port cannot run: nothing is left behind.
        let _ = std::fs::remove_file(&output);
        return PatchOutcome::failed(PatchError::ChangesCode, format!("{:?}", validation.reason));
    }
    progress(1.0);
    PatchOutcome {
        ok: true,
        path: output.to_string_lossy().into_owned(),
        validation: Some(validation),
        error: None,
        detail: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iso::IsoReason;
    use std::fs;

    fn varint(value: u64) -> Vec<u8> {
        let mut digits = vec![(value & 0x7F) as u8];
        let mut rest = value >> 7;
        while rest != 0 {
            digits.push((rest & 0x7F) as u8 | 0x80);
            rest >>= 7;
        }
        digits.reverse();
        digits
    }

    /// A one-window patch over "abcdefghijklmnop": copies "abcd", then adds "WXYZ". No checksum, so any 16-byte base
    /// decodes, and a shorter base is "another ISO".
    fn patch() -> Vec<u8> {
        let (data, inst, addr) = (b"WXYZ".to_vec(), vec![20u8, 5], vec![0u8]);
        let mut body = varint(8);
        body.push(0);
        for section in [&data, &inst, &addr] {
            body.extend(varint(section.len() as u64));
        }
        for section in [&data, &inst, &addr] {
            body.extend_from_slice(section);
        }
        let mut file = vec![0xD6, 0xC3, 0xC4, 0x00, 0x00, 0x01];
        file.extend(varint(16));
        file.extend(varint(0));
        file.extend(varint(body.len() as u64));
        file.extend(body);
        file
    }

    fn translation() -> IsoValidation {
        IsoValidation { ok: true, reason: None, hash: "x".into(), translation: true, edition: String::new() }
    }

    #[test]
    fn the_patched_iso_goes_next_to_the_original_and_never_over_it() {
        let dir = tempfile::tempdir().unwrap();
        let iso = dir.path().join("God of War.iso");
        assert_eq!(
            output_path(&iso, Path::new("C:/d/God of War PT-BR.xdelta")),
            dir.path().join("God of War PT-BR.iso")
        );
        // A patch named like the original gets another name.
        fs::write(&iso, b"x").unwrap();
        assert_eq!(output_path(&iso, Path::new("God of War.xdelta")), dir.path().join("God of War (2).iso"));
        assert_eq!(output_path(&iso, Path::new("   .xdelta")), dir.path().join("God of War (patched).iso"));
    }

    #[test]
    fn a_patch_is_applied_validated_and_the_original_kept() {
        let dir = tempfile::tempdir().unwrap();
        let (iso, patch_path) = (dir.path().join("God of War.iso"), dir.path().join("PT-BR.xdelta"));
        fs::write(&iso, b"abcdefghijklmnop").unwrap();
        fs::write(&patch_path, patch()).unwrap();
        let mut fractions = Vec::new();
        let outcome = apply_with(
            &iso,
            &patch_path,
            |f| {
                fractions.push(f);
                true
            },
            |_| translation(),
        );
        assert!(outcome.ok, "{outcome:?}");
        assert_eq!(fs::read(dir.path().join("PT-BR.iso")).unwrap(), b"abcdWXYZ");
        assert_eq!(fs::read(&iso).unwrap(), b"abcdefghijklmnop", "the original is only read");
        assert_eq!(fractions.last(), Some(&1.0));
        assert!(outcome.validation.unwrap().translation);
    }

    #[test]
    fn failures_are_named_and_leave_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let (iso, patch_path) = (dir.path().join("God of War.iso"), dir.path().join("PT-BR.xdelta"));
        fs::write(&iso, b"abcdefghijklmnop").unwrap();
        let run = |bytes: &[u8], validation: IsoValidation| {
            fs::write(&patch_path, bytes).unwrap();
            let outcome = apply_with(&iso, &patch_path, |_| true, |_| validation.clone());
            assert!(!dir.path().join("PT-BR.iso").exists() && !dir.path().join("PT-BR.iso.part").exists());
            outcome.error
        };
        assert_eq!(run(b"PK\x03\x04", translation()), Some(PatchError::NotXdelta));
        assert_eq!(run(&[0xD6, 0xC3, 0xC4, 0, 0x01, 0], translation()), Some(PatchError::Compressed));
        let modified = IsoValidation { ok: false, reason: Some(IsoReason::Modified), ..translation() };
        assert_eq!(run(&patch(), modified), Some(PatchError::ChangesCode));
        // Cancelled on the first block.
        fs::write(&patch_path, patch()).unwrap();
        assert_eq!(apply_with(&iso, &patch_path, |_| false, |_| translation()).error, Some(PatchError::Cancelled));
        // A base shorter than the patch reads is another ISO.
        fs::write(&iso, b"abc").unwrap();
        assert_eq!(apply_with(&iso, &patch_path, |_| true, |_| translation()).error, Some(PatchError::WrongIso));
        assert_eq!(check_patch(&dir.path().join("missing.xdelta")), Err(PatchError::PatchUnreadable));
        assert_eq!(fs::read(&iso).unwrap(), b"abc", "the original is never written");
    }
}
