use std::collections::HashMap;

use ps2_r5900::{decode, Instruction};
use ps2_recomp::flow::analyzer::ControlFlowAnalyzer;
use ps2_recomp::reporter::Reporter;
use ps2_recomp::types::{Function, Section};

const BASE: u32 = 0x0010_0000;

fn decode_all(base: u32, words: &[u32]) -> Vec<Instruction> {
    words.iter().enumerate().map(|(index, raw)| decode(base + index as u32 * 4, *raw, false)).collect()
}

fn function(name: &str, start: u32, words: usize) -> Function {
    Function { name: name.into(), start, end: start + words as u32 * 4, is_recompiled: true, ..Function::default() }
}

fn code_section(words: usize) -> Section {
    Section {
        name: ".text".into(),
        address: BASE,
        size: words as u32 * 4,
        is_code: true,
        is_data: false,
        is_bss: false,
        is_read_only: false,
        data: None,
    }
}

const NOP: u32 = 0;
const JR_RA: u32 = 0x03E0_0008;

fn analyze(
    words: &[u32],
    extra: &[Function],
    sections: &[Section],
    tables: &HashMap<u32, Vec<u32>>,
) -> ps2_recomp::flow::analyzer::AnalysisResult {
    let instructions = decode_all(BASE, words);
    let function = function("sub_00100000", BASE, words.len());
    let reporter = Reporter::new();
    let analyzer = ControlFlowAnalyzer::new(sections, tables, Some(&reporter));
    let mut all = vec![function.clone()];
    all.extend_from_slice(extra);
    analyzer.analyze(&function, &instructions, Some(&all))
}

#[test]
fn a_backward_branch_is_a_label_and_a_loop_resume_point() {
    // 0: addiu t0, t0, -1 ; 4: bne t0, zero, 0 ; 8: nop ; c: jr ra ; 10: nop
    let words = [0x2508_FFFF, 0x1500_FFFE, NOP, JR_RA, NOP];
    let result = analyze(&words, &[], &[code_section(words.len())], &HashMap::new());
    assert!(result.entry_points.contains(&BASE), "the loop head is a label");
    assert!(!result.resume_entry_points.contains(&BASE), "the function start is never a resume point");
    assert!(result.jump_table_targets.is_empty());
}

#[test]
fn a_jal_marks_the_return_address_as_a_resume_point() {
    // 0: jal 0x00100020 ; 4: nop ; 8: addiu v0, zero, 1 (resume) ; c: jr ra ; 10: nop
    let words = [0x0C00_0008, NOP, 0x2402_0001, JR_RA, NOP];
    let result = analyze(&words, &[], &[code_section(words.len())], &HashMap::new());
    assert!(result.resume_entry_points.contains(&(BASE + 8)));
    assert!(result.entry_points.contains(&(BASE + 8)));
}

#[test]
fn a_syscall_marks_the_next_instruction_as_a_resume_point() {
    // 0: syscall ; 4: jr ra ; 8: nop
    let words = [0x0000_000C, JR_RA, NOP];
    let result = analyze(&words, &[], &[code_section(words.len())], &HashMap::new());
    assert!(result.resume_entry_points.contains(&(BASE + 4)), "syscall has no delay slot: +4, not +8");
}

#[test]
fn a_branch_into_another_function_is_an_external_entry_point() {
    let other = function("sub_00100040", BASE + 0x40, 4);
    // 0: beq zero, zero, +0x20 (lands at BASE + 0x24: inside `other`) ; 4: nop ; 8: jr ra ; c: nop
    let words = [0x1000_0008, NOP, JR_RA, NOP];
    let mut other = other;
    other.start = BASE + 0x20;
    other.end = BASE + 0x40;
    let sections = [Section { size: 0x100, ..code_section(0) }];
    let result = analyze(&words, &[other], &sections, &HashMap::new());
    assert!(result.external_entry_points.contains(&(BASE + 0x24)));
}

#[test]
fn an_unresolved_register_jump_promotes_every_instruction_to_a_fallback_entry() {
    // 0: jr t0 ; 4: nop ; 8: jr ra ; c: nop
    let words = [0x0100_0008, NOP, JR_RA, NOP];
    let instructions = decode_all(BASE, &words);
    let function = function("sub_00100000", BASE, words.len());
    let reporter = Reporter::new();
    let tables = HashMap::new();
    let sections = [code_section(words.len())];
    let result = ControlFlowAnalyzer::new(&sections, &tables, Some(&reporter)).analyze(&function, &instructions, None);

    assert_eq!(result.indirect_fallback_entry_points.len(), 4);
    assert_eq!(result.entry_points.len(), 4);
    assert_eq!(reporter.counters().indirect_fallback_promotions, 1);
    assert_eq!(reporter.counters().indirect_fallback_entries, 4);
    assert!(reporter.summary().contains("unresolved JR/JALR at 0x100000; promoted 4 fallback entries"));
}

#[test]
fn a_bounded_jump_table_in_rodata_resolves_its_targets() {
    let rodata_address = 0x0020_0000u32;
    let cases = [BASE + 0x30, BASE + 0x34, BASE + 0x38];
    let data: Vec<u8> = cases.iter().flat_map(|case| case.to_le_bytes()).collect();
    let rodata = Section {
        name: ".rodata".into(),
        address: rodata_address,
        size: data.len() as u32,
        is_code: false,
        is_data: true,
        is_bss: false,
        is_read_only: true,
        data: Some(data),
    };

    // sltiu v0, a0, 3 ; nop ; lui v1, 0x20 ; addiu v1, v1, 0 ; sll a0, a0, 2 ; addu v1, v1, a0 ;
    // lw v0, 0(v1) ; nop ; jr v0 ; nop ; ... padding up to the three cases ; jr ra ; nop
    let mut words =
        vec![0x2C82_0003, NOP, 0x3C03_0020, 0x2463_0000, 0x0004_2080, 0x0064_1821, 0x8C62_0000, NOP, 0x0040_0008, NOP];
    while words.len() < 0x30 / 4 {
        words.push(NOP);
    }
    words.extend([JR_RA, JR_RA, JR_RA, NOP]);

    let sections = [Section { size: 0x1000, ..code_section(0) }, rodata];
    let result = analyze(&words, &[], &sections, &HashMap::new());
    let targets = result.jump_table_targets.get(&(BASE + 0x20)).expect("the jr at +0x20 is a jump table");
    assert_eq!(targets, &cases.to_vec());
    for case in cases {
        assert!(result.entry_points.contains(&case));
    }
}

#[test]
fn a_configured_jump_table_wins_over_the_heuristics() {
    let table_address = 0x0020_0000u32;
    let mut tables = HashMap::new();
    tables.insert(table_address, vec![BASE + 0x30, BASE + 0x30, BASE + 0x34]);

    let mut words =
        vec![0x2C82_0003, NOP, 0x3C03_0020, 0x2463_0000, 0x0004_2080, 0x0064_1821, 0x8C62_0000, NOP, 0x0040_0008, NOP];
    while words.len() < 0x38 / 4 {
        words.push(NOP);
    }
    words.extend([JR_RA, NOP]);

    let sections = [Section { size: 0x1000, ..code_section(0) }];
    let result = analyze(&words, &[], &sections, &tables);
    assert_eq!(result.jump_table_targets.get(&(BASE + 0x20)), Some(&vec![BASE + 0x30, BASE + 0x34]));
}
