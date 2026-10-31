//! The synthetic ELF the parity gate uses when the game is not available.
//!
//! It is generated in memory, never committed: the repository must not carry any
//! binary derived from the game. The image holds one function per instruction
//! family, swept over every sub-opcode the translators know (and some they do not),
//! plus the shapes the control flow code cares about: a jump table, a GIF DMA
//! kick, calls, loops, likely branches, a syscall and a function that falls off
//! its end. The expected tree comes from the C++ recompiler once, as a manifest.

/// Where the code, the read-only data and the BSS live.
pub const TEXT_ADDR: u32 = 0x0010_0000;
pub const RODATA_ADDR: u32 = 0x0020_0000;
pub const BSS_ADDR: u32 = 0x0030_0000;
const BSS_SIZE: u32 = 0x1000;

const NOP: u32 = 0;

fn word(op: u32, rs: u32, rt: u32, rd: u32, sa: u32, function: u32) -> u32 {
    (op << 26) | (rs << 21) | (rt << 16) | (rd << 11) | (sa << 6) | function
}

fn imm(op: u32, rs: u32, rt: u32, immediate: u32) -> u32 {
    (op << 26) | (rs << 21) | (rt << 16) | (immediate & 0xFFFF)
}

const JR_RA: u32 = (31 << 21) | 8;

struct Function {
    name: String,
    words: Vec<u32>,
}

/// The fixture: its ELF image and the config that goes with it.
pub struct Fixture {
    pub elf: Vec<u8>,
    functions: Vec<(String, u32, u32)>,
}

fn sweep(name: &str, instructions: impl IntoIterator<Item = u32>) -> Function {
    let mut words = Vec::new();
    for instruction in instructions {
        words.push(instruction);
        words.push(NOP);
    }
    words.push(JR_RA);
    words.push(NOP);
    Function { name: name.to_owned(), words }
}

/// Operand patterns `(rs, rt, rd, sa)`: all set, all zero, and the mixed zero forms the translators
/// special-case (a write to `$zero`, a read of `$zero`).
const PATTERNS: [(u32, u32, u32, u32); 4] = [(5, 6, 7, 3), (0, 0, 0, 0), (5, 0, 7, 0), (5, 6, 0, 31)];

fn primary_sweep() -> Function {
    let skipped = [0, 1, 0x10, 0x11, 0x12, 0x1C];
    let mut instructions = Vec::new();
    for (rs, rt, _, _) in PATTERNS {
        for op in 2..64u32 {
            if skipped.contains(&op) {
                continue;
            }
            // J/JAL carry a target; the I-type forms use 0x1234 (and a short positive offset below).
            let immediate = if matches!(op, 2 | 3) { 0x40 } else { 0x1234 };
            instructions.push(imm(op, rs, rt, immediate));
        }
        // The branch forms with a short forward offset keep the targets inside the function.
        for op in [4, 5, 6, 7, 20, 21, 22, 23] {
            instructions.push(imm(op, rs, rt, 2));
        }
        // Negative immediates print as the unsigned 32-bit pattern.
        for op in [8, 9, 10, 11, 12, 13, 14, 0x18, 0x19] {
            instructions.push(imm(op, rs, rt, 0xFFE8));
        }
    }
    sweep("fx_primary", instructions)
}

fn special_sweep() -> Function {
    let mut instructions = Vec::new();
    for (rs, rt, rd, sa) in PATTERNS {
        instructions.extend((0..64u32).map(|function| word(0, rs, rt, rd, sa, function)));
    }
    sweep("fx_special", instructions)
}

fn regimm_sweep() -> Function {
    let mut instructions = Vec::new();
    for (rs, _, _, _) in PATTERNS {
        instructions.extend((0..32u32).map(|rt| imm(1, rs, rt, 2)));
    }
    sweep("fx_regimm", instructions)
}

