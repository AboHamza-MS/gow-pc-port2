//! GOW-Port (0.3.1): a CHD disc image (`chdman`'s format, common in emulator collections) turned into an ISO in the wizard.
//!
//! The wizard validates a `.chd` in place (see [`crate::iso::validate_iso`]); before installing, this module writes the ISO
//! inside it next to the CHD, under the CHD's name, with the installer's own reader (`ps2-tools`, no `chdman`). The CHD
//! is only read, every hunk and both SHA-1s are checked, the ISO is written to a `.part` file published only when all of
//! it checks out, and the result goes through the same validation as any ISO. An ISO already converted earlier (same
//! name, valid) is used again instead of converting twice.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::iso::{self, IsoValidation};
use crate::system;

/// Why a CHD could not be converted, as the interface names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChdError {
    /// Not a CHD the reader supports (an old version, a delta of a parent CHD, another codec).
    Unreadable,
    /// A hunk or the SHA-1 does not check out: the file is damaged or incomplete.
    Damaged,
    /// Not enough free space next to the CHD for the ISO.
    Space,
    /// The ISO inside is not a God of War USA the port can install.
    NotGame,
    /// The player stopped it.
    Cancelled,
    /// The ISO could not be written.
    Write,
}

impl ChdError {
    fn from_reader(message: &str) -> Self {
        if message == ps2_tools::chd::CANCELLED {
            Self::Cancelled
        } else if message.contains("damaged") {
            Self::Damaged
        } else if message.starts_with("not a CHD") || message.contains("not supported") || message.contains("parent") {
            Self::Unreadable
        } else {
            Self::Write
        }
    }
}

/// What converting a CHD will do, for the wizard to show before it starts.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChdPlan {
    /// The ISO that will be written (or the one already there).
    pub output: String,
    /// Size of that ISO.
    pub iso_gb: f64,
    /// Free space in the CHD's folder.
    pub free_gb: f64,
    pub enough_space: bool,
    /// The ISO was converted before and is valid: nothing to convert.
    pub existing: bool,
}

/// What a conversion produced.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChdOutcome {
    pub ok: bool,
    /// The ISO (empty on failure).
    pub path: String,
    pub validation: Option<IsoValidation>,
    pub error: Option<ChdError>,
    /// The reader's own message, in English, for the log and a tooltip.
    pub detail: String,
}

impl ChdOutcome {
    fn failed(error: ChdError, detail: impl Into<String>) -> Self {
        Self { ok: false, path: String::new(), validation: None, error: Some(error), detail: detail.into() }
    }
}

/// The ISO converted before, when it is there and valid.
fn existing_iso(chd: &Path, validate: &impl Fn(&Path) -> IsoValidation) -> Option<PathBuf> {
    let candidate = chd.with_extension("iso");
    (candidate.is_file() && validate(&candidate).ok).then_some(candidate)
}

/// Where the ISO goes: next to the CHD with its name ("God of War.chd" gives "God of War.iso"), with " (2)", " (3)"...
/// when that name is taken by another file.
pub fn output_path(chd: &Path) -> PathBuf {
    let folder = chd.parent().map(Path::to_path_buf).unwrap_or_default();
    let stem = chd.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_else(|| "God of War".into());
    let mut candidate = folder.join(format!("{stem}.iso"));
    let mut number = 2;
    while candidate.exists() {
        candidate = folder.join(format!("{stem} ({number}).iso"));
        number += 1;
    }
    candidate
}

/// Reads the CHD's header and says where the ISO will go and whether it fits, decoding nothing.
pub fn plan(chd: &Path) -> Result<ChdPlan, ChdError> {
    plan_with(chd, |path| iso::validate_iso(&path.to_string_lossy()))
}

fn plan_with(chd: &Path, validate: impl Fn(&Path) -> IsoValidation) -> Result<ChdPlan, ChdError> {
    let info = ps2_tools::chd::info(chd).map_err(|message| ChdError::from_reader(&message))?;
    let iso_gb = info.iso_bytes as f64 / 1024f64.powi(3);
    let folder = chd.parent().map(Path::to_path_buf).unwrap_or_default();
    let free_gb = system::free_space_gb(&folder);
    if let Some(existing) = existing_iso(chd, &validate) {
        return Ok(ChdPlan {
            output: existing.to_string_lossy().into_owned(),
            iso_gb,
            free_gb,
            enough_space: true,
            existing: true,
        });
    }
    Ok(ChdPlan {
        output: output_path(chd).to_string_lossy().into_owned(),
        iso_gb,
        free_gb,
        // 256 MB more for the file system.
        enough_space: free_gb >= iso_gb + 0.25,
        existing: false,
    })
}

/// Converts `chd` to an ISO next to it. `progress` gets a fraction and returns `false` to stop.
pub fn convert(chd: &Path, progress: impl FnMut(f64) -> bool) -> ChdOutcome {
    convert_with(chd, progress, |path| iso::validate_iso(&path.to_string_lossy()))
}

