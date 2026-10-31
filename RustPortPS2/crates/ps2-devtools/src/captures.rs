//! Inspectors of local captures: `cancel-line` (movement controller timeline), `gif-inspect` (GIF packets) and
//! `vif-extract` (the input of `replay_vif_chain`). None of them needs game files to be tested.

use std::collections::HashSet;
use std::path::Path;

use ps2_parity::sha256_hex;
use serde_json::{json, Map, Value};

use crate::util::{option, parse_int_auto};

// --- cancel-line ------------------------------------------------------------------------------------------------

pub fn cancel_line(args: &[String]) -> i32 {
    let Some(path) = args.first() else {
        eprintln!("usage: ps2-dev cancel-line SEGMENT.bin [T0 T1]");
        return 2;
    };
    let data = match std::fs::read(path) {
        Ok(data) => data,
        Err(error) => {
            eprintln!("{path}: {error}");
            return 1;
        }
    };
    let (t0, t1) =
        match (args.get(1).and_then(|v| v.parse::<f64>().ok()), args.get(2).and_then(|v| v.parse::<f64>().ok())) {
            (Some(a), Some(b)) => (a, b),
            _ => (0.0, 1e9),
        };
    const RECORD: usize = 16 + 0x200;
    let offsets = [0x44usize, 0x54, 0x58, 0x60, 0x70, 0x78, 0x1ac];
    let header: Vec<String> = offsets.iter().map(|o| format!("{o:>9x}")).collect();
    println!("t      bot  {}", header.join(" "));
    let mut previous: Option<Vec<u32>> = None;
    for i in 0..data.len() / RECORD {
        let at = i * RECORD;
        let t = f64::from_le_bytes(data[at..at + 8].try_into().expect("8 bytes"));
        let bot = u16::from_le_bytes(data[at + 8..at + 10].try_into().expect("2 bytes"));
        if !(t0 <= t && t <= t1) {
            continue;
        }
        let row: Vec<u32> = offsets
            .iter()
            .map(|o| u32::from_le_bytes(data[at + 16 + o..at + 20 + o].try_into().expect("4 bytes")))
            .collect();
        let b = !bot;
        let mut s = String::new();
        if b & 0x8000 != 0 {
            s.push_str("SQ");
        }
        if b & 0x400 != 0 {
            s.push_str("L1");
        }
        if b & 0x1000 != 0 {
            s.push_str("TR");
        }
        if previous.as_ref() != Some(&row) || !s.is_empty() {
            let values: Vec<String> = row.iter().map(|v| format!("{v:9x}")).collect();
            println!("{t:.2} {s:<4} {}", values.join(" "));
            previous = Some(row);
        }
    }
    0
}

// --- gif-inspect ------------------------------------------------------------------------------------------------

fn bounds(vertices: &[(u64, u64, u64, bool)], pick: impl Fn(&(u64, u64, u64, bool)) -> u64) -> Value {
    let min = vertices.iter().map(&pick).min().unwrap_or(0);
    let max = vertices.iter().map(&pick).max().unwrap_or(0);
    json!([min, max])
}

