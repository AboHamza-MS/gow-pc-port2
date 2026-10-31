//! `MMI` (multimedia) opcodes.
//!
//! Port of `PS2Recomp/ps2xRecomp/src/lib/mmi_translator.cpp`. The HI1/LO1 moves,
//! the 64-bit multiply/divide families, the accumulator forms, `plzcw` and the
//! packed shifts are here; the MMI0-3/PMFHL/PMTHL tables live in
//! `mmi_translation_helpers.cpp` and are delegated through the trait.

use ps2_r5900::opcodes::*;
use ps2_r5900::Instruction;

use crate::translate::instruction::FamilyTranslator;

/// `{ int64_t result = a * b; ctx->lo = ...; ctx->hi = ...; [SET_GPR_S32] }`.
fn multiply(ty: &str, accessor: &str, lo: &str, hi: &str, rs: u32, rt: u32, rd: Option<u32>) -> String {
    let store = rd.map(|rd| format!(" SET_GPR_S32(ctx, {rd}, (int32_t)result);")).unwrap_or_default();
    format!(
        "{{ {ty} result = ({ty}){accessor}(ctx, {rs}) * ({ty}){accessor}(ctx, {rt}); \
         ctx->{lo} = (uint64_t)(int64_t)(int32_t)result; \
         ctx->{hi} = (uint64_t)(int64_t)(int32_t)(result >> 32);{store} }}"
    )
}

/// `{ uint64_t acc = Ps2HiLoToU64(...); <ty> prod = ...; <ty> result = acc <op> prod; ... }`.
#[allow(clippy::too_many_arguments)]
fn accumulate(
    ty: &str,
    accessor: &str,
    acc_hi: &str,
    acc_lo: &str,
    hi: &str,
    lo: &str,
    op: char,
    rs: u32,
    rt: u32,
    rd: Option<u32>,
) -> String {
    let store = rd.map(|rd| format!(" SET_GPR_S32(ctx, {rd}, (int32_t)result);")).unwrap_or_default();
    format!(
        "{{ uint64_t acc = Ps2HiLoToU64(ctx->{acc_hi}, ctx->{acc_lo}); \
         {ty} prod = ({ty}){accessor}(ctx, {rs}) * ({ty}){accessor}(ctx, {rt}); \
         {ty} result = acc {op} prod; \
         ctx->{lo} = Ps2SignExt32ToU64((uint32_t)result); \
         ctx->{hi} = Ps2SignExt32ToU64((uint32_t)(result >> 32));{store} }}"
    )
}

