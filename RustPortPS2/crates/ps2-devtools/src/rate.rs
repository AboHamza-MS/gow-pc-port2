//! `rate-readers`: a static scan of the generated C++ (`native/ps2xRuntime/src/runner/*.cpp`) for the code that depends
//! on the game's logic rate (high logic FPS research, `logs/plan/PLANO-FPS-ALTO-0.3.1.pt-BR.md`).
//!
//! The scan reads the disassembly comments the recompiler writes before each instruction
//! (`// 0x17d84c: 0xaca4c654  sw  $a0, -0x39AC($a1)`) and follows the constants built with `lui`/`addiu`/`ori`
//! through the registers, so it resolves `off($base)` addresses and the float constants moved with `mtc1`. It is a
//! linear, per-function heuristic: register values do not merge at branch targets, and a constant loaded from
//! `.rodata` with `lwc1` is not seen.
//!
//! It reports who reads and writes the rate globals (frames per second, floor in fields), who reads the time step
//! (delta), and the functions that use a rate constant (60, 1/60, 30, 50, 59.94...). A function that reads the delta
//! and also holds such a constant, or holds one without reading the delta, is the place to look at first.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::util::{first_positional, option, parse_hex};

/// 0x29C640 (frames per second, 60 or 30), 0x29C650 (floor in seconds), 0x29C654 (floor in fields).
pub const DEFAULT_RATE: [u32; 3] = [0x0029_C640, 0x0029_C650, 0x0029_C654];
/// 0x29C64C (delta, s) and 0x29C658 (scaled delta, s).
pub const DEFAULT_DELTA: [u32; 2] = [0x0029_C64C, 0x0029_C658];

/// One memory access with a resolved address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Access {
    pub pc: u32,
    pub address: u32,
    pub size: u32,
    pub write: bool,
    pub mnemonic: String,
}

/// A float constant moved into an FPU register (`mtc1`) that matches one of the rate constants.
#[derive(Debug, Clone, PartialEq)]
pub struct RateConstant {
    pub pc: u32,
    pub bits: u32,
    pub name: &'static str,
}

/// What the scan found in one generated function.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FunctionScan {
    pub name: String,
    pub address: u32,
    pub accesses: Vec<Access>,
    pub constants: Vec<RateConstant>,
}

/// Rate constants, matched with a relative tolerance so the rounding of the immediate does not matter.
const CONSTANTS: [(f32, &str); 8] = [
    (60.0, "60.0"),
    (1.0 / 60.0, "1/60"),
    (59.94, "59.94"),
    (1.0 / 59.94, "1/59.94"),
    (30.0, "30.0"),
    (1.0 / 30.0, "1/30"),
    (50.0, "50.0"),
    (1.0 / 50.0, "1/50"),
];

pub fn rate_constant(bits: u32) -> Option<&'static str> {
    let value = f32::from_bits(bits);
    if !value.is_finite() || value == 0.0 {
        return None;
    }
    CONSTANTS.iter().find(|(constant, _)| ((value - constant) / constant).abs() < 1e-4).map(|(_, name)| *name)
}

/// A disassembly comment: `// 0xPC: 0xWORD  mnemonic  operands [(Delay Slot)]`.
struct Instruction<'a> {
    pc: u32,
    mnemonic: &'a str,
    operands: Vec<&'a str>,
}

fn parse_instruction(line: &str) -> Option<Instruction<'_>> {
    let rest = line.trim_start().strip_prefix("// 0x")?;
    let (pc, rest) = rest.split_once(": 0x")?;
    let pc = u32::from_str_radix(pc, 16).ok()?;
    let (_word, rest) = rest.split_once(char::is_whitespace)?;
    let rest = rest.trim();
    let rest = rest.strip_suffix("(Delay Slot)").unwrap_or(rest).trim();
    let (mnemonic, operands) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    let operands = operands.split(',').map(str::trim).filter(|o| !o.is_empty()).collect();
    Some(Instruction { pc, mnemonic, operands })
}

/// An immediate as the disassembly prints it: `0x3C`, `-0x39AC`, or decimal.
fn immediate(text: &str) -> Option<i64> {
    if text.contains("0x") {
        parse_hex(text)
    } else {
        text.parse().ok()
    }
}

/// `off($base)` -> (offset, base register).
fn memory_operand(text: &str) -> Option<(i64, &str)> {
    let (offset, base) = text.strip_suffix(')')?.split_once('(')?;
    Some((if offset.is_empty() { 0 } else { immediate(offset)? }, base))
}

fn access_of(mnemonic: &str) -> Option<(u32, bool)> {
    Some(match mnemonic {
        "lb" | "lbu" => (1, false),
        "lh" | "lhu" => (2, false),
        "lw" | "lwu" | "lwc1" | "lwl" | "lwr" => (4, false),
        "ld" | "ldc1" | "ldl" | "ldr" => (8, false),
        "lq" => (16, false),
        "sb" => (1, true),
        "sh" => (2, true),
        "sw" | "swc1" | "swl" | "swr" => (4, true),
        "sd" | "sdc1" | "sdl" | "sdr" => (8, true),
        "sq" => (16, true),
        _ => return None,
    })
}

