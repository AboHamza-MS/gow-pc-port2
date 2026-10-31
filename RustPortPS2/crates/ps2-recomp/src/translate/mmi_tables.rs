//! MMI0 and MMI1 (`mmi_translation_helpers.cpp`, first half).
//!
//! Both tables are almost entirely mechanical: a sub-opcode maps to either a
//! `PS2_*` runtime helper or an SSE intrinsic, always writing `rd` from `rs`/`rt`
//! (except `PABSW`/`PABSH`, which the GOW port reads from `rt` only). Three
//! operations (and `PADSBH`/`QFSRV`) have their own text.

use ps2_r5900::opcodes::*;
use ps2_r5900::Instruction;

use crate::translate::instruction::FamilyTranslator;

/// `SET_GPR_VEC(ctx, rd, <expr>(GPR_VEC(ctx, rs), GPR_VEC(ctx, rt)));`
fn binary(rd: u32, rs: u32, rt: u32, expression: &str) -> String {
    format!("SET_GPR_VEC(ctx, {rd}, {expression}(GPR_VEC(ctx, {rs}), GPR_VEC(ctx, {rt})));")
}

/// `SET_GPR_VEC(ctx, rd, <expr>(GPR_VEC(ctx, rt)));` (the unary `PABS*` form).
fn unary_rt(rd: u32, rt: u32, expression: &str) -> String {
    format!("SET_GPR_VEC(ctx, {rd}, {expression}(GPR_VEC(ctx, {rt})));")
}

/// `MMI0Functions`: sub-opcode -> expression, in declaration order.
const MMI0_TABLE: &[(u32, &str)] = &[
    (MMI0_PADDW, "PS2_PADDW"),
    (MMI0_PSUBW, "PS2_PSUBW"),
    (MMI0_PCGTW, "PS2_PCGTW"),
    (MMI0_PMAXW, "PS2_PMAXW"),
    (MMI0_PADDH, "PS2_PADDH"),
    (MMI0_PSUBH, "PS2_PSUBH"),
    (MMI0_PCGTH, "PS2_PCGTH"),
    (MMI0_PMAXH, "PS2_PMAXH"),
    (MMI0_PADDB, "PS2_PADDB"),
    (MMI0_PSUBB, "PS2_PSUBB"),
    (MMI0_PCGTB, "PS2_PCGTB"),
    (MMI0_PADDSW, "PS2_PADDSW"),
    (MMI0_PSUBSW, "PS2_PSUBSW"),
    (MMI0_PEXTLW, "PS2_PEXTLW"),
    (MMI0_PPACW, "PS2_PPACW"),
    (MMI0_PADDSH, "_mm_adds_epi16"),
    (MMI0_PSUBSH, "_mm_subs_epi16"),
    (MMI0_PEXTLH, "PS2_PEXTLH"),
    (MMI0_PPACH, "PS2_PPACH"),
    (MMI0_PADDSB, "_mm_adds_epi8"),
    (MMI0_PSUBSB, "_mm_subs_epi8"),
    (MMI0_PEXTLB, "PS2_PEXTLB"),
    (MMI0_PPACB, "PS2_PPACB"),
];

/// `MMI1Functions`: sub-opcode -> expression for the binary forms.
const MMI1_TABLE: &[(u32, &str)] = &[
    (MMI1_PCEQW, "PS2_PCEQW"),
    (MMI1_PMINW, "PS2_PMINW"),
    (MMI1_PCEQH, "PS2_PCEQH"),
    (MMI1_PMINH, "PS2_PMINH"),
    (MMI1_PCEQB, "PS2_PCEQB"),
    (MMI1_PADDUW, "ps2_paddu32"),
    (MMI1_PSUBUW, "ps2_psubu32"),
    (MMI1_PEXTUW, "PS2_PEXTUW"),
    (MMI1_PADDUH, "_mm_adds_epu16"),
    (MMI1_PSUBUH, "_mm_subs_epu16"),
    (MMI1_PEXTUH, "PS2_PEXTUH"),
    (MMI1_PADDUB, "_mm_adds_epu8"),
    (MMI1_PSUBUB, "_mm_subs_epu8"),
    (MMI1_PEXTUB, "PS2_PEXTUB"),
];

