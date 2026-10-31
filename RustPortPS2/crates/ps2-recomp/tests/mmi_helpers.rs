//! The packed HI/LO moves.

use ps2_r5900::opcodes::*;
use ps2_r5900::Instruction;
use ps2_recomp::translate::instruction::FamilyTranslator;
use ps2_recomp::translate::mmi_helpers::{pmfhl, pmthl};

struct Hooks;

impl FamilyTranslator for Hooks {
    fn translate_special(&mut self, _: &Instruction) -> String {
        String::new()
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

fn packed_move(function: u32, rs: u32, rd: u32, sa: u32) -> (String, String) {
    let raw = (OPCODE_MMI << 26) | (rs << 21) | (rd << 11) | (sa << 6) | function;
    let inst = ps2_r5900::decode(0x0010_0000, raw, false);
    (pmfhl(&mut Hooks, &inst), pmthl(&mut Hooks, &inst))
}

#[test]
fn every_pmfhl_variation_maps_to_its_runtime_helper() {
    for (sa, helper) in [
        (PMFHL_LW, "PS2_PMFHL_LW"),
        (PMFHL_UW, "PS2_PMFHL_UW"),
        (PMFHL_SLW, "PS2_PMFHL_SLW"),
        (PMFHL_LH, "PS2_PMFHL_LH"),
        (PMFHL_SH, "PS2_PMFHL_SH"),
    ] {
        let (text, _) = packed_move(MMI_PMFHL, 0, 2, sa);
        assert_eq!(text, format!("SET_GPR_VEC(ctx, 2, {helper}(ctx->hi, ctx->lo));"), "sa {sa}");
    }
}

#[test]
fn pmthl_reads_the_two_low_lanes_of_rs() {
    let (_, text) = packed_move(MMI_PMTHL, 8, 2, PMFHL_LW);
    assert_eq!(
        text,
        "{ __m128i val = GPR_VEC(ctx, 8); ctx->lo = _mm_extract_epi32(val, 0); \
         ctx->hi = _mm_extract_epi32(val, 1); }"
    );
}

#[test]
fn unknown_sub_functions_are_reported() {
    let (pmfhl_text, pmthl_text) = packed_move(MMI_PMFHL, 0, 2, 0x1F);
    assert_eq!(pmfhl_text, "<unhandled: Unhandled PMFHL instruction: function 0x1F>");
    assert_eq!(pmthl_text, "<unhandled: Unhandled PMTHL instruction: function 0x1F>");
}
