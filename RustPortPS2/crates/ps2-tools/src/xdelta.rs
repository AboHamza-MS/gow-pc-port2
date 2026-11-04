//! GOW-Port (T5.4): `xdelta-apply`, applies an xdelta3 patch (VCDIFF, RFC 3284) to a disc image with no external program.
//!
//! Fan translations ship as xdelta3 patches against the USA ISO. This reader decodes what xdelta3 writes by default: the
//! RFC 3284 format with the default code table, plus xdelta3's two extensions (the application header, which is skipped, and
//! the Adler-32 of each target window, which is checked). Secondary compression (`xdelta3 -S djw|fgk|lzma`) is refused with a
//! message that says how to rebuild the patch without it.
//!
//! The decoded image goes to `<output>.part` and replaces `<output>` only once every window has the expected size and
//! checksum, so a wrong base disc or a damaged patch never leaves a half-written ISO behind. The base is checked on the very
//! first window: a patch made for another ISO fails at once instead of after gigabytes.

use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Header indicator: secondary compressor id follows (xdelta3 `-S`).
const VCD_DECOMPRESS: u8 = 0x01;
/// Header indicator: an application-defined code table follows.
const VCD_CODETABLE: u8 = 0x02;
/// Header indicator (xdelta3): an application header follows.
const VCD_APPHEADER: u8 = 0x04;
/// Window indicator: the window copies from the source file.
const VCD_SOURCE: u8 = 0x01;
/// Window indicator: the window copies from the target already written.
const VCD_TARGET: u8 = 0x02;
/// Window indicator (xdelta3): the Adler-32 of the target window follows.
const VCD_ADLER32: u8 = 0x04;

/// Address cache sizes of the default code table (RFC 3284, 5.1).
const NEAR_SIZE: usize = 4;
const SAME_SIZE: usize = 3;

/// Limits that keep a corrupt length from allocating the whole memory. xdelta3 windows are at most 16 MiB of target
/// (`-W`) and the source segment of a window is bounded by its source buffer (`-B`, 64 MiB by default, a few GiB at most).
const MAX_TARGET_WINDOW: u64 = 256 << 20;
const MAX_SEGMENT: u64 = 4 << 30;

/// The error of a decode stopped because `progress` asked for it.
pub const CANCELLED: &str = "cancelled";

/// What `apply` produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    pub windows: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Noop,
    Add,
    Run,
    Copy(u8),
}

/// One half of a code table entry: the operation and its size, where 0 means "the size follows as a varint".
#[derive(Debug, Clone, Copy)]
struct Half {
    op: Op,
    size: u8,
}

/// The default code table of RFC 3284, section 5.6: 256 entries of one or two instructions.
fn default_code_table() -> [[Half; 2]; 256] {
    let noop = Half { op: Op::Noop, size: 0 };
    let mut table = [[noop; 2]; 256];
    let mut code = 0usize;
    let mut put = |first: Half, second: Half| {
        table[code] = [first, second];
        code += 1;
    };
    put(Half { op: Op::Run, size: 0 }, noop);
    for size in 0..=17 {
        put(Half { op: Op::Add, size }, noop);
    }
    for mode in 0..9 {
        put(Half { op: Op::Copy(mode), size: 0 }, noop);
        for size in 4..=18 {
            put(Half { op: Op::Copy(mode), size }, noop);
        }
    }
    for mode in 0..6 {
        for add in 1..=4 {
            for copy in 4..=6 {
                put(Half { op: Op::Add, size: add }, Half { op: Op::Copy(mode), size: copy });
            }
        }
    }
    for mode in 6..9 {
        for add in 1..=4 {
            put(Half { op: Op::Add, size: add }, Half { op: Op::Copy(mode), size: 4 });
        }
    }
    for mode in 0..9 {
        put(Half { op: Op::Copy(mode), size: 4 }, Half { op: Op::Add, size: 1 });
    }
    debug_assert_eq!(code, 256);
    table
}

/// Adler-32 (RFC 1950) of `data`.
pub fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65_521;
    let (mut a, mut b) = (1u32, 0u32);
    // 5552 bytes is the longest run that cannot overflow `b` before the reduction.
    for chunk in data.chunks(5552) {
        for &byte in chunk {
            a += u32::from(byte);
            b += a;
        }
        a %= MOD;
        b %= MOD;
    }
    (b << 16) | a
}

