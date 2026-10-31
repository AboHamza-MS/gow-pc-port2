//! `iop-gen`: ahead-of-time compiler for the game's IRX modules (IOP, R3000A) to C++ (`ps2xIOP/src/native`).
//!
//! Port of `tools/iop/iopn_gen.py`. `<dump dir>` comes from a run with `PS2X_IOP_DUMP_MODULES=<dump dir>`:
//! `<name>.bin` is the module's code as relocated at its load address, `modules.txt` lists
//! `<name> <base hex> <text size hex> <fnv hash hex>` per load. The runtime uses a generated module only when the module
//! is loaded again at the same base with the same bytes.
//!
//! Every instruction is emitted as specialised C++ with exactly the semantics of `iopExecute` (`ps2xIOP`
//! `core/iop_cpu_exec.h`): operands are read before the pending load of the previous instruction is committed, the commit
//! skips a register the instruction itself wrote, loads (and `MFC0`) become the new pending load, `LWL`/`LWR` merge with a
//! pending value of the same register, exceptions go through `iopRaiseException`. A branch is evaluated, its delay slot is
//! emitted inline right after it, then the code jumps to the target or falls through. Code is split in chunks (one C++
//! function each); a chunk switches over all its addresses for dynamic entries.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

/// Instructions per C++ function.
const CHUNK: usize = 512;

/// `IRX functions replaced by native C++` (`<module ident> <offset hex> <name>` per line).
pub const DEFAULT_OVERRIDES: &str = include_str!("../../../native/ps2xIOP/native_overrides.txt");

/// The override offsets per module ident.
pub fn parse_overrides(text: &str) -> BTreeMap<String, BTreeSet<usize>> {
    let mut result: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split('#').next().unwrap_or("").split_whitespace().collect();
        if parts.len() >= 2 {
            if let Ok(offset) = usize::from_str_radix(parts[1], 16) {
                result.entry(parts[0].to_owned()).or_default().insert(offset);
            }
        }
    }
    result
}

fn sext16(value: u32) -> i64 {
    i64::from(value as u16 as i16)
}

struct Fields {
    op: u32,
    rs: u32,
    rt: u32,
    rd: u32,
    sa: u32,
    fn_: u32,
    imm: u32,
}

fn fields(w: u32) -> Fields {
    Fields {
        op: w >> 26,
        rs: (w >> 21) & 31,
        rt: (w >> 16) & 31,
        rd: (w >> 11) & 31,
        sa: (w >> 6) & 31,
        fn_: w & 63,
        imm: w & 0xFFFF,
    }
}

fn is_branch(w: u32) -> bool {
    let f = fields(w);
    match f.op {
        0 => matches!(f.fn_, 0x08 | 0x09),
        1 => matches!(f.rt, 0x00 | 0x01 | 0x10 | 0x11),
        op => matches!(op, 0x02..=0x07),
    }
}

/// A general register as a C++ operand.
fn g(r: u32) -> String {
    if r == 0 {
        "0u".to_owned()
    } else {
        format!("cpu.gpr[{r}]")
    }
}

/// Commit the pending load of the previous instruction (unless this instruction wrote that register).
fn commit(written: u32) -> Vec<String> {
    if written != 0 {
        vec![format!(
            "    if (cpu.pendingLoad) {{ if (cpu.pendingLoadReg != {written}u) cpu.gpr[cpu.pendingLoadReg] = cpu.pendingLoadValue; cpu.pendingLoad = false; }}"
        )]
    } else {
        vec![
            "    if (cpu.pendingLoad) { cpu.gpr[cpu.pendingLoadReg] = cpu.pendingLoadValue; cpu.pendingLoad = false; }"
                .to_owned(),
        ]
    }
}