/// `translateMMI0Instruction`.
pub fn mmi0(hooks: &mut impl FamilyTranslator, inst: &Instruction) -> String {
    let subfunction = inst.sa;
    let (rs, rt, rd) = (inst.rs, inst.rt, inst.rd);

    if let Some((_, expression)) = MMI0_TABLE.iter().find(|(code, _)| *code == subfunction) {
        return binary(rd, rs, rt, expression);
    }

    match subfunction {
        MMI0_PEXT5 => pext5(inst),
        MMI0_PPAC5 => ppac5(inst),
        _ => hooks.emit_unhandled(inst, &format!("Unhandled MMI0 instruction: function 0x{subfunction:X}")),
    }
}

/// `translateMMI1Instruction`.
pub fn mmi1(hooks: &mut impl FamilyTranslator, inst: &Instruction) -> String {
    let subfunction = inst.sa;
    let (rs, rt, rd) = (inst.rs, inst.rt, inst.rd);

    // GOW-Port: the operand of the absolute value is `rt`, not `rs`.
    if subfunction == MMI1_PABSW {
        return unary_rt(rd, rt, "PS2_PABSW");
    }
    if subfunction == MMI1_PABSH {
        return unary_rt(rd, rt, "PS2_PABSH");
    }
    if let Some((_, expression)) = MMI1_TABLE.iter().find(|(code, _)| *code == subfunction) {
        return binary(rd, rs, rt, expression);
    }

    match subfunction {
        MMI1_PADSBH => padsbh(inst),
        MMI1_QFSRV => {
            format!("SET_GPR_VEC(ctx, {}, PS2_QFSRV(GPR_VEC(ctx, {}), GPR_VEC(ctx, {}), ctx->sa & 0x7F));", rd, rs, rt)
        }
        _ => hooks.emit_unhandled(inst, &format!("Unhandled MMI1 instruction: function 0x{subfunction:X}")),
    }
}

/// `translatePEXT5`: spreads the four 5-bit fields of `rt` into `rd`.
fn pext5(inst: &Instruction) -> String {
    format!(
        "{{ __m128i rt = GPR_VEC(ctx, {}); \n   __m128i m1 = _mm_set1_epi32(0x0000001F); \n   \
         __m128i m2 = _mm_set1_epi32(0x000003E0); \n   __m128i m3 = _mm_set1_epi32(0x00007C00); \n   \
         __m128i m4 = _mm_set1_epi32(0x00008000); \n   \
         __m128i a1 = _mm_slli_epi32(_mm_and_si128(rt, m1), 3); \n   \
         __m128i a2 = _mm_slli_epi32(_mm_and_si128(rt, m2), 6); \n   \
         __m128i a3 = _mm_slli_epi32(_mm_and_si128(rt, m3), 9); \n   \
         __m128i a4 = _mm_slli_epi32(_mm_and_si128(rt, m4), 16); \n   \
         SET_GPR_VEC(ctx, {}, _mm_or_si128(_mm_or_si128(a1, a2), _mm_or_si128(a3, a4))); }}",
        inst.rt, inst.rd
    )
}

/// `translatePPAC5`: the reverse of `PEXT5`.
fn ppac5(inst: &Instruction) -> String {
    format!(
        "{{ __m128i rt = GPR_VEC(ctx, {}); \n   __m128i m1 = _mm_set1_epi32(0x0000001F); \n   \
         __m128i m2 = _mm_set1_epi32(0x000003E0); \n   __m128i m3 = _mm_set1_epi32(0x00007C00); \n   \
         __m128i m4 = _mm_set1_epi32(0x00008000); \n   \
         __m128i a1 = _mm_and_si128(_mm_srli_epi32(rt, 3), m1); \n   \
         __m128i a2 = _mm_and_si128(_mm_srli_epi32(rt, 6), m2); \n   \
         __m128i a3 = _mm_and_si128(_mm_srli_epi32(rt, 9), m3); \n   \
         __m128i a4 = _mm_and_si128(_mm_srli_epi32(rt, 16), m4); \n   \
         SET_GPR_VEC(ctx, {}, _mm_or_si128(_mm_or_si128(a1, a2), _mm_or_si128(a3, a4))); }}",
        inst.rt, inst.rd
    )
}

/// `translatePADSBH`.
fn padsbh(inst: &Instruction) -> String {
    format!(
        "{{ __m128i rs = GPR_VEC(ctx, {}); __m128i rt = GPR_VEC(ctx, {}); \n   \
         __m128i sub = _mm_sub_epi16(rs, rt); \n   __m128i add = _mm_add_epi16(rs, rt); \n   \
         SET_GPR_VEC(ctx, {}, _mm_unpacklo_epi64(sub, _mm_unpackhi_epi64(add, add))); }}",
        inst.rs, inst.rt, inst.rd
    )
}
