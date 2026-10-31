//! Builders for the synthetic ELF images used by the tests.
//!
//! They are generated in memory, never committed: the repository must not carry
//! any binary derived from the game, and a hand-built image is enough to pin the
//! loader's behaviour.

#![allow(dead_code)]

pub const TEXT_ADDR: u32 = 0x0010_0000;
pub const RODATA_ADDR: u32 = 0x0010_0100;
pub const BSS_ADDR: u32 = 0x0010_0200;
pub const VUTEXT_ADDR: u32 = 0x0010_0300;

pub const TEXT_WORDS: [u32; 8] = [
    0x27BD_FFE8, // addiu sp, sp, -0x18
    0x03E0_0008, // jr ra
    0x0000_0000, // nop (delay slot)
    0x0C00_0004, // jal 0x00100010
    0x2402_0001, // addiu v0, zero, 1
    0x0000_000C, // syscall
    0x0085_1020, // add rd, a0, a1
    0x7C0F_2800, // sq (MMI, only a placeholder here)
];

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
    entry.push(0); // st_other
    entry.extend_from_slice(&shndx.to_le_bytes());
    entry
}

/// An ELF with `.text`, `.rodata`, `.vutext`, `.bss`, `.symtab`, `.strtab`,
/// `.shstrtab` and `.rel.text`.
pub fn sample_elf() -> Vec<u8> {
    let text: Vec<u8> = TEXT_WORDS.iter().flat_map(|word| word.to_le_bytes()).collect();
    let rodata = b"HELLO\0\0\0".to_vec();
    let vutext = vec![0u8; 16];

    let mut strtab = vec![0u8];
    let n_main = push_name(&mut strtab, "main");
    let n_helper = push_name(&mut strtab, "helper");

    let mut symtab = vec![0u8; 16];
    symtab.extend(symbol(n_main, TEXT_ADDR, text.len() as u32, 0x12, 1));
    symtab.extend(symbol(n_helper, TEXT_ADDR + 8, 8, 0x12, 1));

    let mut rel = Vec::with_capacity(8);
    rel.extend_from_slice(&(TEXT_ADDR + 4).to_le_bytes());
    rel.extend_from_slice(&((1u32 << 8) | 4u32).to_le_bytes());

    let mut shstrtab = vec![0u8];
    let s_text = push_name(&mut shstrtab, ".text");
    let s_rodata = push_name(&mut shstrtab, ".rodata");
    let s_vutext = push_name(&mut shstrtab, ".vutext");
    let s_bss = push_name(&mut shstrtab, ".bss");
    let s_symtab = push_name(&mut shstrtab, ".symtab");
    let s_strtab = push_name(&mut shstrtab, ".strtab");
    let s_shstrtab = push_name(&mut shstrtab, ".shstrtab");
    let s_rel = push_name(&mut shstrtab, ".rel.text");

    let mut file = vec![0u8; 52];
    let text_off = file.len() as u32;
    file.extend_from_slice(&text);
    let rodata_off = file.len() as u32;
    file.extend_from_slice(&rodata);
    let vutext_off = file.len() as u32;
    file.extend_from_slice(&vutext);
    let symtab_off = file.len() as u32;
    file.extend_from_slice(&symtab);
    let strtab_off = file.len() as u32;
    file.extend_from_slice(&strtab);
    let shstrtab_off = file.len() as u32;
    file.extend_from_slice(&shstrtab);
    let rel_off = file.len() as u32;
    file.extend_from_slice(&rel);
    let shoff = file.len() as u32;

    let flags_alloc_exec = 0x2 | 0x4;
    let headers: Vec<Vec<u8>> = vec![
        section_header(0, 0, 0, 0, 0, 0, 0, 0, 0, 0),
        section_header(s_text, 1, flags_alloc_exec, TEXT_ADDR, text_off, text.len() as u32, 0, 0, 4, 0),
        section_header(s_rodata, 1, 0x2, RODATA_ADDR, rodata_off, rodata.len() as u32, 0, 0, 1, 0),
        section_header(s_vutext, 1, flags_alloc_exec, VUTEXT_ADDR, vutext_off, vutext.len() as u32, 0, 0, 16, 0),
        section_header(s_bss, 8, 0x2 | 0x1, BSS_ADDR, 0, 0x100, 0, 0, 16, 0),
        section_header(s_symtab, 2, 0, 0, symtab_off, symtab.len() as u32, 6, 1, 4, 16),
        section_header(s_strtab, 3, 0, 0, strtab_off, strtab.len() as u32, 0, 0, 1, 0),
        section_header(s_shstrtab, 3, 0, 0, shstrtab_off, shstrtab.len() as u32, 0, 0, 1, 0),
        section_header(s_rel, 9, 0, 0, rel_off, rel.len() as u32, 5, 1, 4, 8),
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
    file[20..24].copy_from_slice(&1u32.to_le_bytes()); // e_version
    file[24..28].copy_from_slice(&TEXT_ADDR.to_le_bytes()); // e_entry
    file[28..32].copy_from_slice(&0u32.to_le_bytes()); // e_phoff
    file[32..36].copy_from_slice(&shoff.to_le_bytes());
    file[36..40].copy_from_slice(&0x2092_4085u32.to_le_bytes()); // e_flags
    file[40..42].copy_from_slice(&52u16.to_le_bytes());
    file[42..44].copy_from_slice(&0u16.to_le_bytes());
    file[44..46].copy_from_slice(&0u16.to_le_bytes());
    file[46..48].copy_from_slice(&40u16.to_le_bytes());
    file[48..50].copy_from_slice(&(headers.len() as u16).to_le_bytes());
    file[50..52].copy_from_slice(&7u16.to_le_bytes()); // .shstrtab

    file
}

/// An ELF without section headers: the loader must fall back to `PT_LOAD`.
pub fn segments_only_elf() -> Vec<u8> {
    let payload = [0x27BD_FFE8u32, 0x03E0_0008u32, 0x0000_0000u32, 0x2402_0001u32];
    let bytes: Vec<u8> = payload.iter().flat_map(|word| word.to_le_bytes()).collect();

    let mut file = vec![0u8; 52 + 32];
    let payload_off = file.len() as u32;
    file.extend_from_slice(&bytes);

    file[0..4].copy_from_slice(b"\x7fELF");
    file[4] = 1;
    file[5] = 1;
    file[6] = 1;
    file[16..18].copy_from_slice(&2u16.to_le_bytes());
    file[18..20].copy_from_slice(&8u16.to_le_bytes());
    file[20..24].copy_from_slice(&1u32.to_le_bytes());
    file[24..28].copy_from_slice(&TEXT_ADDR.to_le_bytes());
    file[28..32].copy_from_slice(&52u32.to_le_bytes()); // e_phoff
    file[32..36].copy_from_slice(&0u32.to_le_bytes()); // e_shoff
    file[36..40].copy_from_slice(&0u32.to_le_bytes());
    file[40..42].copy_from_slice(&52u16.to_le_bytes());
    file[42..44].copy_from_slice(&32u16.to_le_bytes());
    file[44..46].copy_from_slice(&1u16.to_le_bytes());
    file[46..48].copy_from_slice(&0u16.to_le_bytes());
    file[48..50].copy_from_slice(&0u16.to_le_bytes());
    file[50..52].copy_from_slice(&0u16.to_le_bytes());

    // One PT_LOAD segment: 16 bytes of file, 32 bytes of memory.
    let program = |offset: usize, value: u32, file: &mut Vec<u8>| {
        file[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    };
    program(52, 1, &mut file); // PT_LOAD
    program(56, payload_off, &mut file);
    program(60, TEXT_ADDR, &mut file);
    program(64, TEXT_ADDR, &mut file);
    program(68, bytes.len() as u32, &mut file);
    program(72, 32, &mut file); // p_memsz
    program(76, 0x1 | 0x4, &mut file); // PF_X | PF_R
    program(80, 4, &mut file);

    file
}

/// An ELF with a single `.text` section and a symbol table.
///
/// `symbols` are `(name, address, size)` of `STT_FUNC`/`STB_GLOBAL` symbols.
/// A symbol whose address is outside `.text` gets `SHN_ABS`, so it is neither
/// imported nor inside a code section.
pub fn text_only_elf(words: &[u32], symbols: &[(&str, u32, u32)]) -> Vec<u8> {
    let text: Vec<u8> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
    let text_end = TEXT_ADDR + text.len() as u32;

    let mut strtab = vec![0u8];
    let name_offsets: Vec<u32> = symbols.iter().map(|(name, _, _)| push_name(&mut strtab, name)).collect();

    let mut symtab = vec![0u8; 16];
    for ((_, address, size), name_offset) in symbols.iter().zip(&name_offsets) {
        let shndx: u16 = if *address >= TEXT_ADDR && *address < text_end { 1 } else { 0xFFF1 };
        symtab.extend(symbol(*name_offset, *address, *size, 0x12, shndx));
    }

    let mut shstrtab = vec![0u8];
    let s_text = push_name(&mut shstrtab, ".text");
    let s_symtab = push_name(&mut shstrtab, ".symtab");
    let s_strtab = push_name(&mut shstrtab, ".strtab");
    let s_shstrtab = push_name(&mut shstrtab, ".shstrtab");

    let mut file = vec![0u8; 52];
    let text_off = file.len() as u32;
    file.extend_from_slice(&text);
    let symtab_off = file.len() as u32;
    file.extend_from_slice(&symtab);
    let strtab_off = file.len() as u32;
    file.extend_from_slice(&strtab);
    let shstrtab_off = file.len() as u32;
    file.extend_from_slice(&shstrtab);
    let shoff = file.len() as u32;

    let headers: Vec<Vec<u8>> = vec![
        section_header(0, 0, 0, 0, 0, 0, 0, 0, 0, 0),
        section_header(s_text, 1, 0x2 | 0x4, TEXT_ADDR, text_off, text.len() as u32, 0, 0, 16, 0),
        section_header(s_symtab, 2, 0, 0, symtab_off, symtab.len() as u32, 3, 1, 4, 16),
        section_header(s_strtab, 3, 0, 0, strtab_off, strtab.len() as u32, 0, 0, 1, 0),
        section_header(s_shstrtab, 3, 0, 0, shstrtab_off, shstrtab.len() as u32, 0, 0, 1, 0),
    ];
    for header in &headers {
        file.extend_from_slice(header);
    }

    file[0..4].copy_from_slice(b"\x7fELF");
    file[4] = 1;
    file[5] = 1;
    file[6] = 1;
    file[16..18].copy_from_slice(&2u16.to_le_bytes());
    file[18..20].copy_from_slice(&8u16.to_le_bytes());
    file[20..24].copy_from_slice(&1u32.to_le_bytes());
    file[24..28].copy_from_slice(&TEXT_ADDR.to_le_bytes());
    file[28..32].copy_from_slice(&0u32.to_le_bytes());
    file[32..36].copy_from_slice(&shoff.to_le_bytes());
    file[36..40].copy_from_slice(&0x2092_4085u32.to_le_bytes());
    file[40..42].copy_from_slice(&52u16.to_le_bytes());
    file[42..44].copy_from_slice(&0u16.to_le_bytes());
    file[44..46].copy_from_slice(&0u16.to_le_bytes());
    file[46..48].copy_from_slice(&40u16.to_le_bytes());
    file[48..50].copy_from_slice(&(headers.len() as u16).to_le_bytes());
    file[50..52].copy_from_slice(&4u16.to_le_bytes());

    file
}
