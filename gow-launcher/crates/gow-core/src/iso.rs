//! Validation of the God of War (USA) disc image by its RetroAchievements hash, and the search for it on the disks.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use md5::{Digest, Md5};
use serde::Serialize;

/// RetroAchievements PS2 hash for God of War (USA, SCUS-97399): MD5 of "SCUS_973.99" + the ELF bytes. A modified ISO
/// (translation, patches) yields another hash and does not serve as a source for the recompiled game.
pub const HASH_USA: &str = "33c1808ac082e236188fb887d97b7688";
const SECTOR: u64 = 2048;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum IsoReason {
    Empty,
    NotFound,
    Unreadable,
    Format,
    OtherGame,
    NoExecutable,
    Modified,
    /// GOW-Port (decision D4): God of War from another region or a demo: recognised and named, not supported yet.
    OtherEdition,
    /// GOW-Port (0.3.1): a `.chd` the port cannot read (damaged, a delta of a parent CHD, an old version or codec).
    Chd,
}

/// GOW-Port (decision D4): the other God of War boot executables, from PCSX2's GameIndex. They have their own code, so the
/// USA function map does not apply to them; the wizard names them instead of saying "not God of War".
pub const OTHER_EDITIONS: [(&str, &str); 6] = [
    ("SLPM_670.10", "Japan"),
    ("SCES_531.33", "Europe / Australia"),
    ("SCES_515.33", "Europe, alternate serial"),
    ("SCAJ_300.10", "Asia, SCAJ-30010"),
    ("SCUS_974.67", "USA demo"),
    ("SCED_534.31", "Europe demo"),
];

/// The label of a known God of War edition the port does not support yet, by its boot executable.
pub fn other_edition(boot: &str) -> Option<&'static str> {
    OTHER_EDITIONS.iter().find(|(name, _)| name.eq_ignore_ascii_case(boot)).map(|(_, label)| *label)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IsoValidation {
    pub ok: bool,
    pub reason: Option<IsoReason>,
    pub hash: String,
    /// GOW-Port (decision D4): accepted because the executable has the USA code (`.text`) without being the original
    /// disc's: a translation. It installs and plays the same; RetroAchievements stay off.
    pub translation: bool,
    /// GOW-Port: with `OtherEdition`, which one ("Japan", "Europe / Australia"...).
    pub edition: String,
}

impl IsoValidation {
    fn failed(reason: IsoReason) -> Self {
        Self { ok: false, reason: Some(reason), hash: String::new(), translation: false, edition: String::new() }
    }
}

fn le32(data: &[u8], offset: usize) -> u64 {
    data.get(offset..offset + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as u64)
}

struct Entry {
    lba: u64,
    size: u64,
}

/// Looks for `name` (without ";version") in an ISO 9660 directory already read into memory.
fn find_entry(directory: &[u8], name: &str) -> Option<Entry> {
    let mut pos = 0usize;
    while pos + 33 < directory.len() {
        let length = directory[pos] as usize;
        if length == 0 {
            pos = (pos / SECTOR as usize + 1) * SECTOR as usize; // records do not cross sectors
            continue;
        }
        let name_length = directory[pos + 32] as usize;
        if pos + 33 + name_length > directory.len() {
            break;
        }
        let raw = &directory[pos + 33..pos + 33 + name_length];
        let text: String = raw.iter().map(|&b| b as char).collect();
        let current = text.split(';').next().unwrap_or("");
        if current.eq_ignore_ascii_case(name) {
            return Some(Entry { lba: le32(directory, pos + 2), size: le32(directory, pos + 10) });
        }
        pos += length;
    }
    None
}

/// The name after `BOOT2 = cdrom0:` in SYSTEM.CNF.
fn boot_executable(system_cnf: &str) -> Option<String> {
    let upper = system_cnf.to_ascii_uppercase();
    let at = upper.find("BOOT2")?;
    let rest = system_cnf[at + 5..].trim_start();
    let rest = rest.strip_prefix('=')?.trim_start();
    let rest = rest.get(..7).filter(|p| p.eq_ignore_ascii_case("cdrom0:")).map(|_| &rest[7..])?;
    let rest = rest.trim_start_matches('\\');
    let end = rest.find(|c: char| c.is_whitespace() || c == ';').unwrap_or(rest.len());
    (end > 0).then(|| rest[..end].to_string())
}

