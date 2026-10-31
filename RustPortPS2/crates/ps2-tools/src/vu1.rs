//! VU1 microcode images built from the user's own ELF.
//!
//! Port of `tools/vu1/vu1_manifest.py` (`apply`, `verify`) and `tools/vu1/vu1_programs.py`. God of War's VU1 microcode
//! is inside `SCUS_973.99`, uncompressed. The manifest holds no code, only "this piece of the ELF (offset, length,
//! fingerprint) goes at this micro-memory address"; with it the installer creates the micro-memories that used to be
//! captured by playing, and hands them to the generators. `extend` is a developer-only capture analysis command: it
//! writes more of that metadata when a capture can be rebuilt entirely from the same user's ELF. It never copies
//! captured microcode into the manifest.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::hash::{blake2b_64_hex, fnv};

/// VU1 micro-memory size in bytes.
pub const SIZE: usize = 16384;
const VERSION: u32 = 1;
/// Less than two instruction pairs is not usable code.
const MIN_PIECE: usize = 16;

type Piece = (usize, usize, String);

fn elf_identity(elf: &[u8]) -> String {
    format!("{} {}", elf.len(), blake2b_64_hex(elf))
}

struct Manifest {
    table: Vec<Piece>,
    images: Vec<Vec<(usize, usize)>>,
    /// `Some(true)` when the `E` line matches this ELF, `None` when there is none.
    identity_ok: Option<bool>,
}

fn read_manifest(path: &Path, elf: &[u8]) -> Result<Manifest, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut manifest = Manifest { table: Vec::new(), images: Vec::new(), identity_ok: None };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        let hex = |s: &str| usize::from_str_radix(s, 16).map_err(|_| format!("bad number in manifest: {s}"));
        match fields[0] {
            "V" => {
                if fields.get(1).and_then(|v| v.parse::<u32>().ok()) != Some(VERSION) {
                    return Err(format!("[manifest] unsupported version: {}", fields.get(1).unwrap_or(&"")));
                }
            }
            "E" => manifest.identity_ok = Some(fields[1..3.min(fields.len())].join(" ") == elf_identity(elf)),
            "R" => {
                let length = fields.get(2).and_then(|n| n.parse().ok()).ok_or("bad R line")?;
                manifest.table.push((
                    hex(fields.get(1).ok_or("bad R line")?)?,
                    length,
                    fields.get(3).ok_or("bad R line")?.to_string(),
                ));
            }
            "I" => {
                let mut refs = Vec::new();
                for part in &fields[1..] {
                    let (address, id) = part.split_once(':').ok_or("bad I line")?;
                    refs.push((hex(address)?, id.parse().map_err(|_| "bad I line".to_owned())?));
                }
                manifest.images.push(refs);
            }
            _ => {}
        }
    }
    Ok(manifest)
}

fn rebuild(refs: &[(usize, usize)], table: &[Piece], elf: &[u8]) -> Vec<u8> {
    let mut memory = vec![0u8; SIZE];
    for &(address, id) in refs {
        let (offset, length, _) = &table[id];
        memory[address..address + length].copy_from_slice(&elf[*offset..offset + length]);
    }
    memory
}

fn valid_pieces(elf: &[u8], manifest: &Manifest) -> Vec<bool> {
    manifest
        .table
        .iter()
        .map(|(offset, length, fingerprint)| {
            offset + length <= elf.len() && blake2b_64_hex(&elf[*offset..offset + length]) == *fingerprint
        })
        .collect()
}

fn image_is_well_formed(refs: &[(usize, usize)], manifest: &Manifest, valid: &[bool]) -> bool {
    refs.iter().all(|&(address, id)| id < manifest.table.len() && valid[id] && address + manifest.table[id].1 <= SIZE)
}

/// `(name, image)` for each image whose pieces match their fingerprint; the number of images discarded.
fn assemble(elf: &[u8], manifest: &Manifest) -> (Vec<(String, Vec<u8>)>, usize) {
    let valid = valid_pieces(elf, manifest);
    let mut images = Vec::new();
    let mut invalid = 0;
    for refs in &manifest.images {
        if !image_is_well_formed(refs, manifest, &valid) {
            invalid += 1;
            continue;
        }
        let image = rebuild(refs, &manifest.table, elf);
        images.push((format!("{:016x}", fnv(&image)), image));
    }
    (images, invalid)
}

