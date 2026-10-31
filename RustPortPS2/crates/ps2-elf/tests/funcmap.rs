//! Tests for the Ghidra function-map parser.

mod common;

use std::path::PathBuf;

use ps2_elf::{funcmap::is_auto_generated_name, ElfFile, FunctionMap};

fn sample_elf() -> Vec<u8> {
    common::sample_elf()
}

/// The real `games/scus-97399/funcmap.csv` must stay readable line by line. The ELF here
/// is a stub with one 16-byte code segment, so almost every function is
/// rejected as "outside an executable section" — what this test checks is that
/// every single line has the expected shape.
#[test]
fn reads_every_line_of_the_repository_function_map() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../games/scus-97399/funcmap.csv");
    let text = std::fs::read_to_string(&path).expect("games/scus-97399/funcmap.csv must exist");
    let bytes = common::segments_only_elf();
    let elf = ElfFile::parse(&bytes).unwrap();

    let map = FunctionMap::from_ghidra_csv(&text, &elf);
    let rows = text.lines().skip(1).filter(|line| !line.is_empty()).count();

    assert!(rows > 6000, "unexpected number of rows: {rows}");
    assert_eq!(map.skipped_lines(), 0, "every row of the real map must be four fields with parsable addresses");
    assert_eq!(
        map.len() + map.rejected_outside_executable() + map.rejected_invalid_range(),
        rows,
        "every row must be either accepted or counted as rejected"
    );
    // 0x00100008 falls inside the stub's code segment.
    assert_eq!(map.len(), 1);
    assert_eq!(map.entries()[0].start, 0x0010_0008);
}

#[test]
fn parses_sorts_clamps_and_filters() {
    let bytes = sample_elf();
    let elf = ElfFile::parse(&bytes).unwrap();
    let csv = "\
Name,Start,End,Size
main,0x00100000,0x00100020,32
clamped,0x00100010,0x00110000,0
zero_end,0x00100010,0x0,0
sub_00100000,0x00100000,0x00100020,32
vutext,0x00100300,0x00100310,16
rodata,0x00100100,0x00100108,8
";
    let map = FunctionMap::from_ghidra_csv(csv, &elf);

    // `main` wins over the auto-generated name at the same address, and the two
    // entries at 0x00100010 collapse into the first one after sorting.
    let names: Vec<&str> = map.entries().iter().map(|entry| entry.name.as_str()).collect();
    assert_eq!(names, ["main", "clamped"]);

    assert_eq!(map.entries()[0].start, 0x0010_0000);
    assert_eq!(map.entries()[0].end, 0x0010_0020);

    // The end past the section is clamped to the section end; a zero end too.
    assert_eq!(map.entries()[1].start, 0x0010_0010);
    assert_eq!(map.entries()[1].end, 0x0010_0020);

    // `.vutext` and `.rodata` are not EE code.
    assert_eq!(map.rejected_outside_executable(), 2);
    assert_eq!(map.rejected_invalid_range(), 0);
    assert_eq!(map.skipped_lines(), 0);

    assert!(map.find(0x0010_0000).is_some());
    assert!(map.find(0x0010_0004).is_none());
}

#[test]
fn the_first_line_is_always_a_header() {
    let bytes = sample_elf();
    let elf = ElfFile::parse(&bytes).unwrap();

    // Without a header line the C++ loader silently loses the first function.
    let map = FunctionMap::from_ghidra_csv("main,0x00100000,0x00100020,32\n", &elf);
    assert!(map.is_empty());
}

#[test]
fn counts_lines_that_are_not_four_fields() {
    let bytes = sample_elf();
    let elf = ElfFile::parse(&bytes).unwrap();
    let csv = "\
Name,Start,End,Size
main,0x00100000,0x00100020
helper,0x00100008,not-a-number,8
";
    let map = FunctionMap::from_ghidra_csv(csv, &elf);
    assert!(map.is_empty());
    assert_eq!(map.skipped_lines(), 2);
}

#[test]
fn recognises_auto_generated_names() {
    for name in ["sub_00100008", "FUN_00100008", "LAB_00100008", "DAT_00100008"] {
        assert!(is_auto_generated_name(name), "{name}");
    }
    for name in ["main", "sceGsResetGraph", "snd_SendIOPCommandAndWait"] {
        assert!(!is_auto_generated_name(name), "{name}");
    }
}