fn cop0_sweep() -> Function {
    let mut instructions = Vec::new();
    for (_, rt, rd, _) in PATTERNS {
        instructions.extend((0..32u32).map(|rs| word(0x10, rs, rt, rd, 0, 0)));
    }
    instructions.extend((0..64u32).map(|function| word(0x10, 16, 0, 0, 0, function)));
    instructions.extend((0..4u32).map(|condition| imm(0x10, 8, condition, 2)));
    sweep("fx_cop0", instructions)
}

fn cop1_sweep() -> Function {
    let mut instructions = Vec::new();
    for (_, rt, rd, sa) in PATTERNS {
        instructions.extend((0..32u32).map(|rs| word(0x11, rs, rt, rd, sa, 0)));
    }
    for format in [16u32, 17, 20, 21] {
        for (_, ft, fs, fd) in PATTERNS {
            instructions.extend((0..64u32).map(|function| word(0x11, format, ft, fs, fd, function)));
        }
    }
    instructions.extend((0..4u32).map(|condition| imm(0x11, 8, condition, 2)));
    sweep("fx_cop1", instructions)
}

fn cop2_sweep() -> Function {
    let mut instructions = Vec::new();
    for (_, rt, rd, _) in PATTERNS {
        instructions.extend((0..16u32).map(|rs| word(0x12, rs, rt, rd, 0, 0)));
    }
    instructions.extend((0..4u32).map(|condition| imm(0x12, 8, condition, 2)));

    // The macro block: bit 25 set, the destination mask in bits 21-24. Special1 is chosen by the
    // function, Special2 by `sa:function[1:0]` when the function is 0x3C-0x3F.
    let macro_word = |dest: u32, rt: u32, rd: u32, sa: u32, function: u32| {
        (0x12 << 26) | (1 << 25) | (dest << 21) | (rt << 16) | (rd << 11) | (sa << 6) | function
    };
    for dest in [0xFu32, 0x8, 0x6, 0x1] {
        for (rt, rd, sa) in [(3u32, 4u32, 5u32), (0, 0, 0), (0, 4, 1), (3, 0, 2)] {
            for function in 0..0x3Cu32 {
                instructions.push(macro_word(dest, rt, rd, sa, function));
            }
        }
        for (rt, rd) in [(3u32, 4u32), (0, 4), (3, 0)] {
            for sa in 0..32u32 {
                for low in 0..4u32 {
                    instructions.push(macro_word(dest, rt, rd, sa, 0x3C | low));
                }
            }
        }
    }
    sweep("fx_cop2", instructions)
}

fn mmi_sweep() -> Function {
    let mut instructions = Vec::new();
    for (rs, rt, rd, sa) in PATTERNS {
        instructions.extend((0..64u32).map(|function| word(0x1C, rs, rt, rd, sa, function)));
        for table in [0x08u32, 0x09, 0x28, 0x29, 0x30, 0x31] {
            instructions.extend((0..32u32).map(|subfunction| word(0x1C, rs, rt, rd, subfunction, table)));
        }
    }
    sweep("fx_mmi", instructions)
}

fn ld_st_sweep() -> Function {
    // Memory accesses off a constant base, so the hardware-register fast paths are exercised.
    let mut instructions = Vec::new();
    let hardware_bases = [0x1000u32, 0x1001, 0x1200, 0x1F80, 0x7000, 0x8000, 0xA000, 0xBFC0, 0x0010, 0x1100];
    for (index, base) in hardware_bases.iter().enumerate() {
        instructions.push(imm(0x0F, 0, 8, *base)); // lui t0, base
        for (_, rt, _, _) in PATTERNS {
            for op in [0x20u32, 0x21, 0x23, 0x24, 0x25, 0x27, 0x28, 0x29, 0x2B, 0x37, 0x3F, 0x1E, 0x1F, 0x31, 0x39] {
                instructions.push(imm(op, 8, rt, (index as u32) * 16));
            }
        }
    }
    sweep("fx_memory", instructions)
}