/// The 2,048-byte sectors of a disc image: an ISO file, or (GOW-Port 0.3.1) the ISO inside a CHD, read in place.
trait Sectors {
    /// `bytes` from sector `lba` on; shorter (or empty) past the end or on a read error.
    fn read_at(&mut self, lba: u64, bytes: u64) -> Vec<u8>;
}

impl Sectors for File {
    fn read_at(&mut self, lba: u64, bytes: u64) -> Vec<u8> {
        let mut buffer = vec![0u8; bytes as usize];
        if self.seek(SeekFrom::Start(lba * SECTOR)).is_err() {
            return Vec::new();
        }
        let mut filled = 0;
        while filled < buffer.len() {
            match self.read(&mut buffer[filled..]) {
                Ok(0) | Err(_) => break,
                Ok(count) => filled += count,
            }
        }
        buffer.truncate(filled);
        buffer
    }
}

impl Sectors for ps2_tools::chd::Disc {
    fn read_at(&mut self, lba: u64, bytes: u64) -> Vec<u8> {
        self.read(lba, bytes).unwrap_or_default()
    }
}

/// GOW-Port (0.3.1): a CHD disc image (MAME's `chdman` format), by its extension.
pub fn is_chd(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("chd"))
}

/// Validates an ISO, or the ISO inside a `.chd` (read in place, without converting it).
pub fn validate_iso(path: &str) -> IsoValidation {
    let path = path.trim();
    if path.is_empty() {
        return IsoValidation::failed(IsoReason::Empty);
    }
    if !Path::new(path).is_file() {
        return IsoValidation::failed(IsoReason::NotFound);
    }
    if is_chd(Path::new(path)) {
        return match ps2_tools::chd::Disc::open(Path::new(path)) {
            Ok(mut disc) => validate_sectors(&mut disc),
            Err(_) => IsoValidation::failed(IsoReason::Chd),
        };
    }
    let Ok(mut file) = File::open(path) else {
        return IsoValidation::failed(IsoReason::Unreadable);
    };
    validate_sectors(&mut file)
}

fn validate_sectors(file: &mut impl Sectors) -> IsoValidation {
    // Primary volume descriptor: sector 16, type 1, "CD001". Images with 2352-byte sectors are not read here.
    let descriptor = file.read_at(16, SECTOR);
    if descriptor.len() < SECTOR as usize || descriptor[0] != 1 || &descriptor[1..6] != b"CD001" {
        return IsoValidation::failed(IsoReason::Format);
    }
    let root_lba = le32(&descriptor, 156 + 2);
    let root_size = le32(&descriptor, 156 + 10).min(1 << 20);
    let root = file.read_at(root_lba, root_size);
    if root.is_empty() {
        return IsoValidation::failed(IsoReason::Format);
    }

    let Some(cnf) = find_entry(&root, "SYSTEM.CNF") else {
        return IsoValidation::failed(IsoReason::OtherGame);
    };
    let text = file.read_at(cnf.lba, cnf.size.min(SECTOR));
    let text: String = text.iter().map(|&b| b as char).collect();
    let boot = boot_executable(&text);
    if let Some(label) = boot.as_deref().and_then(other_edition) {
        return IsoValidation { edition: label.to_owned(), ..IsoValidation::failed(IsoReason::OtherEdition) };
    }
    let Some(name) = boot.filter(|name| name.eq_ignore_ascii_case("SCUS_973.99")) else {
        return IsoValidation::failed(IsoReason::OtherGame);
    };

    let Some(exe) = find_entry(&root, &name).filter(|e| e.size > 0 && e.size <= 64 << 20) else {
        return IsoValidation::failed(IsoReason::NoExecutable);
    };
    let elf = file.read_at(exe.lba, exe.size);
    if elf.len() as u64 != exe.size {
        return IsoValidation::failed(IsoReason::NoExecutable);
    }

    let mut md5 = Md5::new();
    md5.update(name.as_bytes());
    md5.update(&elf);
    let hash = crate::game::hex(&md5.finalize());
    if hash != HASH_USA {
        // GOW-Port (decision D4): a fan translation keeps the USA code byte for byte and recompiles to the same C++.
        let translation = ps2_elf::code_identity::identify(&elf) == ps2_elf::code_identity::CodeIdentity::SameCode;
        let reason = (!translation).then_some(IsoReason::Modified);
        return IsoValidation { ok: translation, reason, hash, translation, edition: String::new() };
    }
    IsoValidation { ok: true, reason: None, hash, translation: false, edition: String::new() }
}