fn raise(code: u32, pc: &str, delay: bool, addr: Option<&str>) -> String {
    let a = addr.map(|a| format!(", {a}")).unwrap_or_default();
    format!(
        "{{ iopRaiseException(cpu, {code}u, {pc}, {}{a}); stop = true; return n; }}",
        if delay { "true" } else { "false" }
    )
}

/// C++ for one non-branch instruction at physical address `a`; `delay` = it runs in a branch delay slot.
fn emit(w: u32, a: u32, delay: bool) -> Vec<String> {
    let Fields { op, rs, rt, rd, sa, fn_, imm } = fields(w);
    let simm = sext16(imm);
    let simm_hex = simm as u32;
    let pc = format!("seg | 0x{a:x}u");
    let mut lines: Vec<String> = vec!["    ++n;".to_owned()];

    let write = |lines: &mut Vec<String>, reg: u32, expr: &str| {
        if reg == 0 {
            lines.extend(commit(0));
        } else {
            lines.push(format!("    {{ const uint32_t v = {expr};"));
            lines.extend(commit(reg).into_iter().map(|c| format!("  {c}")));
            lines.push(format!("      cpu.gpr[{reg}] = v; }}"));
        }
    };
    let load = |lines: &mut Vec<String>, reg: u32, expr: &str, pre: &[String]| {
        lines.push("    {".to_owned());
        lines.extend(pre.iter().map(|p| format!("      {p}")));
        lines.push(format!("      const uint32_t v = {expr};"));
        lines.extend(commit(0).into_iter().map(|c| format!("  {c}")));
        if reg != 0 {
            lines.push(format!("      cpu.pendingLoad = true; cpu.pendingLoadReg = {reg}u; cpu.pendingLoadValue = v;"));
        }
        lines.push("    }".to_owned());
    };
    let other = |lines: &mut Vec<String>, statements: &[String]| {
        lines.extend(statements.iter().map(|s| format!("    {s}")));
        lines.extend(commit(0));
    };

    let ea = format!("{} + 0x{simm_hex:x}u", g(rs));
    let raise_here = |code: u32| format!("    {}", raise(code, &pc, delay, None));
    let overflow_check = |lines: &mut Vec<String>, setup: String| {
        lines.push(setup);
        lines.push(format!("      if (r > 0x7FFFFFFFll || r < -0x80000000ll) {} }}", raise(12, &pc, delay, None)));
    };

    match op {
        0 => match fn_ {
            0x00 => write(&mut lines, rd, &format!("{} << {sa}u", g(rt))),
            0x02 => write(&mut lines, rd, &format!("{} >> {sa}u", g(rt))),
            0x03 => write(&mut lines, rd, &format!("static_cast<uint32_t>(static_cast<int32_t>({}) >> {sa})", g(rt))),
            0x04 => write(&mut lines, rd, &format!("{} << ({} & 31u)", g(rt), g(rs))),
            0x06 => write(&mut lines, rd, &format!("{} >> ({} & 31u)", g(rt), g(rs))),
            0x07 => write(&mut lines, rd, &format!("static_cast<uint32_t>(static_cast<int32_t>({}) >> ({} & 31u))", g(rt), g(rs))),
            0x0C => lines.push(raise_here(8)),
            0x0D => lines.push(raise_here(9)),
            0x10 => write(&mut lines, rd, "cpu.hi"),
            0x11 => other(&mut lines, &[format!("cpu.hi = {};", g(rs))]),
            0x12 => write(&mut lines, rd, "cpu.lo"),
            0x13 => other(&mut lines, &[format!("cpu.lo = {};", g(rs))]),
            0x18 => other(
                &mut lines,
                &[format!(
                    "{{ const int64_t r = static_cast<int64_t>(static_cast<int32_t>({})) * static_cast<int64_t>(static_cast<int32_t>({})); cpu.lo = static_cast<uint32_t>(r); cpu.hi = static_cast<uint32_t>(static_cast<uint64_t>(r) >> 32u); }}",
                    g(rs),
                    g(rt)
                )],
            ),
            0x19 => other(
                &mut lines,
                &[format!(
                    "{{ const uint64_t r = static_cast<uint64_t>({}) * {}; cpu.lo = static_cast<uint32_t>(r); cpu.hi = static_cast<uint32_t>(r >> 32u); }}",
                    g(rs),
                    g(rt)
                )],
            ),
            0x1A => other(&mut lines, &[format!("iopDiv(cpu, {}, {});", g(rs), g(rt))]),
            0x1B => other(&mut lines, &[format!("iopDivu(cpu, {}, {});", g(rs), g(rt))]),
            0x20 | 0x22 => {
                let sign = if fn_ == 0x20 { '+' } else { '-' };
                overflow_check(
                    &mut lines,
                    format!(
                        "    {{ const int64_t r = static_cast<int64_t>(static_cast<int32_t>({})) {sign} static_cast<int32_t>({});",
                        g(rs),
                        g(rt)
                    ),
                );
                write(&mut lines, rd, &format!("{} {sign} {}", g(rs), g(rt)));
            }
            0x21 => write(&mut lines, rd, &format!("{} + {}", g(rs), g(rt))),
            0x23 => write(&mut lines, rd, &format!("{} - {}", g(rs), g(rt))),
            0x24 => write(&mut lines, rd, &format!("{} & {}", g(rs), g(rt))),
            0x25 => write(&mut lines, rd, &format!("{} | {}", g(rs), g(rt))),
            0x26 => write(&mut lines, rd, &format!("{} ^ {}", g(rs), g(rt))),
            0x27 => write(&mut lines, rd, &format!("~({} | {})", g(rs), g(rt))),
            0x2A => write(&mut lines, rd, &format!("static_cast<int32_t>({}) < static_cast<int32_t>({}) ? 1u : 0u", g(rs), g(rt))),
            0x2B => write(&mut lines, rd, &format!("{} < {} ? 1u : 0u", g(rs), g(rt))),
            _ => lines.push(raise_here(10)),
        },
        // Branches are handled by the caller; other REGIMM codes are invalid.
        0x01 => lines.push(raise_here(10)),
        0x08 => {
            overflow_check(
                &mut lines,
                format!("    {{ const int64_t r = static_cast<int64_t>(static_cast<int32_t>({})) + ({simm});", g(rs)),
            );
            write(&mut lines, rt, &format!("{} + 0x{simm_hex:x}u", g(rs)));
        }
        0x09 => write(&mut lines, rt, &format!("{} + 0x{simm_hex:x}u", g(rs))),
        0x0A => write(&mut lines, rt, &format!("static_cast<int32_t>({}) < {simm} ? 1u : 0u", g(rs))),
        0x0B => write(&mut lines, rt, &format!("{} < 0x{simm_hex:x}u ? 1u : 0u", g(rs))),
        0x0C => write(&mut lines, rt, &format!("{} & 0x{imm:x}u", g(rs))),
        0x0D => write(&mut lines, rt, &format!("{} | 0x{imm:x}u", g(rs))),
        0x0E => write(&mut lines, rt, &format!("{} ^ 0x{imm:x}u", g(rs))),
        0x0F => write(&mut lines, rt, &format!("0x{:x}u", imm << 16)),
        0x10 => {
            if rs == 0x00 {
                load(&mut lines, rt, &format!("cpu.cop0[{rd}]"), &[]);
            } else if rs == 0x04 {
                other(&mut lines, &[format!("cpu.cop0[{rd}] = {};", g(rt))]);
            } else if rs == 0x10 && fn_ == 0x10 {
                other(&mut lines, &["{ const uint32_t s = cpu.cop0[12]; cpu.cop0[12] = (s & ~0x0Fu) | ((s >> 2u) & 0x0Fu); }".to_owned()]);
            } else {
                lines.push(raise_here(10));
            }
        }
        0x20 | 0x24 => {
            let expr = if op == 0x20 {
                "static_cast<uint32_t>(static_cast<int32_t>(static_cast<int8_t>(memory.load8(addr))))"
            } else {
                "memory.load8(addr)"
            };
            load(&mut lines, rt, expr, &[format!("const uint32_t addr = {ea};")]);
        }
        0x21 | 0x25 => {
            let expr = if op == 0x21 {
                "static_cast<uint32_t>(static_cast<int32_t>(static_cast<int16_t>(memory.load16(addr))))"
            } else {
                "memory.load16(addr)"
            };
            let pre = [format!("const uint32_t addr = {ea};"), format!("if (addr & 1u) {}", raise(4, &pc, delay, Some("addr")))];
            load(&mut lines, rt, expr, &pre);
        }
        0x23 => {
            let pre = [format!("const uint32_t addr = {ea};"), format!("if (addr & 3u) {}", raise(4, &pc, delay, Some("addr")))];
            load(&mut lines, rt, "memory.load32(addr)", &pre);
        }
        0x22 | 0x26 => {
            let old = format!("(cpu.pendingLoad && cpu.pendingLoadReg == {rt}u ? cpu.pendingLoadValue : {})", g(rt));
            let expr = if op == 0x22 {
                format!("({old} & kLwlMask[addr & 3u]) | (memory.load32(addr & ~3u) << kLwlShift[addr & 3u])")
            } else {
                format!("({old} & kLwrMask[addr & 3u]) | (memory.load32(addr & ~3u) >> kLwrShift[addr & 3u])")
            };
            load(&mut lines, rt, &expr, &[format!("const uint32_t addr = {ea};")]);
        }
        0x28 => other(&mut lines, &[format!("memory.store8({ea}, static_cast<uint8_t>({}));", g(rt))]),
        0x29 => {
            lines.push(format!(
                "    {{ const uint32_t addr = {ea}; if (addr & 1u) {} memory.store16(addr, static_cast<uint16_t>({})); }}",
                raise(5, &pc, delay, Some("addr")),
                g(rt)
            ));
            lines.extend(commit(0));
        }
        0x2B => {
            lines.push(format!(
                "    {{ const uint32_t addr = {ea}; if (addr & 3u) {} memory.store32(addr, {}); }}",
                raise(5, &pc, delay, Some("addr")),
                g(rt)
            ));
            lines.extend(commit(0));
        }
        0x2A => other(&mut lines, &[format!("iopSwl(memory, {ea}, {});", g(rt))]),
        0x2E => other(&mut lines, &[format!("iopSwr(memory, {ea}, {});", g(rt))]),
        _ => lines.push(raise_here(10)),
    }
    lines
}