/// Translate one `MMI` instruction.
pub fn translate(hooks: &mut impl FamilyTranslator, inst: &Instruction) -> String {
    let function = inst.function;
    let rs = inst.rs;
    let rt = inst.rt;
    let rd = inst.rd;
    let sa = inst.sa;
    let rd_if_set = if rd != 0 { Some(rd) } else { None };

    match function {
        MMI_MFHI1 => format!("SET_GPR_U64(ctx, {rd}, ctx->hi1);"),
        MMI_MTHI1 => format!("ctx->hi1 = GPR_U64(ctx, {rs});"),
        MMI_MFLO1 => format!("SET_GPR_U64(ctx, {rd}, ctx->lo1);"),
        MMI_MTLO1 => format!("ctx->lo1 = GPR_U64(ctx, {rs});"),
        MMI_MULT1 => multiply("int64_t", "GPR_S32", "lo1", "hi1", rs, rt, rd_if_set),
        MMI_MULTU1 => multiply("uint64_t", "GPR_U32", "lo1", "hi1", rs, rt, rd_if_set),
        MMI_MADD => accumulate("int64_t", "GPR_S32", "hi", "lo", "hi", "lo", '+', rs, rt, rd_if_set),
        MMI_MADDU => accumulate("uint64_t", "GPR_U32", "hi", "lo", "hi", "lo", '+', rs, rt, rd_if_set),
        MMI_MSUB => accumulate("int64_t", "GPR_S32", "hi", "lo", "hi", "lo", '-', rs, rt, rd_if_set),
        MMI_MSUBU => accumulate("uint64_t", "GPR_U32", "hi", "lo", "hi", "lo", '-', rs, rt, rd_if_set),
        MMI_MADD1 => accumulate("int64_t", "GPR_S32", "hi1", "lo1", "hi1", "lo1", '+', rs, rt, rd_if_set),
        MMI_MADDU1 => accumulate("uint64_t", "GPR_U32", "hi1", "lo1", "hi1", "lo1", '+', rs, rt, rd_if_set),
        MMI_DIV1 => format!(
            concat!(
                "{{ int32_t divisor = GPR_S32(ctx, {rt}); ",
                "int32_t dividend = GPR_S32(ctx, {rs}); ",
                "if (divisor != 0) {{ ",
                "    if (divisor == -1 && dividend == INT32_MIN) {{ ",
                "        ctx->lo1 = (uint64_t)(int64_t)INT32_MIN; ctx->hi1 = 0; ",
                "    }} else {{ ",
                "        ctx->lo1 = (uint64_t)(int64_t)(dividend / divisor); ",
                "        ctx->hi1 = (uint64_t)(int64_t)(dividend % divisor); ",
                "    }} ",
                "}} else {{ ",
                "    ctx->lo1 = (dividend < 0) ? 1ull : 0xFFFFFFFFFFFFFFFFull; ctx->hi1 = (uint64_t)(int64_t)dividend; ",
                "}} }}"
            ),
            rt = rt,
            rs = rs
        ),
        // Note the missing spaces around `=` on the `lo1=...` branch: kept as in the C++.
        MMI_DIVU1 => format!(
            "{{ uint32_t divisor = GPR_U32(ctx, {rt}); if (divisor != 0) {{ \
             ctx->lo1 = (uint64_t)(int64_t)(int32_t)(GPR_U32(ctx, {rs}) / divisor); \
             ctx->hi1 = (uint64_t)(int64_t)(int32_t)(GPR_U32(ctx, {rs}) % divisor); }} else {{ \
             ctx->lo1=0xFFFFFFFFFFFFFFFFull; ctx->hi1=(uint64_t)(int64_t)(int32_t)GPR_U32(ctx,{rs}); }} }}"
        ),
        MMI_PLZCW => format!(
            "{{ \
             uint64_t v = GPR_U64(ctx, {rs}); \
             uint32_t lo = (uint32_t)(v & 0xFFFFFFFFu); \
             uint32_t hi = (uint32_t)(v >> 32); \
             uint64_t out = ((uint64_t)ps2_plzcw32(hi) << 32) | (uint64_t)ps2_plzcw32(lo); \
             SET_GPR_U64(ctx, {rd}, out); \
             }}"
        ),
        MMI_PSLLH => format!("SET_GPR_VEC(ctx, {rd}, _mm_slli_epi16(GPR_VEC(ctx, {rt}), {sa}));"),
        MMI_PSRLH => format!("SET_GPR_VEC(ctx, {rd}, _mm_srli_epi16(GPR_VEC(ctx, {rt}), {sa}));"),
        MMI_PSRAH => format!("SET_GPR_VEC(ctx, {rd}, _mm_srai_epi16(GPR_VEC(ctx, {rt}), {sa}));"),
        MMI_PSLLW => format!("SET_GPR_VEC(ctx, {rd}, _mm_slli_epi32(GPR_VEC(ctx, {rt}), {sa}));"),
        MMI_PSRLW => format!("SET_GPR_VEC(ctx, {rd}, _mm_srli_epi32(GPR_VEC(ctx, {rt}), {sa}));"),
        MMI_PSRAW => format!("SET_GPR_VEC(ctx, {rd}, _mm_srai_epi32(GPR_VEC(ctx, {rt}), {sa}));"),
        MMI_MMI0 => hooks.translate_mmi0(inst),
        MMI_MMI1 => hooks.translate_mmi1(inst),
        MMI_MMI2 => hooks.translate_mmi2(inst),
        MMI_MMI3 => hooks.translate_mmi3(inst),
        MMI_PMFHL => hooks.translate_pmfhl(inst),
        MMI_PMTHL => hooks.translate_pmthl(inst),
        _ => hooks.emit_unhandled(inst, &format!("Unhandled MMI instruction: function 0x{function:X}")),
    }
}