/// Mnemonics whose first operand is not a GPR they write.
fn writes_no_gpr(mnemonic: &str) -> bool {
    access_of(mnemonic).is_some_and(|(_, write)| write)
        || mnemonic.starts_with('b')
        || mnemonic.starts_with("c.")
        || matches!(
            mnemonic,
            "j" | "jr"
                | "jal"
                | "jalr"
                | "mtc1"
                | "ctc1"
                | "mthi"
                | "mtlo"
                | "mthi1"
                | "mtlo1"
                | "div"
                | "divu"
                | "div1"
                | "divu1"
                | "mult"
                | "multu"
                | "sync"
                | "syscall"
                | "break"
                | "nop"
                | "cache"
                | "pref"
                | "ei"
                | "di"
                | "eret"
        )
}

const CALLER_SAVED: [&str; 17] = [
    "$at", "$v0", "$v1", "$a0", "$a1", "$a2", "$a3", "$t0", "$t1", "$t2", "$t3", "$t4", "$t5", "$t6", "$t7", "$t8",
    "$t9",
];

#[derive(Default)]
struct Registers<'a> {
    known: BTreeMap<&'a str, u32>,
}

impl<'a> Registers<'a> {
    fn get(&self, register: &str) -> Option<u32> {
        if register == "$zero" {
            Some(0)
        } else {
            self.known.get(register).copied()
        }
    }

    fn set(&mut self, register: &'a str, value: Option<u32>) {
        if register == "$zero" {
            return;
        }
        match value {
            Some(value) => {
                self.known.insert(register, value);
            }
            None => {
                self.known.remove(register);
            }
        }
    }
}

fn is_gpr(operand: &str) -> bool {
    operand.starts_with('$') && !operand.starts_with("$f")
}

/// Applies one instruction to the register model; records its access or rate constant in `scan`.
fn step<'a>(registers: &mut Registers<'a>, instruction: &Instruction<'a>, scan: &mut FunctionScan) {
    let ops = &instruction.operands;
    let reg = |i: usize| ops.get(i).copied().unwrap_or("");
    if let Some((size, write)) = access_of(instruction.mnemonic) {
        if let Some((offset, base)) = memory_operand(reg(1)) {
            if let Some(base) = registers.get(base) {
                scan.accesses.push(Access {
                    pc: instruction.pc,
                    address: base.wrapping_add(offset as u32),
                    size,
                    write,
                    mnemonic: instruction.mnemonic.to_owned(),
                });
            }
        }
        if !write && is_gpr(reg(0)) {
            registers.set(reg(0), None);
        }
        return;
    }
    let value = match instruction.mnemonic {
        "lui" => immediate(reg(1)).map(|imm| ((imm as u32) & 0xFFFF) << 16),
        "addiu" | "addi" | "daddiu" | "daddi" => match (registers.get(reg(1)), immediate(reg(2))) {
            (Some(base), Some(imm)) => Some(base.wrapping_add(imm as u32)),
            _ => None,
        },
        "ori" => match (registers.get(reg(1)), immediate(reg(2))) {
            (Some(base), Some(imm)) => Some(base | ((imm as u32) & 0xFFFF)),
            _ => None,
        },
        "addu" | "daddu" | "or" if reg(2) == "$zero" => registers.get(reg(1)),
        "addu" | "daddu" | "or" if reg(1) == "$zero" => registers.get(reg(2)),
        "mtc1" => {
            if let Some(bits) = registers.get(reg(0)) {
                if let Some(name) = rate_constant(bits) {
                    scan.constants.push(RateConstant { pc: instruction.pc, bits, name });
                }
            }
            return;
        }
        mnemonic if writes_no_gpr(mnemonic) => return,
        _ => None,
    };
    if is_gpr(reg(0)) {
        registers.set(reg(0), value);
    }
}

/// Scans the generated text of one or more functions (a `runner/*.cpp` file).
pub fn scan_source(text: &str) -> Vec<FunctionScan> {
    let mut scans: Vec<FunctionScan> = Vec::new();
    let mut registers = Registers::default();
    let mut clobber_after_next = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if let Some(name) = trimmed.strip_prefix("// Function: ") {
            scans.push(FunctionScan { name: name.trim().to_owned(), ..FunctionScan::default() });
            registers = Registers::default();
            clobber_after_next = false;
            continue;
        }
        if let Some(range) = trimmed.strip_prefix("// Address: ") {
            if let Some(scan) = scans.last_mut() {
                let start = range.split_whitespace().next().unwrap_or("");
                scan.address = parse_hex(start).map_or(0, |v| v as u32);
            }
            continue;
        }
        let Some(instruction) = parse_instruction(line) else {
            continue;
        };
        let Some(scan) = scans.last_mut() else {
            continue;
        };
        step(&mut registers, &instruction, scan);
        // a call clobbers the caller-saved registers after its delay slot
        if clobber_after_next {
            for register in CALLER_SAVED {
                registers.set(register, None);
            }
            clobber_after_next = false;
        }
        if matches!(instruction.mnemonic, "jal" | "jalr") {
            clobber_after_next = true;
        }
    }
    scans
}