/// Validates the bounds and counts vertices; a kick does not guarantee a primitive.
pub fn inspect(data: &[u8]) -> Result<Value, String> {
    let mut position = 0usize;
    let mut vertices: Vec<(u64, u64, u64, bool)> = Vec::new();
    let mut tags: Vec<Value> = Vec::new();
    let mut prim_types: Vec<(u64, u64)> = Vec::new();

    let require = |position: usize, size: usize| -> Result<(), String> {
        let left = data.len() - position;
        if size > left {
            Err(format!("incomplete payload at 0x{position:x}: {} bytes missing", size - left))
        } else {
            Ok(())
        }
    };
    let read = |at: usize| u64::from_le_bytes(data[at..at + 8].try_into().expect("8 bytes"));
    let xyz = |vertices: &mut Vec<(u64, u64, u64, bool)>, register: u64, low: u64, high: u64, packed: bool| {
        if !matches!(register, 4 | 5 | 12 | 13) {
            return;
        }
        let x = low & 0xffff;
        let y = (low >> if packed { 32 } else { 16 }) & 0xffff;
        let z = if packed {
            if matches!(register, 4 | 12) {
                (high >> 4) & 0xff_ffff
            } else {
                high & 0xffff_ffff
            }
        } else {
            (low >> 32) & if matches!(register, 4 | 12) { 0xff_ffff } else { 0xffff_ffff }
        };
        let no_kick = matches!(register, 12 | 13) || (packed && (high >> 47) & 1 != 0);
        vertices.push((x, y, z, no_kick));
    };

    while position < data.len() {
        require(position, 16)?;
        let (low, high) = (read(position), read(position + 8));
        position += 16;
        let nloop = low & 0x7fff;
        let format = (low >> 58) & 3;
        let nreg = match (low >> 60) & 15 {
            0 => 16,
            n => n,
        };
        let registers: Vec<u64> = (0..nreg).map(|i| (high >> (4 * i)) & 15).collect();
        tags.push(json!({"nloop": nloop, "format": format, "nreg": nreg}));
        if format == 0 && (low >> 46) & 1 != 0 {
            let prim = (low >> 47) & 7;
            match prim_types.iter_mut().find(|(k, _)| *k == prim) {
                Some((_, count)) => *count += 1,
                None => prim_types.push((prim, 1)),
            }
        }
        let count = (nloop * nreg) as usize;
        match format {
            0 => {
                require(position, count * 16)?;
                for i in 0..count {
                    let (v0, v1) = (read(position), read(position + 8));
                    position += 16;
                    let register = registers[i % nreg as usize];
                    xyz(&mut vertices, register, v0, v1, true);
                    if register == 14 {
                        // A+D uses the layout of a 64-bit GS register.
                        xyz(&mut vertices, v1 & 255, v0, 0, false);
                    }
                }
            }
            1 => {
                require(position, count.div_ceil(2) * 16)?;
                for i in 0..count {
                    let v0 = read(position);
                    position += 8;
                    xyz(&mut vertices, registers[i % nreg as usize], v0, 0, false);
                }
                if count & 1 != 0 {
                    position += 8;
                }
            }
            2 => {
                require(position, nloop as usize * 16)?;
                position += nloop as usize * 16;
            }
            _ => return Err(format!("GIF format 3 not supported at 0x{:x}", position - 16)),
        }
    }

    let mut prims = Map::new();
    for (prim, count) in &prim_types {
        prims.insert(prim.to_string(), json!(count));
    }
    let mut result = Map::new();
    result.insert("bytes".into(), json!(data.len()));
    result.insert("sha256".into(), json!(sha256_hex(data)));
    result.insert("tags".into(), Value::Array(tags));
    result.insert("prim_types_with_pre".into(), Value::Object(prims));
    result.insert("vertices".into(), json!(vertices.len()));
    result.insert("kick_vertices".into(), json!(vertices.iter().filter(|v| !v.3).count()));
    result.insert("no_kick_vertices".into(), json!(vertices.iter().filter(|v| v.3).count()));
    let distinct: HashSet<(u64, u64, u64)> = vertices.iter().map(|v| (v.0, v.1, v.2)).collect();
    result.insert("distinct_xyz".into(), json!(distinct.len()));
    if !vertices.is_empty() {
        result.insert(
            "xyz_bounds".into(),
            json!([bounds(&vertices, |v| v.0), bounds(&vertices, |v| v.1), bounds(&vertices, |v| v.2)]),
        );
    }
    Ok(Value::Object(result))
}