/// Evaluates the branch at `a`: sets the locals `tk` (taken) and `tg` (target), writes the link register.
fn emit_branch(w: u32, a: u32) -> Vec<String> {
    let Fields { op, rs, rt, rd, fn_, imm, .. } = fields(w);
    let link = format!("seg | 0x{:x}u", a.wrapping_add(8));
    let relative_target = |a: u32| (i64::from(a) + 4 + (sext16(imm) << 2)) & 0x1FFF_FFFF;
    let mut lines = vec!["    ++n;".to_owned()];

    if op == 0 && fn_ == 0x08 {
        lines.push(format!("    tk = true; tg = {};", g(rs)));
        lines.extend(commit(0));
    } else if op == 0 && fn_ == 0x09 {
        let r = if rd != 0 { rd } else { 31 };
        lines.push(format!("    cpu.gpr[{r}] = {link}; tk = true; tg = {};", g(rs)));
        lines.extend(commit(r));
    } else if op == 0x01 {
        let cond = if matches!(rt, 0x00 | 0x10) {
            format!("static_cast<int32_t>({}) < 0", g(rs))
        } else {
            format!("static_cast<int32_t>({}) >= 0", g(rs))
        };
        let links = matches!(rt, 0x10 | 0x11);
        if links {
            lines.push(format!("    cpu.gpr[31] = {link};"));
        }
        lines.push(format!("    tk = {cond}; tg = seg | 0x{:x}u;", relative_target(a)));
        lines.extend(commit(if links { 31 } else { 0 }));
    } else if matches!(op, 0x02 | 0x03) {
        let target = (a.wrapping_add(4) & 0xF000_0000) | ((w & 0x03FF_FFFF) << 2);
        if op == 0x03 {
            lines.push(format!("    cpu.gpr[31] = {link};"));
        }
        lines.push(format!("    tk = true; tg = (seg & 0xF0000000u) | 0x{:x}u;", target & 0x0FFF_FFFF));
        lines.extend(commit(if op == 0x03 { 31 } else { 0 }));
    } else {
        let cond = match op {
            0x04 => format!("{} == {}", g(rs), g(rt)),
            0x05 => format!("{} != {}", g(rs), g(rt)),
            0x06 => format!("static_cast<int32_t>({}) <= 0", g(rs)),
            _ => format!("static_cast<int32_t>({}) > 0", g(rs)),
        };
        lines.push(format!("    tk = {cond}; tg = seg | 0x{:x}u;", relative_target(a)));
        lines.extend(commit(0));
    }
    lines
}