fn damaged(what: &str) -> String {
    format!("the patch is damaged or is not an xdelta3/VCDIFF file ({what})")
}

/// Reads one byte, `None` at a clean end of file.
fn read_byte(reader: &mut impl Read) -> Result<Option<u8>, String> {
    let mut byte = [0u8; 1];
    loop {
        match reader.read(&mut byte) {
            Ok(0) => return Ok(None),
            Ok(_) => return Ok(Some(byte[0])),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(format!("reading the patch: {error}")),
        }
    }
}

fn need_byte(reader: &mut impl Read, what: &str) -> Result<u8, String> {
    read_byte(reader)?.ok_or_else(|| damaged(&format!("it ends inside {what}")))
}

/// A VCDIFF integer: base-128 digits, most significant first, the high bit set on all but the last.
fn read_varint(reader: &mut impl Read, what: &str) -> Result<u64, String> {
    let mut value = 0u64;
    for _ in 0..10 {
        let byte = need_byte(reader, what)?;
        if value >> 57 != 0 {
            return Err(damaged(&format!("{what} overflows")));
        }
        value = (value << 7) | u64::from(byte & 0x7F);
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(damaged(&format!("{what} is too long")))
}

/// The same integer read from a section of the current window.
fn take_varint(section: &[u8], position: &mut usize, what: &str) -> Result<u64, String> {
    let mut value = 0u64;
    for _ in 0..10 {
        let byte = *section.get(*position).ok_or_else(|| damaged(&format!("the {what} section ends early")))?;
        *position += 1;
        if value >> 57 != 0 {
            return Err(damaged(&format!("a value in the {what} section overflows")));
        }
        value = (value << 7) | u64::from(byte & 0x7F);
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(damaged(&format!("a value in the {what} section is too long")))
}

fn read_section(reader: &mut impl Read, length: u64, what: &str) -> Result<Vec<u8>, String> {
    let mut section = Vec::new();
    reader.take(length).read_to_end(&mut section).map_err(|error| format!("reading the patch: {error}"))?;
    if section.len() as u64 != length {
        return Err(damaged(&format!("it ends inside the {what} section")));
    }
    Ok(section)
}

/// Reads the file header and returns the application header (xdelta3 stores the file names there).
fn read_header(patch: &mut impl Read) -> Result<Vec<u8>, String> {
    let mut magic = [0u8; 4];
    patch.read_exact(&mut magic).map_err(|_| damaged("it is shorter than its header"))?;
    if magic[..3] != [0xD6, 0xC3, 0xC4] {
        return Err("this is not an xdelta3/VCDIFF patch (wrong signature)".to_owned());
    }
    let indicator = need_byte(patch, "the header")?;
    if indicator & VCD_DECOMPRESS != 0 {
        return Err("the patch uses secondary compression, which is not supported: rebuild it with \
                    `xdelta3 -S none -e -s <original.iso> <translated.iso> <patch.xdelta>`"
            .to_owned());
    }
    if indicator & VCD_CODETABLE != 0 {
        return Err("the patch uses a custom code table, which is not supported: rebuild it with xdelta3".to_owned());
    }
    if indicator & VCD_APPHEADER != 0 {
        let length = read_varint(patch, "the application header length")?;
        if length > 1 << 20 {
            return Err(damaged("the application header is too large"));
        }
        return read_section(patch, length, "application header");
    }
    Ok(Vec::new())
}

/// The RFC 3284 address cache (section 5.1).
struct AddressCache {
    near: [u64; NEAR_SIZE],
    next: usize,
    same: [u64; SAME_SIZE * 256],
}

impl AddressCache {
    fn new() -> Self {
        Self { near: [0; NEAR_SIZE], next: 0, same: [0; SAME_SIZE * 256] }
    }

    fn decode(&mut self, here: u64, mode: u8, addresses: &[u8], position: &mut usize) -> Result<u64, String> {
        let mode = usize::from(mode);
        let address = if mode == 0 {
            take_varint(addresses, position, "address")?
        } else if mode == 1 {
            let back = take_varint(addresses, position, "address")?;
            here.checked_sub(back).ok_or_else(|| damaged("a copy points before the window"))?
        } else if mode < 2 + NEAR_SIZE {
            self.near[mode - 2].wrapping_add(take_varint(addresses, position, "address")?)
        } else {
            let byte = *addresses.get(*position).ok_or_else(|| damaged("the address section ends early"))?;
            *position += 1;
            self.same[(mode - 2 - NEAR_SIZE) * 256 + usize::from(byte)]
        };
        self.near[self.next] = address;
        self.next = (self.next + 1) % NEAR_SIZE;
        self.same[(address % (SAME_SIZE as u64 * 256)) as usize] = address;
        Ok(address)
    }
}

/// Decodes the instructions of one window into `target`.
fn decode_window(
    table: &[[Half; 2]; 256],
    segment: &[u8],
    data: &[u8],
    instructions: &[u8],
    addresses: &[u8],
    target: &mut Vec<u8>,
) -> Result<(), String> {
    let source_length = segment.len() as u64;
    let mut cache = AddressCache::new();
    let (mut data_at, mut instruction_at, mut address_at) = (0usize, 0usize, 0usize);
    let capacity = target.capacity();
    while instruction_at < instructions.len() {
        let code = instructions[instruction_at];
        instruction_at += 1;
        for half in table[usize::from(code)] {
            if half.op == Op::Noop {
                continue;
            }
            let size = match half.size {
                0 => take_varint(instructions, &mut instruction_at, "instruction")?,
                size => u64::from(size),
            };
            let size = usize::try_from(size).map_err(|_| damaged("an instruction is too large"))?;
            if target.len() + size > capacity {
                return Err(damaged("a window produces more bytes than it declares"));
            }
            match half.op {
                Op::Noop => {}
                Op::Add => {
                    let bytes =
                        data.get(data_at..data_at + size).ok_or_else(|| damaged("the data section ends early"))?;
                    target.extend_from_slice(bytes);
                    data_at += size;
                }
                Op::Run => {
                    let byte = *data.get(data_at).ok_or_else(|| damaged("the data section ends early"))?;
                    data_at += 1;
                    target.resize(target.len() + size, byte);
                }
                Op::Copy(mode) => {
                    let here = source_length + target.len() as u64;
                    let address = cache.decode(here, mode, addresses, &mut address_at)?;
                    if address >= here {
                        return Err(damaged("a copy points past the bytes produced so far"));
                    }
                    copy(segment, target, address, size);
                }
            }
        }
    }
    Ok(())
}

/// A COPY: from the source segment, from the window's own output, or both. A copy from the output may overlap the bytes it
/// is producing (that is how VCDIFF encodes repetitions), so that part goes byte by byte.
fn copy(segment: &[u8], target: &mut Vec<u8>, address: u64, size: usize) {
    let source_length = segment.len() as u64;
    let mut address = address;
    let mut left = size;
    if address < source_length {
        let start = address as usize;
        let count = left.min(segment.len() - start);
        target.extend_from_slice(&segment[start..start + count]);
        address += count as u64;
        left -= count;
    }
    if left == 0 {
        return;
    }
    let start = (address - source_length) as usize;
    if start + left <= target.len() {
        target.extend_from_within(start..start + left);
    } else {
        for offset in 0..left {
            let byte = target[start + offset];
            target.push(byte);
        }
    }
}

/// Reads `length` bytes at `position` of a seekable file, with the reason it failed in the message.
fn read_segment(file: &mut (impl Read + Seek), position: u64, length: u64, which: &str) -> Result<Vec<u8>, String> {
    let end = file.seek(SeekFrom::End(0)).map_err(|error| format!("reading the {which}: {error}"))?;
    if position.checked_add(length).is_none_or(|last| last > end) {
        return Err(match which {
            "base disc" => "the patch reads past the end of the base disc: it was made for a different ISO".to_owned(),
            _ => damaged("a window copies from output that was not written yet"),
        });
    }
    file.seek(SeekFrom::Start(position)).map_err(|error| format!("reading the {which}: {error}"))?;
    let mut segment = vec![0u8; length as usize];
    file.read_exact(&mut segment).map_err(|error| format!("reading the {which}: {error}"))?;
    Ok(segment)
}

/// Decodes `patch` against `source` into `output`, which must be empty, readable and writable (a target window copies from
/// the output already written). `progress` receives the bytes written so far after each window and returns `false` to stop
/// (the decode then fails with [`CANCELLED`]).
pub fn decode(
    patch: &mut impl Read,
    source: &mut (impl Read + Seek),
    output: &mut (impl Read + Write + Seek),
    mut progress: impl FnMut(u64) -> bool,
) -> Result<Summary, String> {
    read_header(patch)?;
    let table = default_code_table();
    let mut summary = Summary { windows: 0, bytes: 0 };
    while let Some(indicator) = read_byte(patch)? {
        let window = summary.windows + 1;
        if indicator & !(VCD_SOURCE | VCD_TARGET | VCD_ADLER32) != 0 || indicator & VCD_SOURCE & (indicator >> 1) != 0 {
            return Err(damaged(&format!("window {window} has an unknown indicator {indicator:#04x}")));
        }
        let segment_place = if indicator & (VCD_SOURCE | VCD_TARGET) != 0 {
            let length = read_varint(patch, "a source segment length")?;
            let position = read_varint(patch, "a source segment position")?;
            if length > MAX_SEGMENT {
                return Err(damaged(&format!("window {window} reads a {length}-byte segment")));
            }
            Some((length, position))
        } else {
            None
        };
        let _encoding_length = read_varint(patch, "a window length")?;
        let target_length = read_varint(patch, "a target window length")?;
        if target_length > MAX_TARGET_WINDOW {
            return Err(damaged(&format!("window {window} declares {target_length} bytes")));
        }
        if need_byte(patch, "a window")? != 0 {
            return Err("the patch compresses its sections (xdelta3 secondary compression), which is not supported: \
                        rebuild it with `xdelta3 -S none`"
                .to_owned());
        }
        let data_length = read_varint(patch, "a data section length")?;
        let instructions_length = read_varint(patch, "an instruction section length")?;
        let addresses_length = read_varint(patch, "an address section length")?;
        let checksum = if indicator & VCD_ADLER32 != 0 {
            let mut bytes = [0u8; 4];
            patch.read_exact(&mut bytes).map_err(|_| damaged("it ends inside a window checksum"))?;
            Some(u32::from_be_bytes(bytes))
        } else {
            None
        };
        // Every section byte produces at least one target byte, apart from varint sizes and addresses: bound them by the
        // target length plus room for those, so a corrupt length is caught before it is read into memory.
        let bound = target_length.saturating_mul(2) + 1024;
        if data_length > bound || instructions_length > bound || addresses_length > bound {
            return Err(damaged(&format!("window {window} has oversized sections")));
        }
        let data = read_section(patch, data_length, "data")?;
        let instructions = read_section(patch, instructions_length, "instruction")?;
        let addresses = read_section(patch, addresses_length, "address")?;

        let segment = match segment_place {
            Some((length, position)) if indicator & VCD_SOURCE != 0 => {
                read_segment(source, position, length, "base disc")?
            }
            Some((length, position)) => {
                let segment = read_segment(output, position, length, "output")?;
                output.seek(SeekFrom::End(0)).map_err(|error| format!("writing the output: {error}"))?;
                segment
            }
            None => Vec::new(),
        };

        let mut target = Vec::with_capacity(target_length as usize);
        let decoded = decode_window(&table, &segment, &data, &instructions, &addresses, &mut target);
        let wrong_base = || {
            if window == 1 {
                "the patch does not match this disc (first window): it was made for a different ISO. Use the original \
                 USA disc (SCUS-97399) it was made from"
                    .to_owned()
            } else {
                format!("window {window} does not decode: the patch is damaged or was made for a different ISO")
            }
        };
        if let Err(error) = decoded {
            return Err(if window == 1 { format!("{}; {error}", wrong_base()) } else { error });
        }
        if target.len() as u64 != target_length {
            return Err(damaged(&format!("window {window} produced {} of {target_length} bytes", target.len())));
        }
        if checksum.is_some_and(|expected| adler32(&target) != expected) {
            return Err(wrong_base());
        }
        output.write_all(&target).map_err(|error| format!("writing the output: {error}"))?;
        summary.windows = window;
        summary.bytes += target_length;
        if !progress(summary.bytes) {
            return Err(CANCELLED.to_owned());
        }
    }
    if summary.windows == 0 {
        return Err(damaged("it has no windows"));
    }
    Ok(summary)
}

fn part_path(output: &Path) -> PathBuf {
    let mut name = output.file_name().map(|name| name.to_os_string()).unwrap_or_default();
    name.push(".part");
    output.with_file_name(name)
}

/// Applies the patch file to the base disc and writes the result to `output`, published only when every window checks out.
pub fn apply(patch: &Path, source: &Path, output: &Path, mut progress: impl FnMut(u64)) -> Result<Summary, String> {
    apply_cancellable(patch, source, output, |bytes| {
        progress(bytes);
        true
    })
}

/// Checks that `patch` is a VCDIFF file this reader can apply (signature, no secondary compression, the default code
/// table), reading only its header: for a quick answer when a patch is chosen, before gigabytes are decoded.
pub fn check_header(patch: &Path) -> Result<(), String> {
    let mut file = BufReader::new(File::open(patch).map_err(|error| format!("{}: {error}", patch.display()))?);
    read_header(&mut file).map(|_| ())
}

/// [`apply`] with a `progress` that returns `false` to stop; a stopped decode leaves nothing behind and fails with
/// [`CANCELLED`].
pub fn apply_cancellable(
    patch: &Path,
    source: &Path,
    output: &Path,
    progress: impl FnMut(u64) -> bool,
) -> Result<Summary, String> {
    if output == source || output == patch {
        return Err("the output must be a new file, not the base disc or the patch".to_owned());
    }
    let mut patch_file =
        BufReader::with_capacity(1 << 20, File::open(patch).map_err(|error| format!("{}: {error}", patch.display()))?);
    let mut source_file = File::open(source).map_err(|error| format!("{}: {error}", source.display()))?;
    let part = part_path(output);
    let mut part_file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&part)
        .map_err(|error| format!("{}: {error}", part.display()))?;
    let result = decode(&mut patch_file, &mut source_file, &mut part_file, progress).and_then(|summary| {
        part_file.sync_all().map(|()| summary).map_err(|error| format!("writing the output: {error}"))
    });
    drop(part_file);
    match result {
        Ok(summary) => {
            // Windows refuses to rename over an existing file.
            if output.exists() {
                std::fs::remove_file(output).map_err(|error| format!("{}: {error}", output.display()))?;
            }
            std::fs::rename(&part, output).map_err(|error| format!("{}: {error}", output.display()))?;
            Ok(summary)
        }
        Err(error) => {
            let _ = std::fs::remove_file(&part);
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_code_table_matches_the_rfc() {
        let table = default_code_table();
        // Spot checks from RFC 3284, 5.6.
        assert_eq!(table[0][0].op, Op::Run);
        assert_eq!((table[1][0].op, table[1][0].size), (Op::Add, 0));
        assert_eq!((table[18][0].op, table[18][0].size), (Op::Add, 17));
        assert_eq!((table[19][0].op, table[19][0].size), (Op::Copy(0), 0));
        assert_eq!((table[34][0].op, table[34][0].size), (Op::Copy(0), 18));
        assert_eq!((table[162][0].op, table[162][0].size), (Op::Copy(8), 18));
        assert_eq!((table[163][0].size, table[163][1].op, table[163][1].size), (1, Op::Copy(0), 4));
        assert_eq!((table[234][0].size, table[234][1].op, table[234][1].size), (4, Op::Copy(5), 6));
        assert_eq!((table[235][1].op, table[235][1].size), (Op::Copy(6), 4));
        assert_eq!((table[247][0].op, table[247][1].op, table[247][1].size), (Op::Copy(0), Op::Add, 1));
        assert_eq!((table[255][0].op, table[255][1].op), (Op::Copy(8), Op::Add));
    }

    #[test]
    fn adler32_matches_zlib() {
        assert_eq!(adler32(b""), 1);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
        assert_eq!(adler32(&vec![0xFF; 100_000]), 0x149A_302C);
    }

    #[test]
    fn a_copy_from_the_output_may_overlap_itself() {
        let mut target = b"ab".to_vec();
        copy(b"", &mut target, 0, 6);
        assert_eq!(target, b"abababab");
        let mut target = b"Z".to_vec();
        copy(b"xyz", &mut target, 1, 4);
        assert_eq!(target, b"ZyzZy", "source then output");
    }
}
