//! Structural inspection of the user's game ELF with the parser of the Rust recompiler (`RustPortPS2`).
//!
//! This is the launcher's first use of the migration: the same reader that will translate the game also reports what
//! the file contains, so a wrong dump — or a `funcmap.csv` exported from another build — is visible before an
//! installation spends minutes compiling.
//!
//! The report is deliberately flat (`ok` + `error` strings, like [`crate::iso`]) so it can travel to the interface
//! unchanged when the panel lands.

use std::fs;
use std::path::Path;

use ps2_elf::{ElfFile, FunctionMap};
use serde::Serialize;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameElfReport {
    /// The ELF parsed. When a function map was given, it also has to be readable.
    pub ok: bool,
    pub error: String,
    pub entry: String,
    pub sections: usize,
    pub code_bytes: u32,
    pub has_code: bool,
    /// Functions accepted from the Ghidra map, if one was given.
    pub map_functions: usize,
    /// Map entries dropped because their start is outside a code section.
    pub map_outside_executable: usize,
}

/// Inspect `elf_path`, optionally cross-checking it against a Ghidra `funcmap.csv`.
pub fn inspect(elf_path: &Path, funcmap_path: Option<&Path>) -> GameElfReport {
    let mut report = GameElfReport::default();

    let bytes = match fs::read(elf_path) {
        Ok(bytes) => bytes,
        Err(error) => {
            report.error = format!("cannot read {}: {error}", elf_path.display());
            return report;
        }
    };
    let elf = match ElfFile::parse(&bytes) {
        Ok(elf) => elf,
        Err(error) => {
            report.error = format!("{}: {error}", elf_path.display());
            return report;
        }
    };

    report.ok = true;
    report.entry = format!("0x{:08X}", elf.entry());
    report.sections = elf.sections().len();
    report.code_bytes = elf.sections().iter().filter(|section| section.is_code).map(|section| section.size).sum();
    report.has_code = elf.has_any_executable_section();

    if let Some(path) = funcmap_path {
        match fs::read_to_string(path) {
            Ok(text) => {
                let map = FunctionMap::from_ghidra_csv(&text, &elf);
                report.map_functions = map.len();
                report.map_outside_executable = map.rejected_outside_executable();
            }
            Err(error) => {
                report.ok = false;
                report.error = format!("cannot read {}: {error}", path.display());
            }
        }
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A MIPS ELF header with no sections and no segments: enough to exercise the report wiring.
    fn header_only_elf(entry: u32) -> Vec<u8> {
        let mut bytes = vec![0u8; 52];
        bytes[0..4].copy_from_slice(b"\x7fELF");
        bytes[4] = 1; // ELFCLASS32
        bytes[5] = 1; // little-endian
        bytes[6] = 1; // version
        bytes[16..18].copy_from_slice(&2u16.to_le_bytes()); // ET_EXEC
        bytes[18..20].copy_from_slice(&8u16.to_le_bytes()); // EM_MIPS
        bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
        bytes[24..28].copy_from_slice(&entry.to_le_bytes());
        bytes[40..42].copy_from_slice(&52u16.to_le_bytes());
        bytes[46..48].copy_from_slice(&40u16.to_le_bytes());
        bytes
    }

    #[test]
    fn reports_the_entry_point_of_a_mips_elf() {
        let directory = tempfile::tempdir().unwrap();
        let elf = directory.path().join("SCUS_973.99");
        fs::write(&elf, header_only_elf(0x0010_0008)).unwrap();

        let report = inspect(&elf, None);
        assert!(report.ok, "{}", report.error);
        assert_eq!(report.entry, "0x00100008");
        assert_eq!(report.sections, 0);
        assert!(!report.has_code);
        assert_eq!(report.map_functions, 0);
    }

    #[test]
    fn refuses_a_file_that_is_not_a_ps2_elf() {
        let directory = tempfile::tempdir().unwrap();
        let elf = directory.path().join("garbage.bin");
        fs::write(&elf, b"not an elf").unwrap();

        let report = inspect(&elf, None);
        assert!(!report.ok);
        assert!(report.error.contains("garbage.bin"), "{}", report.error);
        assert_eq!(report.entry, "");
    }

    #[test]
    fn a_map_that_matches_nothing_is_reported() {
        let directory = tempfile::tempdir().unwrap();
        let elf = directory.path().join("SCUS_973.99");
        fs::write(&elf, header_only_elf(0x0010_0008)).unwrap();
        let map = directory.path().join("funcmap.csv");
        fs::write(&map, "Name,Start,End,Size\nmain,0x00100000,0x00100010,16\nhelper,0x00100010,0x00100018,8\n")
            .unwrap();

        let report = inspect(&elf, Some(&map));
        assert!(report.ok, "{}", report.error);
        // Without code sections every row is rejected, which is what a map from
        // another build looks like.
        assert_eq!(report.map_functions, 0);
        assert_eq!(report.map_outside_executable, 2);
    }

    #[test]
    fn a_missing_map_is_an_error() {
        let directory = tempfile::tempdir().unwrap();
        let elf = directory.path().join("SCUS_973.99");
        fs::write(&elf, header_only_elf(0x0010_0008)).unwrap();

        let report = inspect(&elf, Some(&directory.path().join("missing.csv")));
        assert!(!report.ok);
        assert!(report.error.contains("missing.csv"), "{}", report.error);
    }
}
