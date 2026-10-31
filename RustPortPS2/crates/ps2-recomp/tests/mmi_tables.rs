//! The MMI0/MMI1 tables.

use ps2_r5900::opcodes::*;
use ps2_r5900::Instruction;
use ps2_recomp::translate::instruction::FamilyTranslator;
use ps2_recomp::translate::mmi_tables::{mmi0, mmi1};

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

fn packed(function: u32, rs: u32, rt: u32, rd: u32, sa: u32) -> Instruction {
    let raw = (OPCODE_MMI << 26) | (rs << 21) | (rt << 16) | (rd << 11) | (sa << 6) | function;
    ps2_r5900::decode(0x0010_0000, raw, false)
}

#[test]
fn the_mmi0_runtime_helpers_and_intrinsics_are_chosen_by_sub_opcode() {
    let inst = packed(MMI_MMI0, 8, 5, 2, MMI0_PADDW);
    assert_eq!(mmi0(&mut Hooks, &inst), "SET_GPR_VEC(ctx, 2, PS2_PADDW(GPR_VEC(ctx, 8), GPR_VEC(ctx, 5)));");

    let inst = packed(MMI_MMI0, 8, 5, 2, MMI0_PADDSH);
    assert_eq!(mmi0(&mut Hooks, &inst), "SET_GPR_VEC(ctx, 2, _mm_adds_epi16(GPR_VEC(ctx, 8), GPR_VEC(ctx, 5)));");
}

#[test]
fn pext5_and_ppac5_have_their_own_text() {
    let inst = packed(MMI_MMI0, 8, 5, 2, MMI0_PEXT5);
    let text = mmi0(&mut Hooks, &inst);
    assert!(text.starts_with("{ __m128i rt = GPR_VEC(ctx, 5); \n"), "{text}");
    assert!(text.contains("_mm_slli_epi32(_mm_and_si128(rt, m1), 3);"), "{text}");
    assert!(
        text.ends_with("SET_GPR_VEC(ctx, 2, _mm_or_si128(_mm_or_si128(a1, a2), _mm_or_si128(a3, a4))); }"),
        "{text}"
    );

    let inst = packed(MMI_MMI0, 8, 5, 2, MMI0_PPAC5);
    assert!(mmi0(&mut Hooks, &inst).contains("_mm_srli_epi32(rt, 3)"), "{}", mmi0(&mut Hooks, &inst));
}

#[test]
fn the_absolute_value_reads_rt_like_the_gow_port() {
    let inst = packed(MMI_MMI1, 8, 5, 2, MMI1_PABSW);
    assert_eq!(mmi1(&mut Hooks, &inst), "SET_GPR_VEC(ctx, 2, PS2_PABSW(GPR_VEC(ctx, 5)));");
    let inst = packed(MMI_MMI1, 8, 5, 2, MMI1_PABSH);
    assert_eq!(mmi1(&mut Hooks, &inst), "SET_GPR_VEC(ctx, 2, PS2_PABSH(GPR_VEC(ctx, 5)));");
}

#[test]
fn the_unsigned_saturating_forms_use_the_unsigned_intrinsics() {
    let inst = packed(MMI_MMI1, 8, 5, 2, MMI1_PADDUH);
    assert_eq!(mmi1(&mut Hooks, &inst), "SET_GPR_VEC(ctx, 2, _mm_adds_epu16(GPR_VEC(ctx, 8), GPR_VEC(ctx, 5)));");
    let inst = packed(MMI_MMI1, 8, 5, 2, MMI1_PADDUB);
    assert_eq!(mmi1(&mut Hooks, &inst), "SET_GPR_VEC(ctx, 2, _mm_adds_epu8(GPR_VEC(ctx, 8), GPR_VEC(ctx, 5)));");
}

#[test]
fn padsbh_and_qfsrv_have_their_own_text() {
    let inst = packed(MMI_MMI1, 8, 5, 2, MMI1_PADSBH);
    let text = mmi1(&mut Hooks, &inst);
    assert!(text.contains("_mm_unpacklo_epi64(sub, _mm_unpackhi_epi64(add, add))"), "{text}");

    let inst = packed(MMI_MMI1, 8, 5, 2, MMI1_QFSRV);
    assert_eq!(
        mmi1(&mut Hooks, &inst),
        "SET_GPR_VEC(ctx, 2, PS2_QFSRV(GPR_VEC(ctx, 8), GPR_VEC(ctx, 5), ctx->sa & 0x7F));"
    );
}

#[test]
fn unknown_sub_opcodes_are_reported() {
    let inst = packed(MMI_MMI0, 8, 5, 2, 0x1C);
    assert_eq!(mmi0(&mut Hooks, &inst), "<unhandled: Unhandled MMI0 instruction: function 0x1C>");
    let inst = packed(MMI_MMI1, 8, 5, 2, 0x1C);
    assert_eq!(mmi1(&mut Hooks, &inst), "<unhandled: Unhandled MMI1 instruction: function 0x1C>");
}