/// `vu1-manifest apply ELF MANIFEST OUTPUT`.
pub fn apply_manifest(elf_path: &Path, manifest_path: &Path, output: &Path) -> Result<i32, String> {
    let elf = fs::read(elf_path).map_err(|e| format!("{}: {e}", elf_path.display()))?;
    let manifest = read_manifest(manifest_path, &elf)?;
    if manifest.identity_ok == Some(false) {
        eprintln!("[manifest] this ELF is not the one the manifest describes; only the matching pieces are used");
    }
    fs::create_dir_all(output).map_err(|e| e.to_string())?;
    let valid = valid_pieces(&elf, &manifest);
    let invalid = manifest.images.iter().filter(|refs| !image_is_well_formed(refs, &manifest, &valid)).count();
    if invalid > 0 {
        eprintln!("[manifest] {invalid} images discarded: their pieces do not match this ELF");
    }
    let (mut written, mut existing) = (0, 0);
    // GOW-Port: install the images as they are rebuilt. A full retail manifest has hundreds of thousands of
    // 16 KiB images; retaining them all until this loop turns a disk-only install step into a multi-gigabyte RAM peak.
    for refs in &manifest.images {
        if !image_is_well_formed(refs, &manifest, &valid) {
            continue;
        }
        let image = rebuild(refs, &manifest.table, &elf);
        let name = format!("{:016x}", fnv(&image));
        let target = output.join(format!("vu1_{name}.bin"));
        if target.exists() {
            existing += 1;
            continue;
        }
        fs::write(&target, &image).map_err(|e| format!("{}: {e}", target.display()))?;
        written += 1;
    }
    println!(
        "images written: {written}; already existed: {existing}; described in the manifest: {}",
        manifest.images.len()
    );
    Ok(if written + existing > 0 { 0 } else { 1 })
}

/// The `*.bin` files of exactly one micro-memory in `folder`, sorted by name.
fn load_captures(folder: &Path) -> Result<Vec<(PathBuf, Vec<u8>)>, String> {
    let mut paths: Vec<PathBuf> = fs::read_dir(folder)
        .map_err(|e| format!("{}: {e}", folder.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "bin"))
        .collect();
    paths.sort();
    let mut captures = Vec::new();
    for path in paths {
        let data = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        if data.len() == SIZE {
            captures.push((path, data));
        }
    }
    Ok(captures)
}

/// `vu1-manifest verify ELF MANIFEST CAPTURES`.
pub fn verify_manifest(elf_path: &Path, manifest_path: &Path, folder: &Path) -> Result<i32, String> {
    let elf = fs::read(elf_path).map_err(|e| format!("{}: {e}", elf_path.display()))?;
    let manifest = read_manifest(manifest_path, &elf)?;
    let (images, invalid) = assemble(&elf, &manifest);
    if invalid > 0 {
        eprintln!("[manifest] {invalid} images discarded: their pieces do not match this ELF");
    }
    let assembled: BTreeMap<String, Vec<u8>> = images.into_iter().collect();
    let (mut total, mut equal) = (0usize, 0usize);
    for (_, image) in load_captures(folder)? {
        total += 1;
        if assembled.get(&format!("{:016x}", fnv(&image))) == Some(&image) {
            equal += 1;
        }
    }
    let identity = match manifest.identity_ok {
        Some(true) => "matches",
        Some(false) => "different",
        None => "no data",
    };
    println!(
        "ELF identity: {identity}; captures: {total}; rebuilt identical: {equal} ({:.1} %); extra in the manifest: {}",
        100.0 * equal as f64 / total.max(1) as f64,
        assembled.len() - equal
    );
    Ok(if equal == total { 0 } else { 1 })
}

fn word(bytes: &[u8]) -> u64 {
    u64::from_le_bytes(bytes.try_into().expect("VU words are always eight bytes"))
}

fn nonzero_word(bytes: &[u8]) -> bool {
    bytes.iter().any(|&byte| byte != 0)
}