fn touches(access: &Access, address: u32) -> bool {
    address >= access.address && address - access.address < access.size
}

/// Who reads and writes `watched` addresses, and the rate constants, as a text report.
pub fn report(scans: &[FunctionScan], rate: &[u32], delta: &[u32], watch: &[u32]) -> String {
    let mut out = String::new();
    let functions_touching = |address: u32, write: bool| -> Vec<(String, Vec<u32>)> {
        scans
            .iter()
            .filter_map(|scan| {
                let pcs: Vec<u32> =
                    scan.accesses.iter().filter(|a| a.write == write && touches(a, address)).map(|a| a.pc).collect();
                (!pcs.is_empty()).then(|| (scan.name.clone(), pcs))
            })
            .collect()
    };
    let list = |out: &mut String, title: &str, addresses: &[u32]| {
        for &address in addresses {
            for (write, verb) in [(false, "read"), (true, "written")] {
                let found = functions_touching(address, write);
                out.push_str(&format!("{title} 0x{address:08X} {verb} by {} function(s)\n", found.len()));
                for (name, pcs) in found {
                    let pcs: Vec<String> = pcs.iter().map(|pc| format!("0x{pc:X}")).collect();
                    out.push_str(&format!("  {name} at {}\n", pcs.join(" ")));
                }
            }
        }
    };
    list(&mut out, "rate", rate);
    list(&mut out, "delta", delta);
    list(&mut out, "watch", watch);

    let reads_any = |scan: &FunctionScan, addresses: &[u32]| {
        scan.accesses.iter().any(|a| !a.write && addresses.iter().any(|&address| touches(a, address)))
    };
    let names = |scan: &FunctionScan| -> String {
        let set: BTreeSet<&str> = scan.constants.iter().map(|c| c.name).collect();
        set.into_iter().collect::<Vec<_>>().join(" ")
    };
    let with_constants: Vec<&FunctionScan> = scans.iter().filter(|s| !s.constants.is_empty()).collect();
    let suspects: Vec<&&FunctionScan> = with_constants.iter().filter(|s| reads_any(s, delta)).collect();
    out.push_str(&format!("delta readers with a rate constant: {}\n", suspects.len()));
    for scan in suspects {
        out.push_str(&format!("  {} [{}]\n", scan.name, names(scan)));
    }
    let others: Vec<&&FunctionScan> = with_constants.iter().filter(|s| !reads_any(s, delta)).collect();
    out.push_str(&format!("rate constants without a delta read (per-frame candidates): {}\n", others.len()));
    for scan in others {
        out.push_str(&format!("  {} [{}]\n", scan.name, names(scan)));
    }
    out
}

fn address_list(text: Option<String>, default: &[u32]) -> Result<Vec<u32>, String> {
    match text {
        None => Ok(default.to_vec()),
        Some(text) if text.is_empty() => Ok(Vec::new()),
        Some(text) => text
            .split(',')
            .map(|item| parse_hex(item.trim()).map(|v| v as u32).ok_or_else(|| format!("bad address: {item}")))
            .collect(),
    }
}

/// Scans every `*.cpp` of `folder` (sorted, so the report is stable).
pub fn scan_folder(folder: &Path) -> Result<Vec<FunctionScan>, String> {
    let mut files: Vec<_> = std::fs::read_dir(folder)
        .map_err(|e| format!("{}: {e}", folder.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "cpp"))
        .collect();
    files.sort();
    let mut scans = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        scans.extend(scan_source(&text));
    }
    scans.sort_by_key(|scan| scan.address);
    Ok(scans)
}

pub fn run(args: &[String]) -> i32 {
    let Some(folder) = first_positional(args, &["--rate", "--delta", "--watch"]) else {
        eprintln!("usage: ps2-dev rate-readers RUNNER_DIR [--rate A,B] [--delta A,B] [--watch A,B]");
        return 2;
    };
    let lists = (|| {
        Ok::<_, String>((
            address_list(option(args, "--rate"), &DEFAULT_RATE)?,
            address_list(option(args, "--delta"), &DEFAULT_DELTA)?,
            address_list(option(args, "--watch"), &[])?,
        ))
    })();
    let (rate, delta, watch) = match lists {
        Ok(lists) => lists,
        Err(message) => {
            eprintln!("{message}");
            return 2;
        }
    };
    match scan_folder(Path::new(folder)) {
        Ok(scans) => {
            print!("{}", report(&scans, &rate, &delta, &watch));
            0
        }
        Err(message) => {
            eprintln!("{message}");
            1
        }
    }
}
