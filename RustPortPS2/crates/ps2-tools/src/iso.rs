//! `iso-extract`: an ISO9660 lister/extractor for PS2 discs.
//!
//! Port of `tools/iso_extract.py`. Handles dual-layer DVDs: layer 1 has its own volume descriptor right after layer 0,
//! with extents relative to (that sector - 16). `--layout` also writes `.disc_layout` (a text manifest) and
//! `.disc_meta` (every non-zero sector that belongs to no file), so the runtime serves the extracted files at their
//! original sector numbers.

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub const SECTOR: u64 = 2048;

/// One file of the disc: its path, first sector and size in bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscFile {
    pub name: String,
    pub lba: u64,
    pub size: u64,
}

fn u32_at(data: &[u8], offset: usize) -> u64 {
    data.get(offset..offset + 4).map_or(0, |b| u64::from(u32::from_le_bytes([b[0], b[1], b[2], b[3]])))
}

/// Read up to `length` bytes at `offset` (a short read at the end of the image returns what exists).
fn read_at<R: Read + Seek>(reader: &mut R, offset: u64, length: usize) -> std::io::Result<Vec<u8>> {
    reader.seek(SeekFrom::Start(offset))?;
    let mut buffer = Vec::with_capacity(length);
    reader.by_ref().take(length as u64).read_to_end(&mut buffer)?;
    Ok(buffer)
}

fn read_directory<R: Read + Seek>(
    reader: &mut R,
    lba: u64,
    size: u64,
    path: &str,
    out: &mut Vec<DiscFile>,
    base: u64,
) -> std::io::Result<()> {
    let data = read_at(reader, (base + lba) * SECTOR, size as usize)?;
    let mut offset = 0usize;
    while offset < data.len() {
        let record_length = usize::from(data[offset]);
        if record_length == 0 {
            // Records never span sectors.
            offset = (offset / SECTOR as usize + 1) * SECTOR as usize;
            continue;
        }
        let Some(record) = data.get(offset..offset + record_length) else {
            break;
        };
        let extent_lba = u32_at(record, 2);
        let extent_length = u32_at(record, 10);
        let flags = record.get(25).copied().unwrap_or(0);
        let name_length = usize::from(record.get(32).copied().unwrap_or(0));
        let name_bytes = record.get(33..33 + name_length).unwrap_or(&[]);
        offset += record_length;
        if name_bytes == [0] || name_bytes == [1] {
            continue;
        }
        let name: String = name_bytes.iter().map(|&b| if b.is_ascii() { b as char } else { '\u{fffd}' }).collect();
        let name = name.split(';').next().unwrap_or("").to_owned();
        let full = if path.is_empty() { name } else { format!("{path}/{name}") };
        if flags & 2 != 0 {
            read_directory(reader, extent_lba, extent_length, &full, out, base)?;
        } else {
            out.push(DiscFile { name: full, lba: base + extent_lba, size: extent_length });
        }
    }
    Ok(())
}

/// Every file on the disc and the first sector of layer 1 (0 on a single-layer disc).
pub fn scan<R: Read + Seek>(reader: &mut R) -> std::io::Result<(Vec<DiscFile>, u64)> {
    let pvd = read_at(reader, 16 * SECTOR, SECTOR as usize)?;
    if pvd.get(1..6) != Some(b"CD001".as_slice()) {
        return Err(std::io::Error::other("not ISO9660"));
    }
    let mut files = Vec::new();
    let root = &pvd[156..156 + 34];
    read_directory(reader, u32_at(root, 2), u32_at(root, 10), "", &mut files, 0)?;

    let layer0 = u32_at(&pvd, 80);
    let mut layer1 = 0;
    let pvd1 = read_at(reader, layer0 * SECTOR, SECTOR as usize)?;
    if pvd1.get(1..6) == Some(b"CD001".as_slice()) {
        layer1 = layer0.saturating_sub(16);
        let root1 = &pvd1[156..156 + 34];
        read_directory(reader, u32_at(root1, 2), u32_at(root1, 10), "", &mut files, layer1)?;
    }
    Ok((files, layer1))
}

/// `.disc_meta` (sector number + 2048 bytes for each non-zero sector outside the files) and `.disc_layout`.
pub fn write_layout(reader: &mut File, files: &[DiscFile], layer1: u64, output: &Path) -> std::io::Result<()> {
    let total = reader.metadata()?.len() / SECTOR;
    let mut covered: Vec<(u64, u64)> = files.iter().map(|f| (f.lba, f.lba + f.size.div_ceil(SECTOR))).collect();
    covered.sort_unstable();
    covered.push((total, total));

    let mut meta: Vec<(u32, Vec<u8>)> = Vec::new();
    let mut next = 0u64;
    for (start, end) in covered {
        for sector in next..start {
            let data = read_at(reader, sector * SECTOR, SECTOR as usize)?;
            if data.iter().any(|&b| b != 0) {
                meta.push((sector as u32, data));
            }
        }
        next = next.max(end);
    }

    let mut meta_file = std::io::BufWriter::new(File::create(output.join(".disc_meta"))?);
    for (sector, data) in &meta {
        meta_file.write_all(&sector.to_le_bytes())?;
        meta_file.write_all(data)?;
    }
    meta_file.flush()?;

    let mut layout = format!("sectors {total}\nlayer1 {layer1}\n");
    let mut sorted: Vec<&DiscFile> = files.iter().collect();
    sorted.sort_by_key(|f| f.lba);
    for file in sorted {
        layout.push_str(&format!("file {} {} {}\n", file.lba, file.size, file.name));
    }
    fs::write(output.join(".disc_layout"), layout)?;
    println!("layout: {} files, {} metadata sectors, layer1={layer1}", files.len(), meta.len());
    Ok(())
}

/// Options of `iso-extract`.
#[derive(Debug, Default)]
pub struct Options {
    pub iso: PathBuf,
    pub output: Option<PathBuf>,
    pub list: bool,
    pub only: Option<Vec<String>>,
    pub layout: bool,
}

/// Lists and/or extracts the disc.
pub fn run(options: &Options) -> std::io::Result<()> {
    let mut iso = File::open(&options.iso)?;
    let (files, layer1) = scan(&mut iso)?;
    for file in &files {
        if options.list {
            println!("{:8} {:12} {}", file.lba, file.size, file.name);
        }
        let Some(output) = &options.output else {
            continue;
        };
        if options.only.as_ref().is_some_and(|only| !only.contains(&file.name.to_uppercase())) {
            continue;
        }
        let mut destination = output.clone();
        for part in file.name.split('/') {
            destination.push(part);
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        iso.seek(SeekFrom::Start(file.lba * SECTOR))?;
        let mut target = std::io::BufWriter::with_capacity(1 << 20, File::create(&destination)?);
        std::io::copy(&mut Read::by_ref(&mut iso).take(file.size), &mut target)?;
        target.flush()?;
    }
    if let (Some(output), true) = (&options.output, options.layout) {
        write_layout(&mut iso, &files, layer1, output)?;
    }
    Ok(())
}
