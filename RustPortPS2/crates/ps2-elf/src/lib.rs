//! Minimal, dependency-free ELF32 little-endian MIPS reader.
//!
//! It reads a PS2 executable exactly like `PS2Recomp/ps2xRecomp/src/lib/elf_parser.cpp` (`parse`, `loadSections`,
//! `loadSymbols`, `loadRelocations`, `AppendLoadSegmentsAsSections`, `isExecutableSection`, `isDataSection`,
//! `readWord`, `isValidAddress`), because the generated output has to match the C++ recompiler byte for byte. Function
//! boundaries come from the Ghidra function map or from the heuristic scan; there is no DWARF pass.
//!
//! The parser borrows the file bytes, so the caller keeps the buffer alive:
//!
//! ```ignore
//! let bytes = std::fs::read("SCUS_973.99")?;
//! let elf = ps2_elf::ElfFile::parse(&bytes)?;
//! println!("{} sections, entry {:#x}", elf.sections().len(), elf.entry());
//! ```

use std::fmt;

pub mod code_identity;
pub mod funcmap;
pub mod functions;
pub mod number;

pub use funcmap::{is_auto_generated_name, FunctionMap, FunctionMapEntry};
pub use functions::{extract_functions, extract_functions_with_map, extras_from_map, Function};
pub use number::{parse_number, ParseNumberError};

/// `EM_MIPS`: the only machine accepted, exactly like the C++ parser.
pub const EM_MIPS: u16 = 8;

pub const SHT_NULL: u32 = 0;
pub const SHT_PROGBITS: u32 = 1;
pub const SHT_SYMTAB: u32 = 2;
pub const SHT_STRTAB: u32 = 3;
pub const SHT_RELA: u32 = 4;
pub const SHT_NOBITS: u32 = 8;
pub const SHT_REL: u32 = 9;
pub const SHT_DYNSYM: u32 = 11;

pub const SHF_WRITE: u32 = 0x1;
pub const SHF_ALLOC: u32 = 0x2;
pub const SHF_EXECINSTR: u32 = 0x4;

pub const PT_LOAD: u32 = 1;
pub const PF_X: u32 = 0x1;
pub const PF_W: u32 = 0x2;
pub const PF_R: u32 = 0x4;

pub const SHN_UNDEF: u16 = 0;
pub const SHN_XINDEX: u16 = 0xFFFF;
pub const STB_GLOBAL: u8 = 1;
pub const STT_FUNC: u8 = 2;

pub const ELF_HEADER_SIZE: usize = 52;
pub const SECTION_HEADER_SIZE: usize = 40;
pub const PROGRAM_HEADER_SIZE: usize = 32;
pub const SYMBOL_SIZE: usize = 16;
pub const REL_SIZE: usize = 8;
pub const RELA_SIZE: usize = 12;

/// What went wrong while reading the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElfError {
    TooShort { len: usize, needed: usize },
    BadMagic,
    NotElf32 { class: u8 },
    NotLittleEndian { data: u8 },
    NotMips { machine: u16 },
    Truncated { offset: usize, needed: usize, len: usize },
    Malformed(String),
}

impl fmt::Display for ElfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort { len, needed } => {
                write!(f, "file is {len} bytes, the ELF header needs {needed}")
            }
            Self::BadMagic => write!(f, "not an ELF file (bad magic)"),
            Self::NotElf32 { class } => write!(f, "not an ELF32 file (EI_CLASS = {class})"),
            Self::NotLittleEndian { data } => write!(f, "not little-endian (EI_DATA = {data})"),
            Self::NotMips { machine } => write!(f, "not a MIPS ELF file (e_machine = {machine})"),
            Self::Truncated { offset, needed, len } => {
                write!(f, "truncated file: {needed} byte(s) at offset {offset:#x}, file has {len}")
            }
            Self::Malformed(reason) => write!(f, "malformed ELF: {reason}"),
        }
    }
}

impl std::error::Error for ElfError {}

/// ELF header fields the recompiler reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub e_type: u16,
    pub machine: u16,
    pub version: u32,
    pub entry: u32,
    pub phoff: u32,
    pub shoff: u32,
    pub flags: u32,
    pub shstrndx: u16,
}