/// Collapses identical ELF pieces and remaps every image reference to the first occurrence.
///
/// The manifest format identifies a piece by its row number, so historic manifests can contain the same `(offset,
/// length, fingerprint)` under several IDs. Keeping one makes the recipe smaller without changing any reconstructed
/// image.
fn compact_manifest(manifest: &Manifest) -> (Vec<Piece>, Vec<Vec<(usize, usize)>>) {
    let mut ids: BTreeMap<Piece, usize> = BTreeMap::new();
    let mut remap = Vec::with_capacity(manifest.table.len());
    let mut table = Vec::new();
    for piece in &manifest.table {
        let id = match ids.get(piece) {
            Some(&id) => id,
            None => {
                let id = table.len();
                table.push(piece.clone());
                ids.insert(piece.clone(), id);
                id
            }
        };
        remap.push(id);
    }
    let images =
        manifest.images.iter().map(|refs| refs.iter().map(|&(address, id)| (address, remap[id])).collect()).collect();
    (table, images)
}

/// `vu1-manifest extend ELF MANIFEST CAPTURES OUT`.
///
/// Captures are used only as local evidence. Each added `R` line points back into `ELF` and each `I` line maps those
/// verified pieces into VU memory; no capture bytes are written to `OUT`.
pub fn extend_manifest(
    elf_path: &Path,
    manifest_path: &Path,
    captures_path: &Path,
    output: &Path,
) -> Result<i32, String> {
    let elf = fs::read(elf_path).map_err(|e| format!("{}: {e}", elf_path.display()))?;
    let manifest = read_manifest(manifest_path, &elf)?;
    if manifest.identity_ok == Some(false) {
        return Err("[manifest] refusing to extend: this ELF does not match the manifest identity".to_owned());
    }
    let (assembled, invalid) = assemble(&elf, &manifest);
    if invalid > 0 {
        return Err(format!("[manifest] refusing to extend: {invalid} existing images are invalid"));
    }
    let known: HashSet<String> = assembled.into_iter().map(|(name, _)| name).collect();
    let original_piece_count = manifest.table.len();
    let (mut table, mut images) = compact_manifest(&manifest);
    let mut missing: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for (_, image) in load_captures(captures_path)? {
        let name = format!("{:016x}", fnv(&image));
        if !known.contains(&name) {
            missing.entry(name).or_insert(image);
        }
    }

    let mut wanted = HashSet::new();
    for image in missing.values() {
        for pair in image.chunks_exact(8) {
            if nonzero_word(pair) {
                wanted.insert(word(pair));
            }
        }
    }
    let mut locations: HashMap<u64, Vec<usize>> = HashMap::new();
    if elf.len() >= 8 {
        for offset in (0..=elf.len() - 8).step_by(8) {
            let value = word(&elf[offset..offset + 8]);
            if wanted.contains(&value) {
                locations.entry(value).or_default().push(offset);
            }
        }
    }

    // IDs refer to the append order below, not sorted ELF offsets.
    let mut piece_ids: HashMap<(usize, usize), usize> =
        table.iter().enumerate().map(|(id, (offset, length, _))| ((*offset, *length), id)).collect();
    let compacted = original_piece_count - table.len();
    let mut rebuilt = 0usize;
    for image in missing.values() {
        let mut refs = Vec::new();
        let mut address = 0usize;
        let mut complete = true;
        while address < SIZE {
            let pair = &image[address..address + 8];
            if !nonzero_word(pair) {
                address += 8;
                continue;
            }
            let Some(candidates) = locations.get(&word(pair)) else {
                complete = false;
                break;
            };
            let mut best: Option<(usize, usize)> = None;
            for &offset in candidates {
                let mut length = 8usize;
                while address + length < SIZE
                    && offset + length < elf.len()
                    && image[address + length..address + length + 8] == elf[offset + length..offset + length + 8]
                {
                    length += 8;
                }
                if best.is_none_or(|(_, longest)| length > longest) {
                    best = Some((offset, length));
                }
            }
            let (offset, length) = best.expect("a candidate exists");
            refs.push((address, offset, length));
            address += length;
        }
        if !complete {
            continue;
        }
        let mut image_refs = Vec::new();
        for (address, offset, length) in refs {
            let piece = (offset, length);
            let id = match piece_ids.get(&piece) {
                Some(&id) => id,
                None => {
                    let id = table.len();
                    table.push((offset, length, blake2b_64_hex(&elf[offset..offset + length])));
                    piece_ids.insert(piece, id);
                    id
                }
            };
            image_refs.push((address, id));
        }
        images.push(image_refs);
        rebuilt += 1;
    }

    let mut rendered = format!("V {VERSION}\nE {}\n", elf_identity(&elf));
    for (offset, length, fingerprint) in &table {
        rendered.push_str(&format!("R {offset:x} {length} {fingerprint}\n"));
    }
    for refs in &images {
        rendered.push('I');
        for (address, id) in refs {
            rendered.push_str(&format!(" {address:x}:{id}"));
        }
        rendered.push('\n');
    }
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    fs::write(output, rendered).map_err(|e| format!("{}: {e}", output.display()))?;
    println!(
        "captures not already covered: {}; rebuilt from ELF: {}; duplicate pieces removed: {}; manifest pieces: {}",
        missing.len(),
        rebuilt,
        compacted,
        table.len()
    );
    Ok(if rebuilt > 0 || compacted > 0 { 0 } else { 1 })
}