const SKIPPED_FOLDERS: [&str; 7] = [
    "windows",
    "program files",
    "program files (x86)",
    "programdata",
    "$recycle.bin",
    "system volume information",
    "node_modules",
];

fn scan(directory: &Path, depth: u32, deadline: Instant) -> Option<PathBuf> {
    if Instant::now() > deadline {
        return None;
    }
    let entries: Vec<_> = std::fs::read_dir(directory).ok()?.filter_map(Result::ok).collect();
    let files = |extension: &str| {
        let mut found: Vec<PathBuf> = entries
            .iter()
            .map(|entry| entry.path())
            .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case(extension)))
            .collect();
        found.sort();
        found
    };
    // GOW-Port (0.3.1): an ISO first; then a CHD (a compressed DVD is 1-10 GB), validated in place.
    let candidates =
        files("iso").into_iter().map(|iso| (iso, 3u64 << 30)).chain(files("chd").into_iter().map(|chd| (chd, 1 << 30)));
    for (image, smallest) in candidates {
        let size = std::fs::metadata(&image).map(|m| m.len()).unwrap_or(0);
        if size > smallest && size < 10 << 30 && validate_iso(&image.to_string_lossy()).ok {
            return Some(image);
        }
    }
    if depth == 0 {
        return None;
    }
    let mut folders: Vec<PathBuf> = entries
        .iter()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir() && !kind.is_symlink()))
        .map(|entry| entry.path())
        .filter(|path| {
            let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            !SKIPPED_FOLDERS.contains(&name.as_str())
        })
        .collect();
    folders.sort();
    folders.into_iter().find_map(|folder| scan(&folder, depth - 1, deadline))
}

/// Looks in the user's folders and on the disks for a DVD ISO (3-10 GB) or CHD that is God of War USA.
/// Gives up after 25 seconds.
pub fn find_original_iso() -> Option<PathBuf> {
    let deadline = Instant::now() + Duration::from_secs(25);
    let home = dirs::home_dir()?;
    let user_folders =
        [dirs::download_dir(), dirs::desktop_dir(), dirs::document_dir(), Some(home.join("Games")), Some(home.clone())];
    for folder in user_folders.into_iter().flatten().filter(|f| f.is_dir()) {
        let depth = if folder == home { 1 } else { 3 };
        if let Some(found) = scan(&folder, depth, deadline) {
            return Some(found);
        }
    }
    for root in volume_roots() {
        if let Some(found) = scan(&root, 2, deadline) {
            return Some(found);
        }
    }
    None
}

#[cfg(windows)]
fn volume_roots() -> Vec<PathBuf> {
    ('A'..='Z').map(|letter| PathBuf::from(format!("{letter}:\\"))).filter(|root| root.exists()).collect()
}

