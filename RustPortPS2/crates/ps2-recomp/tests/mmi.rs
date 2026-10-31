//! The `MMI` templates.

use ps2_r5900::opcodes::*;
use ps2_r5900::Instruction;
use ps2_recomp::translate::instruction::{translate as translate_generic, FamilyTranslator};
use ps2_recomp::translate::memory::MemoryAccessHint;
use ps2_recomp::translate::mmi::translate;

#[derive(Default)]
struct Hooks {
    calls: Vec<String>,
}

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
    fn translate_mmi(&mut self, inst: &Instruction) -> String {
        translate(self, inst)
    }
    fn emit_unhandled(&mut self, _: &Instruction, reason: &str) -> String {
        format!("<unhandled: {reason}>")
    }
    fn translate_pmfhl(&mut self, _: &Instruction) -> String {
        self.calls.push("pmfhl".to_owned());
        "<pmfhl>".to_owned()
    }
}

fn mmi(function: u32, rs: u32, rt: u32, rd: u32, sa: u32) -> String {
    let raw = (OPCODE_MMI << 26) | (rs << 21) | (rt << 16) | (rd << 11) | (sa << 6) | function;
    translate_generic(&mut Hooks::default(), &ps2_r5900::decode(0x0010_0000, raw, false), MemoryAccessHint::default())
}

#[test]
fn hi1_lo1_moves_use_the_second_accumulator() {
    assert_eq!(mmi(MMI_MFHI1, 0, 0, 2, 0), "SET_GPR_U64(ctx, 2, ctx->hi1);");
    assert_eq!(mmi(MMI_MTHI1, 8, 0, 0, 0), "ctx->hi1 = GPR_U64(ctx, 8);");
    assert_eq!(mmi(MMI_MFLO1, 0, 0, 2, 0), "SET_GPR_U64(ctx, 2, ctx->lo1);");
    assert_eq!(mmi(MMI_MTLO1, 8, 0, 0, 0), "ctx->lo1 = GPR_U64(ctx, 8);");
}

#[test]
fn mult1_writes_rd_only_when_it_is_not_zero() {
    let without = mmi(MMI_MULT1, 8, 5, 0, 0);
    assert!(without.ends_with("(result >> 32); }"), "{without}");
    assert!(!without.contains("SET_GPR_S32"), "{without}");

    let with = mmi(MMI_MULT1, 8, 5, 2, 0);
    assert!(with.contains("SET_GPR_S32(ctx, 2, (int32_t)result);"), "{with}");
}

#[test]
fn divu1_keeps_the_cpp_spacing_quirk() {
    let text = mmi(MMI_DIVU1, 8, 5, 0, 0);
    assert!(text.contains("ctx->lo1=0xFFFFFFFFFFFFFFFFull;"), "{text}");
    assert!(!text.contains("ctx->lo1 = 0xFFFF"), "{text}");
}

#[test]
fn madd_and_madd1_use_their_own_accumulator_pair() {
    let madd = mmi(MMI_MADD, 8, 5, 0, 0);
    assert!(madd.contains("Ps2HiLoToU64(ctx->hi, ctx->lo)"), "{madd}");
    assert!(madd.contains("ctx->hi = Ps2SignExt32ToU64"), "{madd}");

    let madd1 = mmi(MMI_MADD1, 8, 5, 0, 0);
    assert!(madd1.contains("Ps2HiLoToU64(ctx->hi1, ctx->lo1)"), "{madd1}");
}

#[test]
fn plzcw_counts_both_halves() {
    let text = mmi(MMI_PLZCW, 8, 0, 2, 0);
    assert!(text.contains("uint64_t out = ((uint64_t)ps2_plzcw32(hi) << 32) | (uint64_t)ps2_plzcw32(lo);"), "{text}");
    assert!(text.contains("SET_GPR_U64(ctx, 2, out);"), "{text}");
}

#[test]
fn packed_shifts_use_the_sse_intrinsics() {
    assert_eq!(mmi(MMI_PSLLH, 0, 5, 2, 3), "SET_GPR_VEC(ctx, 2, _mm_slli_epi16(GPR_VEC(ctx, 5), 3));");
    assert_eq!(mmi(MMI_PSRAW, 0, 5, 2, 1), "SET_GPR_VEC(ctx, 2, _mm_srai_epi32(GPR_VEC(ctx, 5), 1));");
}

#[test]
fn the_sub_families_are_delegated() {
    assert_eq!(mmi(MMI_PMFHL, 0, 0, 2, 0), "<pmfhl>");
    assert_eq!(mmi(MMI_MMI0, 0, 0, 2, 0), "<unhandled: MMI0 instruction>");
}
