//! Loader tests against hand-built ELF images.

mod common;

use ps2_elf::{ElfError, ElfFile};

#[test]
fn parses_header_and_classifies_sections() {
    let bytes = common::sample_elf();
    let elf = ElfFile::parse(&bytes).expect("the fixture must parse");

    assert_eq!(elf.header().machine, 8);
    assert_eq!(elf.header().e_type, 2);
    assert_eq!(elf.header().flags, 0x2092_4085);
    assert_eq!(elf.entry(), common::TEXT_ADDR);
    assert!(elf.warnings().is_empty(), "{:?}", elf.warnings());

    let names: Vec<&str> = elf.sections().iter().map(|section| section.name.as_str()).collect();
    assert_eq!(names, ["", ".text", ".rodata", ".vutext", ".bss", ".symtab", ".strtab", ".shstrtab", ".rel.text"]);

    let text = elf.section_by_name(".text").expect(".text");
    assert!(text.is_code);
    assert!(!text.is_data);
    assert!(!text.is_bss);
    assert!(text.is_read_only);
    assert_eq!(text.size, 32);

    let rodata = elf.section_by_name(".rodata").expect(".rodata");
    assert!(!rodata.is_code);
    assert!(rodata.is_data);
    assert_eq!(rodata.data, Some(&b"HELLO\0\0\0"[..]));

    // Sony's linker marks VU microprograms executable, but they are not EE code.
    let vutext = elf.section_by_name(".vutext").expect(".vutext");
    assert!(!vutext.is_code, "VU microprogram sections must not be EE code");

    let bss = elf.section_by_name(".bss").expect(".bss");
    assert!(bss.is_bss);
    assert!(bss.is_data);
    assert_eq!(bss.data, None);
}

#[test]
fn reads_words_and_validates_addresses() {
    let bytes = common::sample_elf();
    let elf = ElfFile::parse(&bytes).unwrap();

    for (index, word) in common::TEXT_WORDS.iter().enumerate() {
        let address = common::TEXT_ADDR + (index as u32) * 4;
        assert_eq!(elf.read_word(address), Some(*word), "word {index}");
    }

    // A read that would cross the section end is refused, like `readWord`.
    assert_eq!(elf.read_word(common::TEXT_ADDR + 30), None);
    assert_eq!(elf.read_word(common::TEXT_ADDR + 32), None);
    // BSS has no bytes.
    assert_eq!(elf.read_word(common::BSS_ADDR), None);
    assert_eq!(elf.read_word(0x0000_FFFF), None);

    assert!(elf.is_valid_address(common::TEXT_ADDR));
    assert!(elf.is_valid_address(common::BSS_ADDR + 0xFF));
    // 0x0010_0300 is the start of `.vutext`, which is 16 bytes long.
    assert!(elf.is_valid_address(common::VUTEXT_ADDR));
    assert!(!elf.is_valid_address(common::VUTEXT_ADDR + 0x10));
    // The non-allocated sections sit at address 0 in this fixture (as in a real
    // ELF), so address 0 is "covered" by `.symtab` and counts as valid, exactly
    // like the C++ `isValidAddress`.
    assert!(elf.is_valid_address(0));
    assert!(!elf.is_valid_address(0x0020_0000));

    assert_eq!(elf.section_address(".text"), Some(common::TEXT_ADDR));
    assert_eq!(elf.section_size(".text"), Some(32));
    assert_eq!(elf.section_data(".rodata"), Some(&b"HELLO\0\0\0"[..]));
    assert_eq!(elf.section_data(".bss"), None);
}

#[test]
fn parses_symbols() {
    let bytes = common::sample_elf();
    let elf = ElfFile::parse(&bytes).unwrap();

    assert_eq!(elf.symbols().len(), 2);
    let main = &elf.symbols()[0];
    assert_eq!(main.name, "main");
    assert_eq!(main.address, common::TEXT_ADDR);
    assert_eq!(main.size, 32);
    assert!(main.is_function());
    assert!(main.is_exported());
    assert!(!main.is_imported());
    assert_eq!(main.bind, 1);
    assert_eq!(main.shndx, 1);

    let helper = &elf.symbols()[1];
    assert_eq!(helper.name, "helper");
    assert_eq!(helper.address, common::TEXT_ADDR + 8);
}

#[test]
fn parses_relocations() {
    let bytes = common::sample_elf();
    let elf = ElfFile::parse(&bytes).unwrap();

    assert_eq!(elf.relocations().len(), 1);
    let relocation = &elf.relocations()[0];
    assert_eq!(relocation.offset, common::TEXT_ADDR + 4);
    assert_eq!(relocation.symbol, 1);
    assert_eq!(relocation.symbol_name, "main");
    assert_eq!(relocation.rtype, 4);
    // `SHT_REL` has no addend, as in the C++ loader.
    assert_eq!(relocation.addend, 0);
    assert_eq!(relocation.info, (1 << 8) | 4);
}

#[test]
fn falls_back_to_load_segments_without_section_headers() {
    let bytes = common::segments_only_elf();
    let elf = ElfFile::parse(&bytes).expect("segment-only ELF");

    let names: Vec<&str> = elf.sections().iter().map(|section| section.name.as_str()).collect();
    assert_eq!(names, ["LOAD0", "LOAD0.bss"]);

    let load = &elf.sections()[0];
    assert_eq!(load.address, common::TEXT_ADDR);
    assert_eq!(load.size, 16);
    assert!(load.is_code);
    assert!(load.is_data);
    assert!(load.is_read_only);

    let bss = &elf.sections()[1];
    assert_eq!(bss.address, common::TEXT_ADDR + 16);
    assert_eq!(bss.size, 16);
    assert!(bss.is_bss);
    assert_eq!(bss.data, None);

    assert!(elf.warnings().iter().any(|warning| warning.contains("no section headers")), "{:?}", elf.warnings());
    assert_eq!(elf.read_word(common::TEXT_ADDR), Some(common::TEXT_WORDS[0]));
}

#[test]
fn rejects_files_that_are_not_32_bit_little_endian_mips() {
    let mut bytes = common::sample_elf();
    bytes[0] = 0;
    assert_eq!(ElfFile::parse(&bytes).unwrap_err(), ElfError::BadMagic);

    let mut bytes = common::sample_elf();
    bytes[4] = 2;
    assert_eq!(ElfFile::parse(&bytes).unwrap_err(), ElfError::NotElf32 { class: 2 });

    let mut bytes = common::sample_elf();
    bytes[5] = 2;
    assert_eq!(ElfFile::parse(&bytes).unwrap_err(), ElfError::NotLittleEndian { data: 2 });

    let mut bytes = common::sample_elf();
    bytes[18..20].copy_from_slice(&3u16.to_le_bytes());
    assert_eq!(ElfFile::parse(&bytes).unwrap_err(), ElfError::NotMips { machine: 3 });
}

#[test]
fn rejects_truncated_files() {
    let bytes = common::sample_elf();
    assert!(matches!(ElfFile::parse(&bytes[..40]).unwrap_err(), ElfError::TooShort { needed: 52, .. }));

    let mut truncated = common::sample_elf();
    let shoff = truncated.len() as u32;
    truncated[32..36].copy_from_slice(&shoff.to_le_bytes());
    assert!(matches!(ElfFile::parse(&truncated).unwrap_err(), ElfError::Truncated { .. }));
}
