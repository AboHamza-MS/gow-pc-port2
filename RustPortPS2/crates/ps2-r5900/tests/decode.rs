//! Semantic decode tests: the flags the translators depend on.

use ps2_r5900::opcodes::*;
use ps2_r5900::{decode, Instruction};

const ADDRESS: u32 = 0x0010_0000;

fn at(raw: u32) -> Instruction {
    decode(ADDRESS, raw, false)
}

#[test]
fn addiu_writes_a_gpr_and_has_no_delay_slot() {
    let inst = at(0x27BD_FFE8);
    assert_eq!(inst.opcode, OPCODE_ADDIU);
    assert!(inst.modification.modifies_gpr);
    assert!(!inst.is_branch && !inst.is_jump && !inst.is_call);
    assert!(!inst.has_delay_slot);
    assert!(!inst.modification.modifies_control);
}

#[test]
fn jr_ra_is_a_return_with_a_delay_slot() {
    let inst = at(0x03E0_0008);
    assert!(inst.is_jump && inst.is_return && inst.has_delay_slot);
    assert!(!inst.modification.modifies_gpr, "jr does not write a GPR");
    assert!(inst.modification.modifies_control);
    assert_eq!(inst.jump_target(), 0, "the target lives in a register");
}

#[test]
fn jal_is_a_call_and_the_target_comes_from_the_instruction() {
    // jal 0x0010_0010: the 26-bit field holds the target shifted right by 2.
    let inst = at(0x0C04_0004);
    assert!(inst.is_call && inst.is_jump && inst.has_delay_slot);
    assert!(inst.modification.modifies_gpr, "$ra is written");
    assert_eq!(inst.jump_target(), 0x0010_0010);
}

#[test]
fn branch_targets_are_pc_relative() {
    // beq $t0, $t1, +8
    let beq = at(0x1109_0002);
    assert!(beq.is_branch && beq.has_delay_slot);
    assert!(!beq.is_call);
    assert_eq!(beq.branch_target(), ADDRESS + 4 + 8);

    // bgezal $t0, +8: Rabbitizer already reports the linked branch as a call.
    let bgezal = at(0x0511_0002);
    assert!(bgezal.is_branch && bgezal.is_call);
    assert_eq!(bgezal.branch_target(), ADDRESS + 4 + 8);
}

#[test]
fn eret_is_a_return_without_a_delay_slot() {
    let inst = at(0x4200_0018);
    assert!(inst.is_return);
    assert!(!inst.has_delay_slot);
    assert!(inst.modification.modifies_control);
}

#[test]
fn syscall_changes_control_but_no_gpr() {
    let inst = at(0x0000_000C);
    assert!(inst.modification.modifies_control);
    assert!(!inst.modification.modifies_gpr);
    assert!(!inst.is_call);
}

#[test]
fn mult_writes_hi_lo_and_only_rd_when_asked() {
    // mult $a0, $a1 with rd = $zero.
    let inst = at(0x0085_0018);
    assert!(inst.modification.modifies_control, "HI/LO");
    assert!(!inst.modification.modifies_gpr);

    // Same with rd = $v0 (the R5900 three-operand form).
    let with_rd = at((SPECIAL_MULT << 6) | (2 << 11) | (4 << 21) | (5 << 16));
    assert!(with_rd.modification.modifies_gpr);
}

#[test]
fn quadword_loads_and_stores_are_multimedia() {
    let lq = at(0x7888_0000);
    assert_eq!(lq.opcode, OPCODE_LQ);
    assert!(lq.is_multimedia && lq.is_load);

    let sq = at(0x7C88_0000);
    assert_eq!(sq.opcode, OPCODE_SQ);
    assert!(sq.is_multimedia && sq.is_store);
}

#[test]
fn mmi_sets_the_family_and_the_sub_opcode() {
    // paddw $v0, $a0, $a1: the MMI *group* is the function field, the sub-opcode
    // travels in `sa` (here PADDW, i.e. 0).
    let raw = (OPCODE_MMI << 26) | (4 << 21) | (5 << 16) | (2 << 11) | MMI_MMI0;
    let inst = at(raw);
    assert!(inst.is_mmi && inst.is_multimedia);
    assert_eq!(inst.mmi_type, 0, "MMI0");
    assert_eq!(inst.mmi_function, 0, "PADDW");
    assert_eq!(inst.mmi_function, inst.sa as u8);

    // pmfhl.slw: the variation travels in `sa`.
    let raw = (OPCODE_MMI << 26) | (2 << 11) | (PMFHL_SLW << 6) | MMI_PMFHL;
    let inst = at(raw);
    assert_eq!(inst.pmfhl_variation, PMFHL_SLW as u8);
}

#[test]
fn cop2_transfers_touch_the_right_side() {
    // qmfc2 $v0, $vi0: format QMFC2, rt != 0.
    let raw = (OPCODE_COP2 << 26) | (COP2_QMFC2 << 21) | (2 << 16);
    let inst = at(raw);
    assert!(inst.is_vu && inst.is_multimedia);
    assert!(inst.modification.modifies_gpr);
    assert!(!inst.modification.modifies_vfr);

    // qmtc2: writes vector state instead.
    let raw = (OPCODE_COP2 << 26) | (COP2_QMTC2 << 21) | (2 << 16);
    let inst = at(raw);
    assert!(inst.modification.modifies_vfr);
}

#[test]
fn a_vu_macro_reads_the_field_mask_from_the_instruction() {
    let raw = 0x4A02_09C0; // vaddx $vf7, $vf1, $vf2x
    let inst = at(raw);
    assert!(inst.is_vu && inst.is_multimedia && inst.vector.is_vector);
    assert_eq!(inst.vector.vector_field, ((raw >> 21) & 0xF) as u8);
    assert!(inst.modification.modifies_vfr);
    assert!(inst.modification.modifies_control);
}

#[test]
fn the_disassembly_is_only_filled_when_requested() {
    let lazy = decode(ADDRESS, 0x27BD_FFE8, false);
    assert!(lazy.disassembly.is_empty());
    let full = decode(ADDRESS, 0x27BD_FFE8, true);
    assert_eq!(full.disassembly, "addiu       $sp, $sp, -0x18");
}

#[test]
fn fields_only_construction_leaves_the_semantic_flags_clear() {
    let inst = Instruction::new(ADDRESS, 0x0C00_0004, false);
    assert_eq!(inst.opcode, OPCODE_JAL);
    assert!(!inst.is_call, "Instruction::new only extracts the fields");
    assert!(!inst.modification.modifies_gpr);
}