/// [`convert`] with the validation of the result given, for the tests.
pub fn convert_with(
    chd: &Path,
    mut progress: impl FnMut(f64) -> bool,
    validate: impl Fn(&Path) -> IsoValidation,
) -> ChdOutcome {
    let plan = match plan_with(chd, &validate) {
        Ok(plan) => plan,
        Err(error) => return ChdOutcome::failed(error, chd.display().to_string()),
    };
    if plan.existing {
        progress(1.0);
        let validation = validate(Path::new(&plan.output));
        return ChdOutcome {
            ok: true,
            path: plan.output,
            validation: Some(validation),
            error: None,
            detail: String::new(),
        };
    }
    if !plan.enough_space {
        return ChdOutcome::failed(ChdError::Space, format!("{:.1} GB needed next to the CHD", plan.iso_gb + 0.25));
    }
    let output = PathBuf::from(&plan.output);
    let converted = ps2_tools::chd::extract_iso(chd, &output, 0, |done, total| {
        progress((done as f64 / total.max(1) as f64).min(0.99))
    });
    if let Err(message) = converted {
        return ChdOutcome::failed(ChdError::from_reader(&message), message);
    }
    let validation = validate(&output);
    if !validation.ok {
        // Another game or a modified executable: the port cannot install it, so nothing is left behind.
        let _ = std::fs::remove_file(&output);
        return ChdOutcome { validation: Some(validation), ..ChdOutcome::failed(ChdError::NotGame, "") };
    }
    progress(1.0);
    ChdOutcome { ok: true, path: plan.output, validation: Some(validation), error: None, detail: String::new() }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::fs;

    fn valid() -> IsoValidation {
        IsoValidation { ok: true, reason: None, hash: "x".into(), translation: false, edition: String::new() }
    }

    /// A one-hunk CHD v5 stored uncompressed (no codec, a plain map), holding `data` as a DVD image.
    pub(crate) fn uncompressed_chd(data: &[u8]) -> Vec<u8> {
        let hunk = data.len() as u32;
        let mut file = vec![0u8; 124];
        file[..8].copy_from_slice(b"MComprHD");
        file[8..12].copy_from_slice(&124u32.to_be_bytes());
        file[12..16].copy_from_slice(&5u32.to_be_bytes());
        file[32..40].copy_from_slice(&(data.len() as u64).to_be_bytes());
        file[40..48].copy_from_slice(&124u64.to_be_bytes()); // map right after the header
        file[56..60].copy_from_slice(&hunk.to_be_bytes());
        file[60..64].copy_from_slice(&2048u32.to_be_bytes());
        let raw_sha1 = ps2_tools::hash::Sha1::digest(data);
        file[64..84].copy_from_slice(&raw_sha1);
        file[84..104].copy_from_slice(&ps2_tools::hash::Sha1::digest(&raw_sha1));
        // The map: hunk 0 at block 1 (block = hunk-sized unit of the file), then the data at that block.
        file.extend_from_slice(&1u32.to_be_bytes());
        file.resize(hunk as usize, 0);
        file.extend_from_slice(data);
        file
    }

    #[test]
    fn a_chd_is_converted_next_to_itself_and_kept() {
        let dir = tempfile::tempdir().unwrap();
        let chd = dir.path().join("God of War.chd");
        let data: Vec<u8> = (0..4096u32).map(|i| (i % 251) as u8).collect();
        let bytes = uncompressed_chd(&data);
        fs::write(&chd, &bytes).unwrap();
        let mut fractions = Vec::new();
        let outcome = convert_with(
            &chd,
            |fraction| {
                fractions.push(fraction);
                true
            },
            |path| if path.exists() { valid() } else { IsoValidation { ok: false, ..valid() } },
        );
        assert!(outcome.ok, "{outcome:?}");
        assert_eq!(PathBuf::from(&outcome.path), dir.path().join("God of War.iso"));
        assert_eq!(fs::read(&outcome.path).unwrap(), data);
        assert_eq!(fs::read(&chd).unwrap(), bytes, "the CHD is only read");
        assert_eq!(fractions.last(), Some(&1.0));

        // Converted again: the valid ISO already there is used, not overwritten.
        let again = convert_with(
            &chd,
            |_| true,
            |path| if path.exists() { valid() } else { IsoValidation { ok: false, ..valid() } },
        );
        assert!(again.ok);
        assert_eq!(again.path, outcome.path);
        assert!(plan_with(&chd, |_| valid()).unwrap().existing);
    }

    #[test]
    fn failures_are_named_and_leave_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let chd = dir.path().join("game.chd");
        let invalid = || IsoValidation { ok: false, reason: Some(iso::IsoReason::OtherGame), ..valid() };

        fs::write(&chd, b"not a chd at all").unwrap();
        assert_eq!(convert_with(&chd, |_| true, |_| invalid()).error, Some(ChdError::Unreadable));

        let data = vec![7u8; 2048];
        let mut bytes = uncompressed_chd(&data);
        fs::write(&chd, &bytes).unwrap();
        let other = convert_with(&chd, |_| true, |_| invalid());
        assert_eq!(other.error, Some(ChdError::NotGame));
        assert_eq!(other.validation.unwrap().reason, Some(iso::IsoReason::OtherGame));
        assert!(!dir.path().join("game.iso").exists(), "an ISO of another game is deleted");

        assert_eq!(convert_with(&chd, |_| false, |_| valid()).error, Some(ChdError::Cancelled));

        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        fs::write(&chd, &bytes).unwrap();
        assert_eq!(convert_with(&chd, |_| true, |_| valid()).error, Some(ChdError::Damaged));
        let leftovers: Vec<_> =
            fs::read_dir(dir.path()).unwrap().filter_map(Result::ok).map(|e| e.file_name()).collect();
        assert_eq!(leftovers, vec![std::ffi::OsString::from("game.chd")], "no ISO and no .part stay behind");
    }

    #[test]
    fn the_output_takes_the_chd_name_and_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let chd = dir.path().join("God of War (USA).chd");
        assert_eq!(output_path(&chd), dir.path().join("God of War (USA).iso"));
        fs::write(dir.path().join("God of War (USA).iso"), b"something else").unwrap();
        assert_eq!(output_path(&chd), dir.path().join("God of War (USA) (2).iso"));
    }
}