/// One section, mirroring `ps2recomp::Section`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section<'a> {
    pub name: String,
    pub sh_type: u32,
    pub flags: u32,
    pub address: u32,
    pub size: u32,
    pub offset: u32,
    pub link: u32,
    pub entsize: u32,
    pub is_code: bool,
    pub is_data: bool,
    pub is_bss: bool,
    pub is_read_only: bool,
    /// `None` for `SHT_NOBITS` (BSS) and for data outside the file.
    pub data: Option<&'a [u8]>,
}

impl Section<'_> {
    /// Address range check used by `FindSectionByAddress`.
    pub fn contains(&self, address: u32) -> bool {
        let end = (self.address as u64) + (self.size as u64);
        let address = address as u64;
        address >= self.address as u64 && address < end
    }
}

/// One symbol, mirroring `ps2recomp::Symbol`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub address: u32,
    pub size: u32,
    pub bind: u8,
    pub kind: u8,
    pub shndx: u16,
}

impl Symbol {
    pub fn is_function(&self) -> bool {
        self.kind == STT_FUNC
    }

    pub fn is_imported(&self) -> bool {
        self.shndx == SHN_UNDEF
    }

    pub fn is_exported(&self) -> bool {
        !self.is_imported() && self.bind == STB_GLOBAL
    }
}

/// One relocation, mirroring `ps2recomp::Relocation`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relocation {
    pub offset: u32,
    pub info: u32,
    pub symbol: u32,
    pub symbol_name: String,
    pub rtype: u32,
    /// Always 0 for `SHT_REL`, as in the C++ loader.
    pub addend: i32,
}

/// A parsed ELF file; borrows the file bytes.
#[derive(Debug, Clone)]
pub struct ElfFile<'a> {
    bytes: &'a [u8],
    header: Header,
    sections: Vec<Section<'a>>,
    symbols: Vec<Symbol>,
    relocations: Vec<Relocation>,
    warnings: Vec<String>,
}

impl<'a> ElfFile<'a> {
    /// Parse `bytes`, accepting only 32-bit little-endian MIPS images.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, ElfError> {
        if bytes.len() < ELF_HEADER_SIZE {
            return Err(ElfError::TooShort { len: bytes.len(), needed: ELF_HEADER_SIZE });
        }
        if &bytes[0..4] != b"\x7fELF" {
            return Err(ElfError::BadMagic);
        }
        let class = bytes[4];
        if class != 1 {
            return Err(ElfError::NotElf32 { class });
        }
        let data_encoding = bytes[5];
        if data_encoding != 1 {
            return Err(ElfError::NotLittleEndian { data: data_encoding });
        }
        let machine = u16_at(bytes, 18)?;
        if machine != EM_MIPS {
            return Err(ElfError::NotMips { machine });
        }

        let header = Header {
            e_type: u16_at(bytes, 16)?,
            machine,
            version: u32_at(bytes, 20)?,
            entry: u32_at(bytes, 24)?,
            phoff: u32_at(bytes, 28)?,
            shoff: u32_at(bytes, 32)?,
            flags: u32_at(bytes, 36)?,
            shstrndx: u16_at(bytes, 50)?,
        };
        let phentsize = u16_at(bytes, 42)? as usize;
        let phnum = u16_at(bytes, 44)? as usize;
        let shentsize = u16_at(bytes, 46)? as usize;
        let shnum = u16_at(bytes, 48)? as usize;

        let mut warnings = Vec::new();
        let mut sections = Vec::new();
        if header.shoff != 0 && shnum != 0 {
            sections = load_sections(bytes, &header, shentsize, shnum, &mut warnings)?;
        }
        if sections.is_empty() {
            sections = load_segments(bytes, &header, phentsize, phnum, &mut warnings);
            if !sections.is_empty() {
                warnings.push(format!(
                    "ELF has no section headers; using loadable segments as sections ({} entries).",
                    sections.len()
                ));
            }
        }

        let symbols = load_symbols(&sections, &mut warnings);
        let relocations = load_relocations(&sections, &mut warnings);

        Ok(Self { bytes, header, sections, symbols, relocations, warnings })
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    pub fn entry(&self) -> u32 {
        self.header.entry
    }

    pub fn sections(&self) -> &[Section<'a>] {
        &self.sections
    }

    pub fn symbols(&self) -> &[Symbol] {
        &self.symbols
    }

    pub fn relocations(&self) -> &[Relocation] {
        &self.relocations
    }

