//! `verify-elf`: identity and bounds of the local ELF before it is recompiled.
//!
//! Port of `tools/verify_elf.py`. A function map that fits inside the segments does NOT prove that modified code is
//! compatible: `--allow-modified` lets an experiment through and keeps that warning.

use std::fmt::Write as _;
use std::path::Path;

use ps2_parity::sha256_hex;

/// The PCSX2 CRC of the USA retail executable.
pub const EXPECTED_CRC: u32 = 0xD638_5328;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub sha256: String,
    pub pcsx2_crc: u32,
    pub entry: u32,
    pub mapped_functions: usize,
}

impl Report {
    pub fn known_edition(&self) -> bool {
        self.pcsx2_crc == EXPECTED_CRC
    }

    /// The JSON the Python tool printed (same keys, same order, two-space indent).
    pub fn to_json(&self) -> String {
        let mut out = String::from("{\n");
        let _ = writeln!(out, "  \"sha256\": \"{}\",", self.sha256);
        let _ = writeln!(out, "  \"pcsx2_crc\": \"{:08X}\",", self.pcsx2_crc);
        let _ = writeln!(out, "  \"expected_crc\": \"{EXPECTED_CRC:08X}\",");
        let _ = writeln!(out, "  \"known_edition\": {},", self.known_edition());
        let _ = writeln!(out, "  \"entry\": \"{:08X}\",", self.entry);
        let _ = writeln!(out, "  \"mapped_functions\": {},", self.mapped_functions);
        out.push_str("  \"layout_valid\": true,\n");
        out.push_str("  \"compatibility_verified\": false\n}");
        out
    }
}

fn u16_at(data: &[u8], offset: usize) -> Result<u16, String> {
    data.get(offset..offset + 2).map(|b| u16::from_le_bytes([b[0], b[1]])).ok_or_else(|| "Truncated ELF".to_owned())
}

fn u32_at(data: &[u8], offset: usize) -> Result<u32, String> {
    data.get(offset..offset + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| "Truncated ELF".to_owned())
}

/// `int(text, 16)`: an optional `0x` prefix.
pub fn parse_hex(text: &str) -> Result<u32, String> {
    let digits = text.trim();
    let digits = digits.strip_prefix("0x").or_else(|| digits.strip_prefix("0X")).unwrap_or(digits);
    u32::from_str_radix(digits, 16).map_err(|_| format!("invalid hexadecimal number: {text:?}"))
}

/// The `(Name, Start, End)` rows of `funcmap.csv` (a header line names the columns).
fn read_function_map(path: &Path) -> Result<Vec<(String, u32, u32)>, String> {
    let text = std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next().ok_or("empty function map")?.split(',').map(str::trim).collect();
    let column =
        |name: &str| header.iter().position(|field| *field == name).ok_or_else(|| format!("missing column {name}"));
    let (name_at, start_at, end_at) = (column("Name")?, column("Start")?, column("End")?);

    let mut rows = Vec::new();
    for line in lines.filter(|line| !line.trim().is_empty()) {
        let fields: Vec<&str> = line.split(',').collect();
        let field = |index: usize| fields.get(index).copied().ok_or_else(|| format!("short row: {line}"));
        rows.push((field(name_at)?.to_owned(), parse_hex(field(start_at)?)?, parse_hex(field(end_at)?)?));
    }
    Ok(rows)
}

/// `inspect_elf`.
pub fn inspect(elf: &Path, function_map: &Path) -> Result<Report, String> {
    let data = std::fs::read(elf).map_err(|error| format!("{}: {error}", elf.display()))?;
    inspect_bytes(&data, &read_function_map(function_map)?)
}

pub fn inspect_bytes(data: &[u8], functions: &[(String, u32, u32)]) -> Result<Report, String> {
    if data.len() < 52 || data[..6] != *b"\x7fELF\x01\x01" {
        return Err("A little-endian ELF32 is required".to_owned());
    }
    let kind = u16_at(data, 16)?;
    let machine = u16_at(data, 18)?;
    let version = u32_at(data, 20)?;
    let entry = u32_at(data, 24)?;
    let phoff = u32_at(data, 28)? as usize;
    let phsize = usize::from(u16_at(data, 42)?);
    let phcount = usize::from(u16_at(data, 44)?);
    if (kind, machine, version, entry) != (2, 8, 1, 0x0010_0008) {
        return Err("Type, architecture or entry point does not match the USA map".to_owned());
    }
    if phsize < 32 || phoff + phsize * phcount > data.len() {
        return Err("Truncated segment table".to_owned());
    }

    let mut segments: Vec<(u32, u32)> = Vec::new();
    for index in 0..phcount {
        let base = phoff + index * phsize;
        let (kind, offset, vaddr, filesz, memsz, flags) = (
            u32_at(data, base)?,
            u32_at(data, base + 4)? as usize,
            u32_at(data, base + 8)?,
            u32_at(data, base + 16)?,
            u32_at(data, base + 20)?,
            u32_at(data, base + 24)?,
        );
        if kind != 1 {
            continue;
        }
        if offset + filesz as usize > data.len() || filesz > memsz {
            return Err("Truncated or inconsistent segment".to_owned());
        }
        if flags & 1 != 0 {
            segments.push((vaddr, vaddr.wrapping_add(filesz)));
        }
    }

    let invalid: Vec<&str> = functions
        .iter()
        .filter(|(_, start, end)| !segments.iter().any(|&(low, high)| low <= *start && start < end && *end <= high))
        .map(|(name, _, _)| name.as_str())
        .collect();
    if !invalid.is_empty() {
        let shown: Vec<String> = invalid.iter().take(5).map(|name| format!("'{name}'")).collect();
        return Err(format!("{} functions outside the ELF: [{}]", invalid.len(), shown.join(", ")));
    }

    let crc = data[..data.len() / 4 * 4]
        .chunks_exact(4)
        .fold(0u32, |crc, word| crc ^ u32::from_le_bytes([word[0], word[1], word[2], word[3]]));
    Ok(Report { sha256: sha256_hex(data), pcsx2_crc: crc, entry, mapped_functions: functions.len() })
}