pub fn gif_inspect(args: &[String]) -> i32 {
    let Some(capture) = args.first().filter(|a| !a.starts_with("--")) else {
        eprintln!("usage: ps2-dev gif-inspect CAPTURE [--json FILE]  (a GIF file or a folder with gow_geo_gif_*.bin)");
        return 2;
    };
    let path = Path::new(capture);
    let mut files: Vec<std::path::PathBuf> = if path.is_file() {
        vec![path.to_path_buf()]
    } else {
        let entries =
            std::fs::read_dir(path).map(|d| d.flatten().map(|e| e.path()).collect::<Vec<_>>()).unwrap_or_default();
        entries
            .into_iter()
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("gow_geo_gif_") && n.ends_with(".bin"))
            })
            .collect()
    };
    let number = |p: &std::path::PathBuf| -> u64 {
        let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        stem.rsplit('_').next().and_then(|n| n.parse().ok()).unwrap_or(0)
    };
    if !path.is_file() {
        files.sort_by_key(number);
    }
    if files.is_empty() {
        eprintln!("ps2-dev gif-inspect: error: no GIF captures found");
        return 2;
    }
    let mut rows = Vec::new();
    for file in &files {
        let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("").to_owned();
        let data = match std::fs::read(file) {
            Ok(data) => data,
            Err(error) => {
                eprintln!("{name}: {error}");
                return 1;
            }
        };
        match inspect(&data) {
            Ok(Value::Object(inspection)) => {
                let mut row = Map::new();
                row.insert("file".into(), json!(name));
                row.extend(inspection);
                rows.push(Value::Object(row));
            }
            Ok(_) => unreachable!("inspect returns an object"),
            Err(error) => {
                eprintln!("ps2-dev gif-inspect: error: {name}: {error}");
                return 2;
            }
        }
    }
    let sum = |key: &str| rows.iter().map(|r| r[key].as_u64().unwrap_or(0)).sum::<u64>();
    let mut report = Map::new();
    report.insert("packets".into(), json!(rows.len()));
    report.insert("vertices".into(), json!(sum("vertices")));
    report.insert("kick_vertices".into(), json!(sum("kick_vertices")));
    report.insert("no_kick_vertices".into(), json!(sum("no_kick_vertices")));
    report.insert("packets_without_vertices".into(), json!(rows.iter().filter(|r| r["vertices"] == 0).count()));
    let summary = Value::Object(report.clone());
    report.insert("detail".into(), Value::Array(rows));
    if let Some(output) = option(args, "--json") {
        let text = serde_json::to_string_pretty(&Value::Object(report)).expect("json") + "\n";
        if let Err(error) = std::fs::write(&output, text) {
            eprintln!("{output}: {error}");
            return 1;
        }
    }
    println!("{}", serde_json::to_string_pretty(&summary).expect("json"));
    0
}

// --- vif-extract ------------------------------------------------------------------------------------------------

/// God of War frame headers (DIRECT with the GS configuration and NEXT to the list): the game alternates between two
/// buffers.
pub const FRAME_STARTS: [i64; 2] = [0x450C00, 0x450A00];
pub const RAM: i64 = 0x200_0000;

fn tag_at(ee: &[u8], a: i64) -> (i64, i64, i64) {
    let a = a as usize;
    let t = u64::from_le_bytes(ee[a..a + 8].try_into().expect("8 bytes"));
    ((t & 0xFFFF) as i64, ((t >> 28) & 7) as i64, ((t >> 32) & 0x7FFF_FFFF) as i64)
}

/// Walks a DMA chain from `start`: (END address, number of tags); the address is `None` for an invalid chain.
pub fn walk(ee: &[u8], start: i64, end: Option<i64>, limit: usize) -> (Option<i64>, usize) {
    let bound = RAM.min(ee.len() as i64);
    let (mut a, mut stack, mut n) = (start, Vec::<i64>::new(), 0usize);
    while n < limit {
        if a < 0 || a + 16 > bound || a & 15 != 0 {
            return (None, n);
        }
        let (qwc, tid, addr) = tag_at(ee, a);
        n += 1;
        if !(1..=7).contains(&tid) {
            return (None, n);
        }
        let data = if matches!(tid, 1 | 2 | 5 | 6 | 7) { a + 16 } else { addr & (RAM - 1) };
        if data + qwc * 16 > bound {
            return (None, n);
        }
        match tid {
            1 => a += 16 + qwc * 16,
            2 => a = addr,
            3 | 4 => a += 16,
            5 => {
                if stack.len() >= 2 {
                    return (None, n);
                }
                stack.push(a + 16 + qwc * 16);
                a = addr;
            }
            6 => match stack.pop() {
                Some(back) => a = back,
                None => return (None, n),
            },
            _ => return if end.is_none_or(|e| a == e) { (Some(a), n) } else { (None, n) },
        }
    }
    (None, n)
}

/// A single valid header; an explicit choice does not identify the current frame.
pub fn select_chain(
    ee: &[u8],
    start: Option<i64>,
    end: Option<i64>,
    frame_starts: &[i64],
) -> Result<(i64, i64, usize), String> {
    // GOW-Port: both buffers may keep valid chains; their order does not identify the frame.
    let mut candidates: Vec<i64> = Vec::new();
    match start {
        Some(start) => candidates.push(start),
        None => {
            for &candidate in frame_starts {
                if !candidates.contains(&candidate) {
                    candidates.push(candidate);
                }
            }
        }
    }
    let valid: Vec<(i64, i64, usize)> = candidates
        .iter()
        .filter_map(|&candidate| {
            let (final_address, tags) = walk(ee, candidate, end, 500_000);
            final_address.map(|f| (candidate, f, tags))
        })
        .collect();
    let target = end.map_or(String::new(), |e| format!(" 0x{e:X}"));
    if valid.is_empty() {
        return Err(match start {
            Some(start) => format!("The chain chosen with --start 0x{start:X} does not end in END{target}"),
            None => format!("No frame header ends in END{target}"),
        });
    }
    if valid.len() > 1 {
        let detail: Vec<String> = valid.iter().map(|(c, f, _)| format!("0x{c:X} -> END 0x{f:X}")).collect();
        return Err(format!(
            "Ambiguous selection: {}. Pass --start; choosing a header does not prove that it is the current frame.",
            detail.join(", ")
        ));
    }
    Ok(valid[0])
}