    /// Non-fatal findings, in the spirit of the C++ reporter's warnings.
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    pub fn section_by_name(&self, name: &str) -> Option<&Section<'a>> {
        self.sections.iter().find(|section| section.name == name)
    }

    pub fn section_data(&self, name: &str) -> Option<&'a [u8]> {
        self.section_by_name(name)?.data
    }

    pub fn section_address(&self, name: &str) -> Option<u32> {
        Some(self.section_by_name(name)?.address)
    }

    pub fn section_size(&self, name: &str) -> Option<u32> {
        Some(self.section_by_name(name)?.size)
    }

    pub fn find_section_by_address(&self, address: u32) -> Option<&Section<'a>> {
        self.sections.iter().find(|section| section.contains(address))
    }

    pub fn find_code_section_by_address(&self, address: u32) -> Option<&Section<'a>> {
        self.sections.iter().find(|section| section.is_code && section.contains(address))
    }

    /// Same fallback as `FindFunctionSectionByAddress`: when no section is
    /// marked executable (stripped/malformed ELF), any section matches.
    pub fn find_function_section_by_address(&self, address: u32) -> Option<&Section<'a>> {
        if let Some(section) = self.find_code_section_by_address(address) {
            return Some(section);
        }
        if !self.has_any_executable_section() {
            return self.find_section_by_address(address);
        }
        None
    }

    pub fn has_any_executable_section(&self) -> bool {
        self.sections.iter().any(|section| section.is_code)
    }

    pub fn is_valid_address(&self, address: u32) -> bool {
        self.find_section_by_address(address).is_some()
    }

    /// Read a little-endian word at a virtual address, like `readWord`.
    pub fn read_word(&self, address: u32) -> Option<u32> {
        let section = self.find_section_by_address(address)?;
        let data = section.data?;
        let offset = (address - section.address) as usize;
        if offset.checked_add(4)? > data.len() {
            return None;
        }
        Some(u32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]))
    }

    /// The file bytes this view borrows.
    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }
}

// --- loader -----------------------------------------------------------------

struct RawSection {
    name: u32,
    sh_type: u32,
    flags: u32,
    address: u32,
    offset: u32,
    size: u32,
    link: u32,
    entsize: u32,
}

fn load_sections<'a>(
    bytes: &'a [u8],
    header: &Header,
    shentsize: usize,
    shnum: usize,
    warnings: &mut Vec<String>,
) -> Result<Vec<Section<'a>>, ElfError> {
    let entsize = if shentsize >= SECTION_HEADER_SIZE { shentsize } else { SECTION_HEADER_SIZE };
    let table_end = (header.shoff as usize)
        .checked_add(shnum.checked_mul(entsize).ok_or_else(|| ElfError::Malformed("section table overflow".into()))?)
        .ok_or_else(|| ElfError::Malformed("section table overflow".into()))?;
    need(bytes, header.shoff as usize, table_end - header.shoff as usize)?;

    let mut raws = Vec::with_capacity(shnum);
    for index in 0..shnum {
        let base = header.shoff as usize + index * entsize;
        raws.push(RawSection {
            name: u32_at(bytes, base)?,
            sh_type: u32_at(bytes, base + 4)?,
            flags: u32_at(bytes, base + 8)?,
            address: u32_at(bytes, base + 12)?,
            offset: u32_at(bytes, base + 16)?,
            size: u32_at(bytes, base + 20)?,
            link: u32_at(bytes, base + 24)?,
            entsize: u32_at(bytes, base + 36)?,
        });
    }

    let names: Option<&[u8]> = if header.shstrndx != SHN_XINDEX && (header.shstrndx as usize) < raws.len() {
        let raw = &raws[header.shstrndx as usize];
        if raw.sh_type != SHT_NOBITS && raw.size > 0 {
            slice(bytes, raw.offset, raw.size)
        } else {
            None
        }
    } else {
        if header.shstrndx != SHN_XINDEX {
            warnings.push(format!("section name table index {} is out of range", header.shstrndx));
        }
        None
    };

    let mut sections = Vec::with_capacity(raws.len());
    for raw in raws {
        let name = names.and_then(|table| cstr(table, raw.name as usize)).unwrap_or_default();
        let is_bss = raw.sh_type == SHT_NOBITS;
        let data = if raw.size > 0 && !is_bss {
            match slice(bytes, raw.offset, raw.size) {
                Some(data) => Some(data),
                None => {
                    warnings.push(format!(
                        "section {name} data is outside the file (offset {:#x}, size {})",
                        raw.offset, raw.size
                    ));
                    None
                }
            }
        } else {
            None
        };
        sections.push(Section {
            name: name.clone(),
            sh_type: raw.sh_type,
            flags: raw.flags,
            address: raw.address,
            size: raw.size,
            offset: raw.offset,
            link: raw.link,
            entsize: raw.entsize,
            is_code: is_executable_section(&name, raw.flags),
            is_data: is_data_section(raw.flags),
            is_bss,
            is_read_only: raw.flags & SHF_WRITE == 0,
            data,
        });
    }
    Ok(sections)
}

