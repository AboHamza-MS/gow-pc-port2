use std::sync::Arc;

use ps2_recomp::calls::{resolve_stub_name, resolve_stub_target, resolve_syscall_name, StubTarget};
use ps2_recomp::generator::{sanitize_identifier_body, CodeGenerator};
use ps2_recomp::recompiler::clamp_filename_length;
use ps2_recomp::types::Symbol;

fn symbol(name: &str, address: u32, is_function: bool) -> Symbol {
    Symbol { name: name.into(), address, size: 0, is_function, is_imported: false, is_exported: false }
}

#[test]
fn identifiers_are_made_safe() {
    assert_eq!(sanitize_identifier_body("foo::bar<int>"), "foo__bar_int_");
    assert_eq!(sanitize_identifier_body("9lives"), "_9lives");
    assert_eq!(sanitize_identifier_body(""), "");
    assert_eq!(sanitize_identifier_body("sub_001"), "sub_001");
}

#[test]
fn function_names_avoid_keywords_and_reserved_identifiers() {
    assert_eq!(CodeGenerator::sanitize_function_name("main"), "ps2_main");
    assert_eq!(CodeGenerator::sanitize_function_name("delete"), "ps2_delete");
    assert_eq!(CodeGenerator::sanitize_function_name("_start"), "ps2_start");
    assert_eq!(CodeGenerator::sanitize_function_name("__ct__Foo"), "ps2__ct__Foo");
    assert_eq!(CodeGenerator::sanitize_function_name("plain"), "plain");
    assert_eq!(CodeGenerator::sanitize_function_name(""), "");
}

#[test]
fn a_rename_wins_over_the_symbol_and_only_functions_count() {
    let symbols = [symbol("sceCdRead", 0x100, true), symbol("some_data", 0x200, false)];
    let mut generator = CodeGenerator::new(&symbols, Arc::new(Vec::new()));
    assert_eq!(generator.get_function_name(0x100), "sceCdRead");
    assert_eq!(generator.get_function_name(0x200), "", "a data symbol is not a function");
    assert_eq!(generator.get_function_name(0x300), "");

    generator.set_renamed_functions([(0x100, "sceCdRead_0x100".to_owned())].into_iter().collect());
    assert_eq!(generator.get_function_name(0x100), "sceCdRead_0x100");
}

#[test]
fn the_first_symbol_at_an_address_is_kept() {
    let symbols = [symbol("first", 0x100, true), symbol("second", 0x100, true)];
    let generator = CodeGenerator::new(&symbols, Arc::new(Vec::new()));
    assert_eq!(generator.get_function_name(0x100), "first");
}

#[test]
fn runtime_names_resolve_with_an_optional_leading_underscore() {
    assert_eq!(resolve_syscall_name("FlushCache"), Some("FlushCache"));
    assert_eq!(resolve_syscall_name("nonexistent_function"), None);
    assert_eq!(resolve_syscall_name(""), None);
    assert_eq!(resolve_stub_target("FlushCache"), StubTarget::Syscall);
    assert_eq!(resolve_stub_target("definitely_not_a_handler"), StubTarget::Unknown);
    // The lookup tolerates one underscore on either side.
    assert_eq!(resolve_syscall_name("_FlushCache"), Some("FlushCache"));
    if let Some(stub) = resolve_stub_name("memcpy") {
        assert_eq!(resolve_stub_name(&format!("_{stub}")), Some(stub));
    }
}

#[test]
fn long_file_names_keep_their_address_suffix() {
    assert_eq!(clamp_filename_length("short_0x100", ".cpp", 100), "short_0x100.cpp");
    let long = format!("{}_0x123456", "n".repeat(120));
    let clamped = clamp_filename_length(&long, ".cpp", 100);
    assert_eq!(clamped.len(), 100);
    assert!(clamped.ends_with("_0x123456.cpp"));
    assert_eq!(clamp_filename_length("abc", ".cpp", 0), "abc.cpp");
    // The C++ computes `max - ext - suffix` in `size_t`: it wraps instead of clamping to 0.
    assert_eq!(clamp_filename_length("anything_0xabcdef", ".cpp", 12), "anything_0xabcdef.cpp");
    assert_eq!(clamp_filename_length("anything_0xabcdef", ".cpp", 13), "_0xabcdef.cpp");
}