fn jump_table_function(case_addresses: &[u32]) -> Function {
    let table_high = RODATA_ADDR >> 16;
    let table_low = RODATA_ADDR & 0xFFFF;
    let cases = case_addresses.len() as u32;
    let mut words = vec![
        imm(0x0B, 4, 2, cases),            // sltiu v0, a0, cases
        imm(0x04, 2, 0, (cases * 2) + 10), // beq v0, zero, out
        NOP,                               //
        imm(0x0F, 0, 3, table_high),       // lui v1, high
        imm(0x09, 3, 3, table_low),        // addiu v1, v1, low
        word(0, 0, 4, 4, 2, 0),            // sll a0, a0, 2
        word(0, 3, 4, 3, 0, 0x21),         // addu v1, v1, a0
        imm(0x23, 3, 2, 0),                // lw v0, 0(v1)
        NOP,                               //
        word(0, 2, 0, 0, 0, 8),            // jr v0
        NOP,                               //
    ];
    for case in 0..cases {
        words.push(imm(0x09, 0, 2, case + 1)); // addiu v0, zero, case + 1
        words.push(JR_RA);
        words.push(NOP);
    }
    words.push(JR_RA);
    words.push(NOP);
    Function { name: "fx_jump_table".to_owned(), words }
}

fn gif_dma_function() -> Function {
    let mut words = Vec::new();
    let targets = [0xE020u32, 0xE010, 0xA030, 0xA000];
    words.push(imm(0x0F, 0, 8, 0x1000)); // lui t0, 0x1000
    for (index, target) in targets.iter().enumerate() {
        words.push(imm(0x0D, 8, 9, *target)); // ori t1, t0, target
        words.push(imm(0x2B, 9, 10 + index as u32, 0)); // sw (t2 + n), 0(t1)
    }
    words.push(JR_RA);
    words.push(NOP);
    Function { name: "fx_gif_dma".to_owned(), words }
}

fn flow_function(leaf: u32) -> Function {
    let words = vec![
        imm(0x09, 29, 29, 0xFFE0),               // addiu sp, sp, -0x20
        (3 << 26) | ((leaf >> 2) & 0x03FF_FFFF), // jal fx_leaf
        NOP,                                     //
        imm(0x09, 0, 8, 3),                      // addiu t0, zero, 3
        imm(0x09, 8, 8, 0xFFFF),                 // loop: addiu t0, t0, -1
        imm(0x05, 8, 0, 0xFFFE),                 // bne t0, zero, loop
        NOP,                                     //
        imm(0x14, 8, 0, 3),                      // beql t0, zero, +3
        imm(0x09, 0, 9, 7),                      //   addiu t1, zero, 7 (delay slot)
        word(0, 25, 0, 31, 0, 9),                // jalr ra, t9
        NOP,                                     //
        0x0000_000C,                             // syscall
        (2 << 26) | ((leaf >> 2) & 0x03FF_FFFF), // j fx_leaf
        NOP,                                     //
    ];
    Function { name: "fx_flow".to_owned(), words }
}

fn fall_through_function() -> Function {
    Function { name: "fx_fall_through".to_owned(), words: vec![imm(0x09, 0, 8, 1), imm(0x09, 0, 9, 2)] }
}

fn leaf_function() -> Function {
    Function { name: "fx_leaf".to_owned(), words: vec![imm(0x09, 0, 2, 5), JR_RA, NOP] }
}

/// Fill with `nop` up to `address`, as unnamed filler (no symbol).
fn pad_to(functions: &mut Vec<Function>, address: u32) {
    let used: u32 = functions.iter().map(|f| f.words.len() as u32 * 4).sum::<u32>() + TEXT_ADDR;
    assert!(used <= address, "fixture layout overlaps at 0x{address:x}");
    let missing = (address - used) / 4;
    if missing > 0 {
        functions.push(Function { name: format!("fx_pad_{address:x}"), words: vec![NOP; missing as usize] });
    }
}

fn push_name(table: &mut Vec<u8>, name: &str) -> u32 {
    let offset = table.len() as u32;
    table.extend_from_slice(name.as_bytes());
    table.push(0);
    offset
}

