//! Shared data types of the recompiler pipeline (`ps2recomp/types.h`).
//!
//! The ELF reader's types borrow the file bytes; the pipeline needs owned
//! copies that outlive the parse, so the few fields it reads are copied here.

use ps2_elf::{ElfFile, Section as ElfSection, Symbol as ElfSymbol};

/// `ps2recomp::Function` (the instructions live in a separate map, like the C++).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Function {
    pub name: String,
    pub start: u32,
    pub end: u32,
    pub is_recompiled: bool,
    pub is_stub: bool,
    pub is_skipped: bool,
}

impl From<&ps2_elf::Function> for Function {
    fn from(function: &ps2_elf::Function) -> Self {
        Self {
            name: function.name.clone(),
            start: function.start,
            end: function.end,
            is_recompiled: false,
            is_stub: function.is_stub,
            is_skipped: function.is_skipped,
        }
    }
}

/// `ps2recomp::Symbol`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub address: u32,
    pub size: u32,
    pub is_function: bool,
    pub is_imported: bool,
    pub is_exported: bool,
}

impl From<&ElfSymbol> for Symbol {
    fn from(symbol: &ElfSymbol) -> Self {
        Self {
            name: symbol.name.clone(),
            address: symbol.address,
            size: symbol.size,
            is_function: symbol.is_function(),
            is_imported: symbol.is_imported(),
            is_exported: symbol.is_exported(),
        }
    }
}

/// `ps2recomp::Section`, with the bytes owned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub name: String,
    pub address: u32,
    pub size: u32,
    pub is_code: bool,
    pub is_data: bool,
    pub is_bss: bool,
    pub is_read_only: bool,
    pub data: Option<Vec<u8>>,
}

impl From<&ElfSection<'_>> for Section {
    fn from(section: &ElfSection<'_>) -> Self {
        Self {
            name: section.name.clone(),
            address: section.address,
            size: section.size,
            is_code: section.is_code,
            is_data: section.is_data,
            is_bss: section.is_bss,
            is_read_only: section.is_read_only,
            data: section.data.map(<[u8]>::to_vec),
        }
    }
}

impl Section {
    /// `address >= section.address && address < section.address + section.size`
    /// with the C++ 32-bit arithmetic.
    pub fn contains(&self, address: u32) -> bool {
        address >= self.address && address < self.address.wrapping_add(self.size)
    }
}

/// Copies of the ELF tables the pipeline keeps after the parse.
pub struct ElfTables {
    pub symbols: Vec<Symbol>,
    pub sections: Vec<Section>,
}

impl ElfTables {
    pub fn from_elf(elf: &ElfFile<'_>) -> Self {
        Self {
            symbols: elf.symbols().iter().map(Symbol::from).collect(),
            sections: elf.sections().iter().map(Section::from).collect(),
        }
    }
}