// ---- vu1-programs -----------------------------------------------------------------------------------------------------

/// `(index, micro address, file offset, size)` of the programs the ELF declares in `.DVP.ovlytab`.
pub fn declared_programs(elf: &[u8]) -> Vec<(usize, usize, usize, usize)> {
    fn u16_at(data: &[u8], offset: usize) -> Option<usize> {
        data.get(offset..offset + 2).map(|b| usize::from(u16::from_le_bytes([b[0], b[1]])))
    }
    fn u32_at(data: &[u8], offset: usize) -> Option<usize> {
        data.get(offset..offset + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
    }

    let parse = || -> Option<Vec<(usize, usize, usize, usize)>> {
        if elf.get(..4)? != b"\x7fELF" {
            return Some(Vec::new());
        }
        let shoff = u32_at(elf, 32)?;
        let (shentsize, shnum, shstrndx) = (u16_at(elf, 46)?, u16_at(elf, 48)?, u16_at(elf, 50)?);
        let phoff = u32_at(elf, 28)?;
        let (phentsize, phnum) = (u16_at(elf, 42)?, u16_at(elf, 44)?);

        let section = |index: usize| -> Option<(usize, usize, usize)> {
            let base = shoff + index * shentsize;
            Some((u32_at(elf, base)?, u32_at(elf, base + 16)?, u32_at(elf, base + 20)?))
        };
        let names_offset = section(shstrndx)?.1;

        let mut table = None;
        let mut sizes = Vec::new();
        for index in 0..shnum {
            let (name, offset, size) = section(index)?;
            let start = names_offset + name;
            let end = start + elf.get(start..)?.iter().position(|&b| b == 0)?;
            let label: String = elf[start..end].iter().map(|&b| b as char).collect();
            if label == ".DVP.ovlytab" {
                table = Some((offset, size));
            } else if label.starts_with(".DVP.overlay") {
                sizes.push(size);
            }
        }
        let table = table?;

        let mut segments = Vec::new();
        for index in 0..phnum {
            let base = phoff + index * phentsize;
            if u32_at(elf, base)? == 1 {
                segments.push((u32_at(elf, base + 8)?, u32_at(elf, base + 4)?, u32_at(elf, base + 16)?));
            }
        }

        let mut programs = Vec::new();
        for index in 0..table.1 / 12 {
            let ee = u32_at(elf, table.0 + index * 12 + 4)?;
            let vu = u32_at(elf, table.0 + index * 12 + 8)?;
            let size = sizes.get(index).copied().unwrap_or(0);
            if let Some(&(vaddr, offset, _)) =
                segments.iter().find(|&&(vaddr, _, filesz)| vaddr <= ee && ee < vaddr + filesz)
            {
                programs.push((index, vu, offset + ee - vaddr, size));
            }
        }
        Some(programs)
    };
    parse().unwrap_or_default()
}

/// The `R` pieces of the manifest as merged `[start, end)` byte ranges of the ELF.
fn covered_pieces(manifest_path: &Path) -> Result<Vec<(usize, usize)>, String> {
    let text = fs::read_to_string(manifest_path).map_err(|e| format!("{}: {e}", manifest_path.display()))?;
    let mut covered: Vec<(usize, usize)> = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.first() == Some(&"R") && fields.len() >= 3 {
            let start = usize::from_str_radix(fields[1], 16).map_err(|_| "bad R line".to_owned())?;
            let length: usize = fields[2].parse().map_err(|_| "bad R line".to_owned())?;
            covered.push((start, start + length));
        }
    }
    covered.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (a, b) in covered {
        match merged.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    Ok(merged)
}

/// The intervals of `[start, end)` that fall in no covered piece.
fn remainders(start: usize, end: usize, covered: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut position = start;
    for &(a, b) in covered {
        if b <= position || a >= end {
            continue;
        }
        if a > position {
            out.push((position, a));
        }
        position = position.max(b);
    }
    if position < end {
        out.push((position, end));
    }
    out
}

/// `(program index, 16 KB image)` with the declared code the manifest does not cover, each piece at its micro address.
pub fn static_images(elf: &[u8], manifest_path: &Path) -> Result<Vec<(usize, Vec<u8>)>, String> {
    let covered = covered_pieces(manifest_path)?;
    let mut result = Vec::new();
    for (index, vu, file_offset, size) in declared_programs(elf) {
        let mut image = vec![0u8; SIZE];
        let mut found = false;
        for (a, b) in remainders(file_offset, file_offset + size, &covered) {
            // Align to 8-byte pairs within the program.
            let a8 = file_offset + (a - file_offset).div_ceil(8) * 8;
            let b8 = file_offset + (b - file_offset) / 8 * 8;
            if b8 < a8 + MIN_PIECE || elf.get(a8..b8).is_none_or(|piece| piece.iter().all(|&byte| byte == 0)) {
                continue;
            }
            let target = vu + (a8 - file_offset);
            if target + (b8 - a8) > SIZE {
                continue;
            }
            image[target..target + (b8 - a8)].copy_from_slice(&elf[a8..b8]);
            found = true;
        }
        if found {
            result.push((index, image));
        }
    }
    Ok(result)
}

/// `vu1-programs apply ELF MANIFEST OUTPUT`.
pub fn apply_programs(elf_path: &Path, manifest_path: &Path, output: &Path) -> Result<i32, String> {
    let elf = fs::read(elf_path).map_err(|e| format!("{}: {e}", elf_path.display()))?;
    let images = static_images(&elf, manifest_path)?;
    fs::create_dir_all(output).map_err(|e| e.to_string())?;
    let mut written = 0;
    for (_, image) in &images {
        let target = output.join(format!("vu1s_{:016x}.bin", fnv(image)));
        if !target.exists() {
            fs::write(&target, image).map_err(|e| format!("{}: {e}", target.display()))?;
            written += 1;
        }
    }
    let declared = declared_programs(&elf).len();
    println!("static images: {} (written {written}) of {declared} declared programs", images.len());
    Ok(if !images.is_empty() || declared == 0 { 0 } else { 1 })
}

/// `vu1-programs report ELF MANIFEST`.
pub fn report_programs(elf_path: &Path, manifest_path: &Path) -> Result<i32, String> {
    let elf = fs::read(elf_path).map_err(|e| format!("{}: {e}", elf_path.display()))?;
    let covered = covered_pieces(manifest_path)?;
    let (mut total, mut unseen) = (0usize, 0usize);
    for (index, vu, file_offset, size) in declared_programs(&elf) {
        let rest: usize = remainders(file_offset, file_offset + size, &covered).iter().map(|(a, b)| b - a).sum();
        total += size;
        unseen += rest;
        println!("prog{index:02} vu=0x{vu:05X} elf=0x{file_offset:X} {size:5} B  unseen {rest:5} B");
    }
    println!(
        "declared {total} B, unseen in any capture {unseen} B ({:.1} %)",
        100.0 * unseen as f64 / total.max(1) as f64
    );
    Ok(0)
}