#[allow(clippy::too_many_arguments)]
fn section_header(
    name: u32,
    sh_type: u32,
    flags: u32,
    address: u32,
    offset: u32,
    size: u32,
    link: u32,
    info: u32,
    align: u32,
    entsize: u32,
) -> Vec<u8> {
    let mut header = Vec::with_capacity(40);
    for value in [name, sh_type, flags, address, offset, size, link, info, align, entsize] {
        header.extend_from_slice(&value.to_le_bytes());
    }
    header
}

fn symbol(name: u32, value: u32, size: u32, info: u8, shndx: u16) -> Vec<u8> {
    let mut entry = Vec::with_capacity(16);
    entry.extend_from_slice(&name.to_le_bytes());
    entry.extend_from_slice(&value.to_le_bytes());
    entry.extend_from_slice(&size.to_le_bytes());
    entry.push(info);
    entry.push(0);
    entry.extend_from_slice(&shndx.to_le_bytes());
    entry
}

impl Fixture {
    pub fn build() -> Self {
        // The leaf comes first so its address is known when the flow function calls it.
        let leaf = leaf_function();
        let leaf_address = TEXT_ADDR;

        // The jump table cases follow the 11-word prologue of the jump table function.
        let jump_table_address = TEXT_ADDR + 0x100;
        let case_count = 4u32;
        let cases: Vec<u32> = (0..case_count).map(|case| jump_table_address + (11 + case * 3) * 4).collect();

        let mut functions: Vec<Function> = vec![leaf];
        pad_to(&mut functions, jump_table_address);
        functions.push(jump_table_function(&cases));
        functions.push(gif_dma_function());
        functions.push(flow_function(leaf_address));
        functions.push(fall_through_function());
        functions.push(primary_sweep());
        functions.push(special_sweep());
        functions.push(regimm_sweep());
        functions.push(cop0_sweep());
        functions.push(cop1_sweep());
        functions.push(cop2_sweep());
        functions.push(mmi_sweep());
        functions.push(ld_st_sweep());
        // Functions that the config turns into a stub, a syscall stub and a skipped one.
        functions.push(Function { name: "memcpy".to_owned(), words: vec![imm(0x09, 0, 2, 1), JR_RA, NOP] });
        functions.push(Function { name: "FlushCache".to_owned(), words: vec![imm(0x09, 0, 2, 2), JR_RA, NOP] });
        functions.push(Function { name: "fx_skipped".to_owned(), words: vec![imm(0x09, 0, 2, 3), JR_RA, NOP] });

        let mut text: Vec<u8> = Vec::new();
        let mut layout: Vec<(String, u32, u32)> = Vec::new();
        for function in &functions {
            let address = TEXT_ADDR + text.len() as u32;
            let size = function.words.len() as u32 * 4;
            text.extend(function.words.iter().flat_map(|word| word.to_le_bytes()));
            if !function.name.starts_with("fx_pad_") {
                layout.push((function.name.clone(), address, size));
            }
        }

        let rodata: Vec<u8> = cases.iter().flat_map(|case| case.to_le_bytes()).collect();

        let mut strtab = vec![0u8];
        let mut symtab = vec![0u8; 16];
        for (name, address, size) in &layout {
            let name_offset = push_name(&mut strtab, name);
            symtab.extend(symbol(name_offset, *address, *size, 0x12, 1));
        }
        let gp_name = push_name(&mut strtab, "_gp");
        symtab.extend(symbol(gp_name, RODATA_ADDR + 0x8000, 0, 0x11, 2));

        let mut shstrtab = vec![0u8];
        let s_text = push_name(&mut shstrtab, ".text");
        let s_rodata = push_name(&mut shstrtab, ".rodata");
        let s_bss = push_name(&mut shstrtab, ".bss");
        let s_symtab = push_name(&mut shstrtab, ".symtab");
        let s_strtab = push_name(&mut shstrtab, ".strtab");
        let s_shstrtab = push_name(&mut shstrtab, ".shstrtab");

        let mut file = vec![0u8; 52];
        let text_off = file.len() as u32;
        file.extend_from_slice(&text);
        let rodata_off = file.len() as u32;
        file.extend_from_slice(&rodata);
        let symtab_off = file.len() as u32;
        file.extend_from_slice(&symtab);
        let strtab_off = file.len() as u32;
        file.extend_from_slice(&strtab);
        let shstrtab_off = file.len() as u32;
        file.extend_from_slice(&shstrtab);
        let shoff = file.len() as u32;

        let alloc_exec = 0x2 | 0x4;
        let headers = [
            section_header(0, 0, 0, 0, 0, 0, 0, 0, 0, 0),
            section_header(s_text, 1, alloc_exec, TEXT_ADDR, text_off, text.len() as u32, 0, 0, 4, 0),
            section_header(s_rodata, 1, 0x2, RODATA_ADDR, rodata_off, rodata.len() as u32, 0, 0, 4, 0),
            section_header(s_bss, 8, 0x2 | 0x1, BSS_ADDR, 0, BSS_SIZE, 0, 0, 16, 0),
            section_header(s_symtab, 2, 0, 0, symtab_off, symtab.len() as u32, 5, 1, 4, 16),
            section_header(s_strtab, 3, 0, 0, strtab_off, strtab.len() as u32, 0, 0, 1, 0),
            section_header(s_shstrtab, 3, 0, 0, shstrtab_off, shstrtab.len() as u32, 0, 0, 1, 0),
        ];
        for header in &headers {
            file.extend_from_slice(header);
        }

        file[0..4].copy_from_slice(b"\x7fELF");
        file[4] = 1; // ELFCLASS32
        file[5] = 1; // ELFDATA2LSB
        file[6] = 1; // EV_CURRENT
        file[16..18].copy_from_slice(&2u16.to_le_bytes()); // ET_EXEC
        file[18..20].copy_from_slice(&8u16.to_le_bytes()); // EM_MIPS
        file[20..24].copy_from_slice(&1u32.to_le_bytes());
        file[24..28].copy_from_slice(&jump_table_address.to_le_bytes()); // e_entry
        file[32..36].copy_from_slice(&shoff.to_le_bytes());
        file[36..40].copy_from_slice(&0x2092_4085u32.to_le_bytes());
        file[40..42].copy_from_slice(&52u16.to_le_bytes());
        file[46..48].copy_from_slice(&40u16.to_le_bytes());
        file[48..50].copy_from_slice(&(headers.len() as u16).to_le_bytes());
        file[50..52].copy_from_slice(&6u16.to_le_bytes()); // .shstrtab

        Self { elf: file, functions: layout }
    }