fn static_target(w: u32, a: u32) -> Option<u32> {
    let Fields { op, imm, .. } = fields(w);
    if op == 0 {
        return None;
    }
    if matches!(op, 0x02 | 0x03) {
        return Some((a.wrapping_add(4) & 0xF000_0000) | ((w & 0x03FF_FFFF) << 2));
    }
    Some(((i64::from(a) + 4 + (sext16(imm) << 2)) & 0x1FFF_FFFF) as u32)
}

/// One module's C++; returns the name of its entry function.
pub fn gen_module(
    name: &str,
    base: u32,
    words: &[u32],
    overrides: &BTreeMap<String, BTreeSet<usize>>,
    out: &mut String,
) -> String {
    let n = words.len();
    let mut stub = vec![false; n];
    for i in 0..n.saturating_sub(1) {
        if words[i] == 0x03E0_0008 && (words[i + 1] & 0xFFFF_0000) == 0x2400_0000 {
            stub[i] = true;
        }
    }
    // The ident without the base suffix.
    let module = name.rsplit_once('_').map_or(name, |(head, _)| head);
    if let Some(offsets) = overrides.get(module) {
        for &offset in offsets {
            if offset % 4 == 0 && offset / 4 < n {
                stub[offset / 4] = true; // native override: hand over like an import stub
            }
        }
    }

    let function = format!("iopn_{name}");
    let chunks = n.div_ceil(CHUNK);
    let mut l: Vec<String> = vec![
        format!("// generated by ps2-tools iop-gen from {name} (base 0x{base:x}, {n} words) - do not edit"),
        "#include \"native/iop_native_support.h\"".to_owned(),
        String::new(),
        "namespace ps2x::iop::detail".to_owned(),
        "{".to_owned(),
        "namespace".to_owned(),
        "{".to_owned(),
    ];

    for c in 0..chunks {
        let lo = c * CHUNK;
        let hi = n.min((c + 1) * CHUNK);
        let clo = base as usize + lo * 4;
        let chi = base as usize + hi * 4;

        // Transfer to `addr` (cpu.pc already holds it).
        let go = |addr: u32| -> String {
            let addr = addr as usize;
            if clo <= addr && addr < chi {
                format!("goto L{:x};", (addr - base as usize) / 4)
            } else {
                "return n;".to_owned()
            }
        };
        let is_delay = |i: usize| i > 0 && is_branch(words[i - 1]) && !stub[i - 1];

        l.push(format!(
            "uint32_t chunk{c}(IopCpuState &cpu, IopMemory &memory, uint32_t budget, const uint32_t seg, bool &stop)"
        ));
        l.push("{".to_owned());
        l.push("    uint32_t n = 0;".to_owned());
        l.push("    bool tk = false;".to_owned());
        l.push("    uint32_t tg = 0;".to_owned());
        l.push("    (void)tk; (void)tg;".to_owned());
        l.push("dispatch:".to_owned());
        l.push("    if (n >= budget) { stop = true; return n; }".to_owned());
        l.push(format!("    switch ((cpu.pc & 0x1FFFFFFFu) - 0x{clo:x}u)"));
        l.push("    {".to_owned());
        for i in lo..hi {
            l.push(format!("    case 0x{:x}u: goto {}{i:x};", (i - lo) * 4, if is_delay(i) { "D" } else { "L" }));
        }
        l.push("    default: return n;".to_owned());
        l.push("    }".to_owned());

        for i in lo..hi {
            let w = words[i];
            let a = base + (i as u32) * 4;
            if is_delay(i) {
                // Entered directly (not after its branch): an ordinary instruction.
                l.push(format!("D{i:x}:"));
                l.extend(emit(w, a, false));
                l.push(format!("    cpu.pc = seg | 0x{:x}u;", a + 4));
                l.push(format!("    {}", if i + 1 < hi { go(a + 4) } else { "return n;".to_owned() }));
                continue;
            }
            l.push(format!("L{i:x}:"));
            if stub[i] {
                l.push(format!("    cpu.pc = seg | 0x{a:x}u; stop = true; return n;"));
                continue;
            }
            if is_branch(w) {
                l.extend(emit_branch(w, a));
                if i + 1 < n {
                    let d = words[i + 1];
                    if is_branch(d) {
                        // Branch in a delay slot: hand over to the interpreter.
                        l.push(format!("    cpu.pc = seg | 0x{a:x}u; stop = true; return n - 1;"));
                        continue;
                    }
                    l.extend(emit(d, a + 4, true));
                }
                l.push("    if (tk) {".to_owned());
                l.push("        cpu.pc = tg;".to_owned());
                l.push("        if (n >= budget) { stop = true; return n; }".to_owned());
                match static_target(w, a) {
                    Some(target) => l.push(format!("        {}", go(target))),
                    None => l.push("        goto dispatch;".to_owned()),
                }
                l.push("    }".to_owned());
                l.push(format!("    cpu.pc = seg | 0x{:x}u;", a + 8));
                l.push("    if (n >= budget) { stop = true; return n; }".to_owned());
                l.push(format!("    {}", if i + 2 < hi { go(a + 8) } else { "return n;".to_owned() }));
                continue;
            }
            l.extend(emit(w, a, false));
            if i + 1 == hi {
                l.push(format!("    cpu.pc = seg | 0x{:x}u;", a + 4));
                l.push("    return n;".to_owned());
            }
        }
        l.push("}".to_owned());
        l.push(String::new());
    }

    l.push("using Chunk = uint32_t (*)(IopCpuState &, IopMemory &, uint32_t, uint32_t, bool &);".to_owned());
    let table: Vec<String> = (0..chunks).map(|c| format!("chunk{c}")).collect();
    l.push(format!("const Chunk kChunks[] = {{{}}};", table.join(", ")));
    l.push("}".to_owned());
    l.push(String::new());
    l.push(format!("uint32_t {function}(IopCpuState &cpu, IopMemory &memory, uint32_t budget)"));
    l.push("{".to_owned());
    l.push("    const uint32_t seg = cpu.pc & 0xE0000000u;".to_owned());
    l.push("    cpu.exception = false;".to_owned());
    l.push("    cpu.yielded = false;".to_owned());
    l.push("    uint32_t n = 0;".to_owned());
    l.push("    while (n < budget)".to_owned());
    l.push("    {".to_owned());
    l.push(format!("        const uint32_t offset = (cpu.pc & 0x1FFFFFFFu) - 0x{base:x}u;"));
    l.push(format!("        if (offset >= 0x{:x}u || (offset & 3u) != 0u || (cpu.pc & 0xE0000000u) != seg)", n * 4));
    l.push("            return n;".to_owned());
    l.push("        bool stop = false;".to_owned());
    l.push(format!(
        "        const uint32_t ran = kChunks[offset / {}u](cpu, memory, budget - n, seg, stop);",
        CHUNK * 4
    ));
    l.push("        n += ran;".to_owned());
    l.push("        if (stop || ran == 0u)".to_owned());
    l.push("            return n;".to_owned());
    l.push("    }".to_owned());
    l.push("    return n;".to_owned());
    l.push("}".to_owned());
    l.push("}".to_owned());
    out.push_str(&l.join("\n"));
    out.push('\n');
    function
}