/// The stream VIF1 receives with CHCR.TTE: each tag's upper 64 bits and its payload.
pub fn flatten(ee: &[u8], start: i64) -> Result<Vec<u8>, String> {
    let (final_address, tags) = walk(ee, start, None, 500_000);
    if final_address.is_none() {
        return Err("Invalid DMA chain or missing END".to_owned());
    }
    let (mut out, mut a, mut stack) = (Vec::new(), start, Vec::<i64>::new());
    for _ in 0..tags {
        let (qwc, tid, addr) = tag_at(ee, a);
        out.extend_from_slice(&ee[a as usize + 8..a as usize + 16]);
        let data = if matches!(tid, 1 | 2 | 5 | 6 | 7) { a + 16 } else { addr & (RAM - 1) };
        match tid {
            1 => a += 16 + qwc * 16,
            2 => a = addr,
            3 | 4 => a += 16,
            5 => {
                stack.push(a + 16 + qwc * 16);
                a = addr;
            }
            6 => a = stack.pop().unwrap_or(-1),
            _ => a = -1,
        }
        out.extend_from_slice(&ee[data as usize..(data + qwc * 16) as usize]);
    }
    Ok(out)
}

pub fn vif_extract(args: &[String]) -> i32 {
    let (Some(dump), Some(output)) = (option(args, "--dump"), args.iter().rev().find(|a| !a.starts_with("--"))) else {
        eprintln!("usage: ps2-dev vif-extract --dump FOLDER [--start 0xADDR] OUTPUT");
        eprintln!("  FOLDER has gow_render_ram.bin, gow_render_vram.bin, gow_vu1_code.bin and gow_vu1_data.bin (GOW_RENDER_DIAG).");
        eprintln!("  The output contains game data: keep it under logs/ and never publish it.");
        return 2;
    };
    let output = Path::new(output);
    if output == Path::new(&dump) {
        eprintln!("the output folder must differ from the dump folder");
        return 2;
    }
    let read = |name: &str| std::fs::read(Path::new(&dump).join(name)).map_err(|e| format!("{name}: {e}"));
    let loaded = (|| -> Result<_, String> {
        Ok((
            read("gow_render_ram.bin")?,
            read("gow_render_vram.bin")?,
            read("gow_vu1_code.bin")?,
            read("gow_vu1_data.bin")?,
        ))
    })();
    let (ee, vram, code, data) = match loaded {
        Ok(files) => files,
        Err(message) => {
            eprintln!("{message}");
            return 1;
        }
    };
    if vram.len() != 0x40_0000 || ee.len() as i64 != RAM {
        eprintln!("Unexpected RAM or VRAM sizes");
        return 1;
    }
    let start = option(args, "--start").and_then(|v| parse_int_auto(&v));
    let (start, final_address, tags) = match select_chain(&ee, start, None, &FRAME_STARTS) {
        Ok(chain) => chain,
        Err(message) => {
            eprintln!("{message}");
            return 1;
        }
    };
    let stream = match flatten(&ee, start) {
        Ok(stream) => stream,
        Err(message) => {
            eprintln!("{message}");
            return 1;
        }
    };
    let write = |name: &str, bytes: &[u8]| std::fs::write(output.join(name), bytes);
    let written = std::fs::create_dir_all(output)
        .and_then(|()| write("vu1MicroMem.bin", &code))
        .and_then(|()| write("vu1Memory.bin", &data))
        .and_then(|()| write("vram.bin", &vram))
        .and_then(|()| write("eeMemory.bin", &ee))
        .and_then(|()| write("vif1_stream.bin", &stream));
    if let Err(error) = written {
        eprintln!("{}: {error}", output.display());
        return 1;
    }
    println!("chain 0x{start:X} -> END 0x{final_address:X}: {tags} tags, {} bytes for VIF1", stream.len());
    0
}
