//! The `SPECIAL` templates.

use ps2_r5900::opcodes::*;
use ps2_r5900::Instruction;
use ps2_recomp::translate::instruction::{translate as translate_generic, FamilyTranslator};
use ps2_recomp::translate::memory::MemoryAccessHint;
use ps2_recomp::translate::special::translate;

struct Hooks;

impl FamilyTranslator for Hooks {
    fn translate_special(&mut self, inst: &Instruction) -> String {
        translate(self, inst)
    }
    fn translate_regimm(&mut self, _: &Instruction) -> String {
        String::new()
    }
    fn translate_cop0(&mut self, _: &Instruction) -> String {
        String::new()
    }
    fn translate_fpu(&mut self, _: &Instruction) -> String {
        String::new()
    }
    fn translate_vu(&mut self, _: &Instruction) -> String {
        String::new()
    }
    fn translate_mmi(&mut self, _: &Instruction) -> String {
        String::new()
    }
    fn emit_unhandled(&mut self, _: &Instruction, reason: &str) -> String {
        format!("<unhandled: {reason}>")
    }
}

/// `OPCODE_SPECIAL | rs | rt | rd | sa | function`, built from the fields so the
/// test cannot get the bit positions wrong.
fn special_raw(function: u32, rs: u32, rt: u32, rd: u32, sa: u32) -> u32 {
    (OPCODE_SPECIAL << 26) | (rs << 21) | (rt << 16) | (rd << 11) | (sa << 6) | function
}

fn special(function: u32, rs: u32, rt: u32, rd: u32, sa: u32) -> String {
    let raw = special_raw(function, rs, rt, rd, sa);
    translate_generic(&mut Hooks, &ps2_r5900::decode(0x0010_0000, raw, false), MemoryAccessHint::default())
}

#[test]
fn sll_distinguishes_the_nop_from_an_empty_string() {
    assert_eq!(special(SPECIAL_SLL, 0, 0, 0, 0), "// NOP");
    // sll $zero, $t0, 4: the C++ emits nothing.
    assert_eq!(special(SPECIAL_SLL, 0, 8, 0, 4), "");
    // sll $v0, $t0, 4
    assert_eq!(special(SPECIAL_SLL, 0, 8, 2, 4), "SET_GPR_S32(ctx, 2, (int32_t)SLL32(GPR_U32(ctx, 8), 4));");
}

#[test]
fn addu_and_subu_use_the_runtime_macros() {
    assert_eq!(
        special(SPECIAL_ADDU, 8, 5, 2, 0),
        "SET_GPR_S32(ctx, 2, (int32_t)ADD32(GPR_U32(ctx, 8), GPR_U32(ctx, 5)));"
    );
    assert_eq!(
        special(SPECIAL_SUBU, 8, 5, 2, 0),
        "SET_GPR_S32(ctx, 2, (int32_t)SUB32(GPR_U32(ctx, 8), GPR_U32(ctx, 5)));"
    );
}

#[test]
fn syscall_sets_pc_and_passes_the_encoded_code() {
    // The code is the 20 bits above the function field: put 1 there.
    let raw = special_raw(SPECIAL_SYSCALL, 0, 0, 0, 0) | (1 << 6);
    assert_eq!(
        translate_generic(&mut Hooks, &ps2_r5900::decode(0x0010_0000, raw, false), MemoryAccessHint::default()),
        "ctx->pc = 0x100004u;\nruntime->handleSyscall(rdram, ctx, 0x1u);"
    );
}

#[test]
fn jr_and_jalr_are_left_to_the_branch_logic() {
    assert_eq!(special(SPECIAL_JR, 31, 0, 0, 0), "// JR $31 - Handled by branch logic");
    assert_eq!(special(SPECIAL_JALR, 8, 0, 31, 0), "// JALR $31, $8 - Handled by branch logic");
}

#[test]
fn mult_writes_rd_only_when_it_is_not_zero() {
    let without_rd = special(SPECIAL_MULT, 8, 5, 0, 0);
    assert!(without_rd.ends_with("(result >> 32); }"), "{without_rd}");
    assert!(!without_rd.contains("SET_GPR_S32"), "{without_rd}");

    let with_rd = special(SPECIAL_MULT, 8, 5, 2, 0);
    assert!(with_rd.contains("SET_GPR_S32(ctx, 2, (int32_t)result);"), "{with_rd}");
}

#[test]
fn movz_uses_the_vector_setter_like_the_cpp() {
    assert_eq!(special(SPECIAL_MOVZ, 8, 5, 2, 0), "if (GPR_U64(ctx, 5) == 0) SET_GPR_VEC(ctx, 2, GPR_VEC(ctx, 8));");
}

/// GOW-Port (decision D2): the same templates with `[codegen] movz_low64`.
struct GowHooks;

impl FamilyTranslator for GowHooks {
    fn codegen(&self) -> ps2_config::Codegen {
        ps2_config::Codegen::gow()
    }
    fn translate_special(&mut self, inst: &Instruction) -> String {
        translate(self, inst)
    }
    fn translate_regimm(&mut self, _: &Instruction) -> String {
        String::new()
    }
    fn translate_cop0(&mut self, _: &Instruction) -> String {
        String::new()
    }
    fn translate_fpu(&mut self, _: &Instruction) -> String {
        String::new()
    }
    fn translate_vu(&mut self, _: &Instruction) -> String {
        String::new()
    }
    fn translate_mmi(&mut self, _: &Instruction) -> String {
        String::new()
    }
    fn emit_unhandled(&mut self, _: &Instruction, reason: &str) -> String {
        format!("<unhandled: {reason}>")
    }
}

#[test]
fn movz_and_movn_move_only_the_low_64_bits_in_gow_mode() {
    let gow = |function: u32| {
        let raw = special_raw(function, 8, 5, 2, 0);
        translate_generic(&mut GowHooks, &ps2_r5900::decode(0x0010_0000, raw, false), MemoryAccessHint::default())
    };
    assert_eq!(gow(SPECIAL_MOVZ), "if (GPR_U64(ctx, 5) == 0) SET_GPR_U64(ctx, 2, GPR_U64(ctx, 8));");
    assert_eq!(gow(SPECIAL_MOVN), "if (GPR_U64(ctx, 5) != 0) SET_GPR_U64(ctx, 2, GPR_U64(ctx, 8));");
    // compat keeps PS2Recomp's 128-bit copy for MOVN too
    assert_eq!(special(SPECIAL_MOVN, 8, 5, 2, 0), "if (GPR_U64(ctx, 5) != 0) SET_GPR_VEC(ctx, 2, GPR_VEC(ctx, 8));");
}

#[test]
fn an_unknown_special_function_is_reported() {
    // 0x3D is unassigned (0x3C and 0x3E are the DSLL32/DSRL32 pair, 0x3F DSRA32).
    assert_eq!(special(0x3D, 8, 5, 2, 0), "<unhandled: Unhandled SPECIAL instruction: 0x3D>");
}