/// `iop-gen DUMP OUT [--overrides FILE]`.
pub fn run(dump: &Path, output: &Path, overrides_text: &str) -> Result<(), String> {
    fs::create_dir_all(output).map_err(|e| format!("{}: {e}", output.display()))?;
    let overrides = parse_overrides(overrides_text);

    // The last load of a module at a base wins.
    let listing = fs::read_to_string(dump.join("modules.txt"))
        .map_err(|e| format!("{}: {e}", dump.join("modules.txt").display()))?;
    let mut seen: BTreeMap<(String, u32), (usize, u64)> = BTreeMap::new();
    for line in listing.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() != 4 {
            continue;
        }
        let parse = |s: &str| u64::from_str_radix(s, 16).map_err(|_| format!("bad number in modules.txt: {s}"));
        seen.insert((parts[0].to_owned(), parse(parts[1])? as u32), (parse(parts[2])? as usize, parse(parts[3])?));
    }

    let mut entries = Vec::new();
    for ((name, base), (size, hash)) in &seen {
        let path = dump.join(format!("{name}.bin"));
        let data = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        if data.len() != *size {
            println!("skip {name}: dump has {} bytes, list says {size}", data.len());
            continue;
        }
        let words: Vec<u32> =
            data[..size / 4 * 4].chunks_exact(4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]])).collect();
        let ident = format!("{name}_{base:x}");
        let mut code = String::new();
        let function = gen_module(&ident, *base, &words, &overrides, &mut code);
        let target = output.join(format!("iopn_{ident}.cpp"));
        fs::write(&target, code).map_err(|e| format!("{}: {e}", target.display()))?;
        entries.push((name.clone(), *base, *size, *hash, function));
        println!("{name}: base 0x{base:x}, {} instructions", words.len());
    }

    let mut registry =
        String::from("// generated by ps2-tools iop-gen - do not edit\n#include \"native/iop_native.h\"\n\n");
    registry.push_str("namespace ps2x::iop::detail\n{\n");
    for (_, _, _, _, function) in &entries {
        let _ = writeln!(registry, "uint32_t {function}(IopCpuState &, IopMemory &, uint32_t);");
    }
    registry.push_str("\nnamespace\n{\nconst IopNativeModule kModules[] = {\n");
    for (name, base, size, hash, function) in &entries {
        let _ = writeln!(registry, "    {{\"{name}\", 0x{base:x}u, 0x{size:x}u, 0x{hash:x}ull, &{function}}},");
    }
    if entries.is_empty() {
        registry.push_str("    {nullptr, 0u, 0u, 0ull, nullptr},\n");
    }
    registry.push_str("};\n}\n\n");
    registry.push_str("const IopNativeModule *iopNativeModules(size_t &count)\n{\n");
    let _ = write!(registry, "    count = {};\n    return kModules;\n}}\n}}\n", entries.len());
    let target = output.join("iopn_registry.cpp");
    fs::write(&target, registry).map_err(|e| format!("{}: {e}", target.display()))
}
