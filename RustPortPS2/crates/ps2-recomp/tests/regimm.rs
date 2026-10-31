//! The `REGIMM` templates.

use ps2_r5900::opcodes::*;
use ps2_r5900::Instruction;
use ps2_recomp::translate::instruction::{translate as translate_generic, FamilyTranslator};
use ps2_recomp::translate::memory::MemoryAccessHint;
use ps2_recomp::translate::regimm::translate;

struct Hooks;

impl FamilyTranslator for Hooks {
    fn translate_special(&mut self, _: &Instruction) -> String {
        String::new()
    }
    fn translate_regimm(&mut self, inst: &Instruction) -> String {
        translate(self, inst)
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

fn regimm(rt: u32, rs: u32, immediate: u32) -> String {
    let raw = (OPCODE_REGIMM << 26) | (rs << 21) | (rt << 16) | immediate;
    translate_generic(&mut Hooks, &ps2_r5900::decode(0x0010_0000, raw, false), MemoryAccessHint::default())
}

#[test]
fn a_linked_branch_is_only_a_comment_with_the_computed_target() {
    // bgezal $t0, +8: 0x0010_0000 + 4 + 8.
    assert_eq!(regimm(REGIMM_BGEZAL, 8, 2), "// REGIMM branch instruction to 0x10000C - Handled by branch logic");
    // bltz $t0, -2 words: 0x0010_0000 + 4 - 8.
    assert_eq!(regimm(REGIMM_BLTZ, 8, 0xFFFE), "// REGIMM branch instruction to 0xFFFFC - Handled by branch logic");
}

#[test]
fn mtsab_and_mtsah_rotate_the_sa_field() {
    assert_eq!(regimm(REGIMM_MTSAB, 4, 0x3), "ctx->sa = ((GPR_U32(ctx, 4) ^ (uint32_t)3) & 0xF) << 3;");
    assert_eq!(regimm(REGIMM_MTSAH, 4, 0x5), "ctx->sa = ((GPR_U32(ctx, 4) ^ (uint32_t)5) & 0x7) << 4;");
}

#[test]
fn the_immediate_traps_compare_the_sign_extended_value() {
    assert_eq!(
        regimm(REGIMM_TGEI, 4, 0xFFFF),
        "if (GPR_S64(ctx, 4) >= (int64_t)(int32_t)4294967295) { runtime->handleTrap(rdram, ctx); }"
    );
}

#[test]
fn an_unknown_regimm_rt_is_reported() {
    assert_eq!(regimm(0x5, 4, 0), "<unhandled: Unhandled REGIMM instruction: 0x5>");
}