/// `isExecutableSection`: `SHF_EXECINSTR`, minus the VU/DVP overlays whose
/// words use the VU ISA instead of the EE/R5900 ISA.
fn is_executable_section(name: &str, flags: u32) -> bool {
    if flags & SHF_EXECINSTR == 0 {
        return false;
    }
    let vu_text = name == ".vutext" || name.starts_with(".vutext.");
    let dvp_overlay = name == ".DVP.overlay" || name.starts_with(".DVP.overlay.");
    !vu_text && !dvp_overlay
}

fn is_data_section(flags: u32) -> bool {
    flags & SHF_ALLOC != 0 && flags & SHF_EXECINSTR == 0
}

/// `AppendLoadSegmentsAsSections`: used when the ELF has no section headers.
fn load_segments<'a>(
    bytes: &'a [u8],
    header: &Header,
    phentsize: usize,
    phnum: usize,
    warnings: &mut Vec<String>,
) -> Vec<Section<'a>> {
    if header.phoff == 0 || phnum == 0 {
        return Vec::new();
    }
    let entsize = if phentsize >= PROGRAM_HEADER_SIZE { phentsize } else { PROGRAM_HEADER_SIZE };
    let mut sections = Vec::new();
    for index in 0..phnum {
        let base = header.phoff as usize + index * entsize;
        let p_type = match u32_at(bytes, base) {
            Ok(value) => value,
            Err(_) => break,
        };
        if p_type != PT_LOAD {
            continue;
        }
        let (offset, vaddr, file_size, mem_size, flags) = match (
            u32_at(bytes, base + 4),
            u32_at(bytes, base + 8),
            u32_at(bytes, base + 16),
            u32_at(bytes, base + 20),
            u32_at(bytes, base + 24),
        ) {
            (Ok(offset), Ok(vaddr), Ok(file_size), Ok(mem_size), Ok(flags)) => {
                (offset, vaddr, file_size, mem_size, flags)
            }
            _ => break,
        };

        if file_size > 0 {
            match slice(bytes, offset, file_size) {
                Some(data) => sections.push(Section {
                    name: format!("LOAD{index}"),
                    sh_type: SHT_PROGBITS,
                    flags,
                    address: vaddr,
                    size: file_size,
                    offset,
                    link: 0,
                    entsize: 0,
                    is_code: flags & PF_X != 0,
                    is_data: flags & PF_W != 0 || flags & PF_R != 0,
                    is_bss: false,
                    is_read_only: flags & PF_W == 0,
                    data: Some(data),
                }),
                None => warnings.push(format!("load segment {index} is outside the file")),
            }
        }

        if mem_size > file_size {
            sections.push(Section {
                name: format!("LOAD{index}.bss"),
                sh_type: SHT_NOBITS,
                flags,
                address: vaddr.wrapping_add(file_size),
                size: mem_size - file_size,
                offset: offset.wrapping_add(file_size),
                link: 0,
                entsize: 0,
                is_code: false,
                is_data: true,
                is_bss: true,
                is_read_only: false,
                data: None,
            });
        }
    }
    sections.sort_by_key(|section| section.address);
    sections
}