#[cfg(not(windows))]
fn volume_roots() -> Vec<PathBuf> {
    vec![PathBuf::from("/mnt"), PathBuf::from("/media")]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A tiny ISO 9660 image: descriptor in sector 16, root in 17, SYSTEM.CNF in 18, the executable in 19.
    fn build_iso(boot: &str, executable: &[u8]) -> Vec<u8> {
        let mut image = vec![0u8; 20 * SECTOR as usize];
        let put = |image: &mut Vec<u8>, sector: usize, bytes: &[u8]| {
            image[sector * SECTOR as usize..sector * SECTOR as usize + bytes.len()].copy_from_slice(bytes);
        };
        let record = |name: &str, lba: u32, size: u32| {
            let mut r = vec![0u8; 33 + name.len()];
            r[0] = r.len() as u8;
            r[2..6].copy_from_slice(&lba.to_le_bytes());
            r[10..14].copy_from_slice(&size.to_le_bytes());
            r[32] = name.len() as u8;
            r[33..].copy_from_slice(name.as_bytes());
            r
        };
        let cnf = format!("BOOT2 = cdrom0:\\{boot};1\r\nVER = 1.00\r\n");
        let mut root = Vec::new();
        root.extend(record("SYSTEM.CNF;1", 18, cnf.len() as u32));
        root.extend(record(&format!("{boot};1"), 19, executable.len() as u32));
        let mut descriptor = vec![0u8; SECTOR as usize];
        descriptor[0] = 1;
        descriptor[1..6].copy_from_slice(b"CD001");
        descriptor[156 + 2..156 + 6].copy_from_slice(&17u32.to_le_bytes());
        descriptor[156 + 10..156 + 14].copy_from_slice(&(root.len() as u32).to_le_bytes());
        put(&mut image, 16, &descriptor);
        put(&mut image, 17, &root);
        put(&mut image, 18, cnf.as_bytes());
        put(&mut image, 19, executable);
        image
    }

    fn write(bytes: &[u8]) -> (tempfile::TempDir, String) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("game.iso");
        std::fs::File::create(&path).unwrap().write_all(bytes).unwrap();
        (directory, path.to_string_lossy().into_owned())
    }

    #[test]
    fn simple_failures() {
        assert_eq!(validate_iso("  ").reason, Some(IsoReason::Empty));
        assert_eq!(validate_iso("/no/such.iso").reason, Some(IsoReason::NotFound));
        let (_keep, path) = write(&[0u8; 100]);
        assert_eq!(validate_iso(&path).reason, Some(IsoReason::Format));
    }

    #[test]
    fn another_game_is_recognized_by_its_boot_file() {
        let (_keep, path) = write(&build_iso("SLUS_123.45", b"ELF"));
        assert_eq!(validate_iso(&path).reason, Some(IsoReason::OtherGame));
    }

    #[test]
    fn another_god_of_war_edition_is_named() {
        let (_keep, path) = write(&build_iso("SCES_531.33", b"ELF"));
        let result = validate_iso(&path);
        assert!(!result.ok);
        assert_eq!(result.reason, Some(IsoReason::OtherEdition));
        assert_eq!(result.edition, "Europe / Australia");
        assert_eq!(other_edition("slpm_670.10"), Some("Japan"));
        assert_eq!(other_edition("SCUS_973.99"), None, "the supported disc is not an other edition");
    }

    #[test]
    fn right_game_wrong_hash_reports_the_hash() {
        let (_keep, path) = write(&build_iso("SCUS_973.99", b"a patched executable"));
        let result = validate_iso(&path);
        assert!(!result.ok);
        assert_eq!(result.reason, Some(IsoReason::Modified));
        let mut md5 = Md5::new();
        md5.update(b"SCUS_973.99");
        md5.update(b"a patched executable");
        assert_eq!(result.hash, crate::game::hex(&md5.finalize()));
    }

    #[test]
    fn the_disc_inside_a_chd_is_validated_in_place() {
        let directory = tempfile::tempdir().unwrap();
        let chd = directory.path().join("game.chd");
        std::fs::write(
            &chd,
            crate::iso_chd::tests::uncompressed_chd(&build_iso("SCUS_973.99", b"a patched executable")),
        )
        .unwrap();
        let result = validate_iso(&chd.to_string_lossy());
        assert_eq!(result.reason, Some(IsoReason::Modified), "read through the CHD as far as the executable");
        std::fs::write(&chd, crate::iso_chd::tests::uncompressed_chd(&build_iso("SLUS_123.45", b"ELF"))).unwrap();
        assert_eq!(validate_iso(&chd.to_string_lossy()).reason, Some(IsoReason::OtherGame));
        std::fs::write(&chd, b"MComprHD but nothing more").unwrap();
        assert_eq!(validate_iso(&chd.to_string_lossy()).reason, Some(IsoReason::Chd));
    }

    #[test]
    fn boot_line_parsing() {
        assert_eq!(boot_executable("BOOT2 = cdrom0:\\SCUS_973.99;1\r\n").as_deref(), Some("SCUS_973.99"));
        assert_eq!(boot_executable("boot2=cdrom0:SCUS_973.99 \n").as_deref(), Some("SCUS_973.99"));
        assert_eq!(boot_executable("BOOT = cdrom0:X"), None);
        assert_eq!(boot_executable("BOOT2 = host:X"), None);
    }
}
