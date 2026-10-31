//! Tests for the function-boundary merge (`extractFunctions`).

mod common;

use ps2_elf::{extract_functions, extract_functions_with_map, extras_from_map, ElfFile, Function, FunctionMap};

const TEXT: u32 = common::TEXT_ADDR;

fn function(name: &str, start: u32, end: u32) -> Function {
    Function { name: name.to_owned(), start, end, is_stub: false, is_skipped: false }
}

fn summary(functions: &[Function]) -> Vec<(&str, u32, u32)> {
    functions.iter().map(|function| (function.name.as_str(), function.start, function.end)).collect()
}

#[test]
fn symbols_become_functions_and_open_ends_chain() {
    let words = [0u32; 16]; // 64 bytes of .text
    let bytes = common::text_only_elf(
        &words,
        &[("main", TEXT, 32), ("zero_a", TEXT + 0x20, 0), ("zero_b", TEXT + 0x30, 0), ("outside", 0x0020_0000, 16)],
    );
    let elf = ElfFile::parse(&bytes).unwrap();

    let functions = extract_functions(&elf, &[], None);
    assert_eq!(
        summary(&functions),
        [
            ("main", TEXT, TEXT + 0x20),
            // Size 0: the end becomes the next start, then the section end.
            ("zero_a", TEXT + 0x20, TEXT + 0x30),
            ("zero_b", TEXT + 0x30, TEXT + 0x40),
        ]
    );
}

#[test]
fn an_interior_auto_label_is_not_a_function() {
    let words = [0u32; 16];
    let bytes = common::text_only_elf(&words, &[("main", TEXT, 0x20)]);
    let elf = ElfFile::parse(&bytes).unwrap();

    let extras = [function("sub_00100010", TEXT + 0x10, 0), function("sub_00100030", TEXT + 0x30, 0)];
    let functions = extract_functions(&elf, &extras, None);
    assert_eq!(
        summary(&functions),
        [
            ("main", TEXT, TEXT + 0x20),
            // 0x0010_0010 is inside main (0x0010_0000..0x0010_0020) and has no
            // authoritative range of its own, so it is dropped; 0x0010_0030 is
            // outside and survives.
            ("sub_00100030", TEXT + 0x30, TEXT + 0x40),
        ]
    );
}

#[test]
fn auto_names_are_upgraded_and_named_names_are_kept() {
    let words = [0u32; 16];

    let bytes = common::text_only_elf(&words, &[("sub_00100000", TEXT, 0)]);
    let elf = ElfFile::parse(&bytes).unwrap();
    let functions = extract_functions(&elf, &[function("entry", TEXT, TEXT + 0x20)], None);
    assert_eq!(summary(&functions), [("entry", TEXT, TEXT + 0x20)]);

    let bytes = common::text_only_elf(&words, &[("main", TEXT, 0x20)]);
    let elf = ElfFile::parse(&bytes).unwrap();
    let functions = extract_functions(&elf, &[function("other", TEXT, TEXT + 0x20)], None);
    assert_eq!(summary(&functions), [("main", TEXT, TEXT + 0x20)], "a named symbol keeps its name");
}

#[test]
fn a_map_removes_the_authoritative_end_of_auto_symbols_it_does_not_list() {
    let words = [0u32; 16];
    let bytes = common::text_only_elf(&words, &[("sub_00100000", TEXT, 0), ("sub_00100020", TEXT + 0x20, 0)]);
    let elf = ElfFile::parse(&bytes).unwrap();

    let csv = "Name,Start,End,Size\nreal_main,0x00100000,0x00100010,16\n";
    let map = FunctionMap::from_ghidra_csv(csv, &elf);
    assert_eq!(map.len(), 1);

    let functions = extract_functions_with_map(&elf, &map);
    assert_eq!(
        summary(&functions),
        [
            ("real_main", TEXT, TEXT + 0x10),
            // The map only denies the authoritative end of an unlisted auto
            // symbol; the symbol itself still becomes a function and its end is
            // inferred from the section.
            ("sub_00100020", TEXT + 0x20, TEXT + 0x40),
        ]
    );

    // The map's own entry, when passed as an extra by hand, is the same thing.
    let extras = extras_from_map(&map);
    assert_eq!(extras, [function("real_main", TEXT, TEXT + 0x10)]);
}

#[test]
fn a_map_drops_auto_symbols_that_start_inside_another_function() {
    let words = [0u32; 16];
    let bytes = common::text_only_elf(
        &words,
        &[("sub_00100000", TEXT, 0), ("sub_00100010", TEXT + 0x10, 0), ("sub_00100030", TEXT + 0x30, 0)],
    );
    let elf = ElfFile::parse(&bytes).unwrap();

    let csv = "Name,Start,End,Size\nreal_main,0x00100000,0x00100020,32\n";
    let map = FunctionMap::from_ghidra_csv(csv, &elf);
    let functions = extract_functions_with_map(&elf, &map);

    assert_eq!(
        summary(&functions),
        [
            ("real_main", TEXT, TEXT + 0x20),
            // 0x0010_0010 is an interior label of real_main and has no
            // authoritative range of its own: dropped. 0x0010_0030 is outside
            // and survives.
            ("sub_00100030", TEXT + 0x30, TEXT + 0x40),
        ]
    );
}

#[test]
fn authoritative_ends_are_clamped_to_the_section() {
    let words = [0u32; 16]; // ends at 0x0010_0040
    let bytes = common::text_only_elf(&words, &[("long", TEXT + 0x30, 0x100)]);
    let elf = ElfFile::parse(&bytes).unwrap();

    let functions = extract_functions(&elf, &[], None);
    assert_eq!(summary(&functions), [("long", TEXT + 0x30, TEXT + 0x40)]);
}

#[test]
fn an_empty_map_behaves_like_no_map() {
    let words = [0u32; 16];
    let bytes = common::text_only_elf(&words, &[("sub_00100000", TEXT, 0)]);
    let elf = ElfFile::parse(&bytes).unwrap();
    let map = FunctionMap::default();

    // Without a loaded map the auto symbol is kept rather than filtered.
    let functions = extract_functions_with_map(&elf, &map);
    assert_eq!(summary(&functions), [("sub_00100000", TEXT, TEXT + 0x40)]);
}