fn load_symbols(sections: &[Section<'_>], warnings: &mut Vec<String>) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    for section in sections {
        if section.sh_type != SHT_SYMTAB && section.sh_type != SHT_DYNSYM {
            continue;
        }
        let link = section.link as usize;
        if link >= sections.len() {
            warnings.push(format!("symbol section link out of bounds: {link}"));
            continue;
        }
        let strings = match sections[link].data {
            Some(data) => data,
            None => {
                warnings.push(format!("symbol section {} has no string table", section.name));
                continue;
            }
        };
        let data = match section.data {
            Some(data) => data,
            None => {
                warnings.push(format!("symbol section {} has no data", section.name));
                continue;
            }
        };
        let entsize = if section.entsize as usize >= SYMBOL_SIZE { section.entsize as usize } else { SYMBOL_SIZE };
        let count = data.len() / entsize;
        for index in 0..count {
            let base = index * entsize;
            let name_offset = u32_at(data, base).unwrap_or(0);
            let address = u32_at(data, base + 4).unwrap_or(0);
            let size = u32_at(data, base + 8).unwrap_or(0);
            let info = data[base + 12];
            let shndx = u16_at(data, base + 14).unwrap_or(0);
            let name = match cstr(strings, name_offset as usize) {
                Some(name) if !name.is_empty() => name,
                _ => continue,
            };
            symbols.push(Symbol { name, address, size, bind: info >> 4, kind: info & 0xF, shndx });
        }
    }
    symbols
}

fn load_relocations(sections: &[Section<'_>], warnings: &mut Vec<String>) -> Vec<Relocation> {
    let mut relocations = Vec::new();
    for section in sections {
        let is_rel = section.sh_type == SHT_REL;
        let is_rela = section.sh_type == SHT_RELA;
        if !is_rel && !is_rela {
            continue;
        }
        let link = section.link as usize;
        if link >= sections.len() {
            warnings.push(format!("relocation section link out of bounds: {link}"));
            continue;
        }
        let data = match section.data {
            Some(data) => data,
            None => continue,
        };
        let minimum = if is_rela { RELA_SIZE } else { REL_SIZE };
        let entsize = if section.entsize as usize >= minimum { section.entsize as usize } else { minimum };
        let count = data.len() / entsize;
        for index in 0..count {
            let base = index * entsize;
            let offset = u32_at(data, base).unwrap_or(0);
            let info = u32_at(data, base + 4).unwrap_or(0);
            let symbol = info >> 8;
            let rtype = info & 0xFF;
            let addend = if is_rela { i32_at(data, base + 8).unwrap_or(0) } else { 0 };
            let symbol_name = symbol_name(sections, link, symbol);
            relocations.push(Relocation { offset, info, symbol, symbol_name, rtype, addend });
        }
    }
    relocations
}

/// Name of symbol `symbol` in the symbol table at `sections[symtab]`.
fn symbol_name(sections: &[Section<'_>], symtab: usize, symbol: u32) -> String {
    let Some(table) = sections.get(symtab) else {
        return String::new();
    };
    let Some(strings) = sections.get(table.link as usize).and_then(|section| section.data) else {
        return String::new();
    };
    let Some(data) = table.data else {
        return String::new();
    };
    let entsize = if table.entsize as usize >= SYMBOL_SIZE { table.entsize as usize } else { SYMBOL_SIZE };
    let base = (symbol as usize).saturating_mul(entsize);
    if base + SYMBOL_SIZE > data.len() {
        return String::new();
    }
    let name_offset = u32_at(data, base).unwrap_or(0);
    cstr(strings, name_offset as usize).unwrap_or_default()
}

// --- byte helpers -----------------------------------------------------------

fn need(bytes: &[u8], offset: usize, len: usize) -> Result<(), ElfError> {
    let end = offset.checked_add(len).ok_or(ElfError::Truncated { offset, needed: len, len: bytes.len() })?;
    if end > bytes.len() {
        return Err(ElfError::Truncated { offset, needed: len, len: bytes.len() });
    }
    Ok(())
}

fn u16_at(bytes: &[u8], offset: usize) -> Result<u16, ElfError> {
    need(bytes, offset, 2)?;
    Ok(u16::from_le_bytes([bytes[offset], bytes[offset + 1]]))
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, ElfError> {
    need(bytes, offset, 4)?;
    Ok(u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]]))
}

fn i32_at(bytes: &[u8], offset: usize) -> Result<i32, ElfError> {
    Ok(u32_at(bytes, offset)? as i32)
}

fn slice(bytes: &[u8], offset: u32, len: u32) -> Option<&[u8]> {
    let start = offset as usize;
    let end = start.checked_add(len as usize)?;
    bytes.get(start..end)
}

fn cstr(bytes: &[u8], offset: usize) -> Option<String> {
    let tail = bytes.get(offset..)?;
    let end = tail.iter().position(|byte| *byte == 0)?;
    Some(String::from_utf8_lossy(&tail[..end]).into_owned())
}