    /// The address of a fixture function by name.
    pub fn address_of(&self, name: &str) -> Option<u32> {
        self.functions.iter().find(|(function, _, _)| function == name).map(|(_, address, _)| *address)
    }

    /// The `recomp.toml` for this fixture: a stub with an explicit address, a syscall
    /// stub by name, a skipped function, an entry hint, a patch and an MMIO mark.
    pub fn config_toml(&self, input: &str, output: &str) -> String {
        let memcpy = self.address_of("memcpy").unwrap_or(0);
        let skipped = self.address_of("fx_skipped").unwrap_or(0);
        let hint = self.address_of("fx_special").unwrap_or(0) + 0x20;
        let patched = self.address_of("fx_flow").unwrap_or(0) + 4;
        let mmio = self.address_of("fx_gif_dma").unwrap_or(0) + 4;
        format!(
            r#"[general]
input = "{input}"
output = "{output}"
single_file_output = false
patch_syscalls = false
patch_cop0 = true
patch_cache = true
stubs = ["memcpy@0x{memcpy:08X}", "FlushCache"]
skip = ["fx_skipped@0x{skipped:08X}"]
entry_points = ["fx_hint@0x{hint:08X}"]

[mmio]
"0x{mmio:x}" = "0x1000f000"

[patches]
"0x{patched:x}" = "0x00000000"
"#
        )
    }
}
