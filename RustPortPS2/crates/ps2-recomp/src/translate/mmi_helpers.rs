//! MMI helper translations (`mmi_translation_helpers.cpp`).
//!
//! The file holds six entry points and about thirty helpers. `PMFHL`/`PMTHL`,
//! the MMI2/MMI3 tables and their helpers are here; MMI0/MMI1 live in
//! `mmi_tables.rs`. Multi-line templates are `concat!` of the C++ literal pieces,
//! because a backslash continuation would strip the leading spaces the C++ keeps.

use ps2_r5900::opcodes::*;
use ps2_r5900::Instruction;

use crate::translate::instruction::FamilyTranslator;

/// `translatePMFHLInstruction`: `pmfhl.<sa>` packs HI/LO into `rd`.
pub fn pmfhl(hooks: &mut impl FamilyTranslator, inst: &Instruction) -> String {
    let subfunction = inst.sa;
    match subfunction {
        PMFHL_LW => format!("SET_GPR_VEC(ctx, {}, PS2_PMFHL_LW(ctx->hi, ctx->lo));", inst.rd),
        PMFHL_UW => format!("SET_GPR_VEC(ctx, {}, PS2_PMFHL_UW(ctx->hi, ctx->lo));", inst.rd),
        PMFHL_SLW => format!("SET_GPR_VEC(ctx, {}, PS2_PMFHL_SLW(ctx->hi, ctx->lo));", inst.rd),
        PMFHL_LH => format!("SET_GPR_VEC(ctx, {}, PS2_PMFHL_LH(ctx->hi, ctx->lo));", inst.rd),
        PMFHL_SH => format!("SET_GPR_VEC(ctx, {}, PS2_PMFHL_SH(ctx->hi, ctx->lo));", inst.rd),
        _ => hooks.emit_unhandled(inst, &format!("Unhandled PMFHL instruction: function 0x{subfunction:X}")),
    }
}

/// `translatePMTHLInstruction`: only the `PMFHL_LW` sub-function is implemented
/// in the C++, reading the two low 32-bit lanes of `rs` into LO and HI.
pub fn pmthl(hooks: &mut impl FamilyTranslator, inst: &Instruction) -> String {
    let subfunction = inst.sa;
    match subfunction {
        PMFHL_LW => format!(
            "{{ __m128i val = GPR_VEC(ctx, {}); ctx->lo = _mm_extract_epi32(val, 0); \
             ctx->hi = _mm_extract_epi32(val, 1); }}",
            inst.rs
        ),
        _ => hooks.emit_unhandled(inst, &format!("Unhandled PMTHL instruction: function 0x{subfunction:X}")),
    }
}

/// `SET_GPR_VEC(ctx, rd, <expr>(GPR_VEC(ctx, rs), GPR_VEC(ctx, rt)));`
fn binary(rd: u32, rs: u32, rt: u32, expression: &str) -> String {
    format!("SET_GPR_VEC(ctx, {rd}, {expression}(GPR_VEC(ctx, {rs}), GPR_VEC(ctx, {rt})));")
}

/// `SET_GPR_VEC(ctx, rd, <expr>(GPR_VEC(ctx, rt)));`
fn unary_rt(rd: u32, rt: u32, expression: &str) -> String {
    format!("SET_GPR_VEC(ctx, {rd}, {expression}(GPR_VEC(ctx, {rt})));")
}

/// `translateMMI2Instruction`.
pub fn mmi2(hooks: &mut impl FamilyTranslator, inst: &Instruction) -> String {
    let subfunction = inst.sa;
    let (rs, rt, rd) = (inst.rs, inst.rt, inst.rd);
    match subfunction {
        MMI2_PMADDW => pmaddw(inst),
        MMI2_PSLLVW => binary(rd, rs, rt, "PS2_PSLLVW"),
        MMI2_PSRLVW => binary(rd, rs, rt, "PS2_PSRLVW"),
        MMI2_PMSUBW => pmsubw(inst),
        MMI2_PMFHI => format!("SET_GPR_U64(ctx, {rd}, ctx->hi);"),
        MMI2_PMFLO => format!("SET_GPR_U64(ctx, {rd}, ctx->lo);"),
        MMI2_PINTH => binary(rd, rs, rt, "PS2_PINTH"),
        MMI2_PMULTW => pmultw(inst),
        MMI2_PDIVW => pdivw(inst),
        MMI2_PCPYLD => binary(rd, rs, rt, "PS2_PCPYLD"),
        MMI2_PAND => binary(rd, rs, rt, "PS2_PAND"),
        MMI2_PXOR => binary(rd, rs, rt, "PS2_PXOR"),
        MMI2_PMADDH => pmaddh(inst),
        MMI2_PHMADH => phmadh(inst),
        MMI2_PMSUBH => pmsubh(inst),
        MMI2_PHMSBH => phmsbh(inst),
        // GOW-Port: permutation corrected according to PCSX2 (see ps2_runtime_macros.h).
        MMI2_PEXEH => unary_rt(rd, rt, "PS2_PEXEH"),
        MMI2_PREVH => unary_rt(rd, rt, "PS2_PREVH"),
        MMI2_PMULTH => pmulth(inst),
        MMI2_PDIVBW => pdivbw(inst),
        MMI2_PEXEW => unary_rt(rd, rt, "PS2_PEXEW"),
        MMI2_PROT3W => unary_rt(rd, rt, "PS2_PROT3W"),
        _ => hooks.emit_unhandled(inst, &format!("Unhandled MMI2 instruction: function 0x{subfunction:X}")),
    }
}

/// `translateMMI3Instruction`.
pub fn mmi3(hooks: &mut impl FamilyTranslator, inst: &Instruction) -> String {
    let subfunction = inst.sa;
    let (rs, rt, rd) = (inst.rs, inst.rt, inst.rd);
    match subfunction {
        MMI3_PMADDUW => pmadduw(inst),
        MMI3_PSRAVW => binary(rd, rs, rt, "PS2_PSRAVW"),
        // PMTHI/PMTLO use the standard HI/LO.
        MMI3_PMTHI => format!("ctx->hi = GPR_U32(ctx, {rs});"),
        MMI3_PMTLO => format!("ctx->lo = GPR_U32(ctx, {rs});"),
        MMI3_PINTEH => binary(rd, rs, rt, "PS2_PINTEH"),
        MMI3_PMULTUW => pmultuw(inst),
        MMI3_PDIVUW => pdivuw(inst),
        // Upper 64 of rs to the lower 64 of rd, upper 64 of rt to the upper 64 (order matters).
        MMI3_PCPYUD => binary(rd, rs, rt, "_mm_unpackhi_epi64"),
        MMI3_POR => binary(rd, rs, rt, "PS2_POR"),
        MMI3_PNOR => binary(rd, rs, rt, "PS2_PNOR"),
        // GOW-Port: permutation corrected according to PCSX2 (see ps2_runtime_macros.h).
        MMI3_PEXCH => unary_rt(rd, rt, "PS2_PEXCH"),
        MMI3_PCPYH => pcpyh(inst),
        MMI3_PEXCW => unary_rt(rd, rt, "PS2_PEXCW"),
        _ => hooks.emit_unhandled(inst, &format!("Unhandled MMI3 instruction: function 0x{subfunction:X}")),
    }
}

/// The PMADDW/PMSUBW/PMULTW/PMADDUW family: `acc <op>= each of the four 32x32 products`.
fn accumulate_words(inst: &Instruction, start: &str, op: &str) -> String {
    format!(
        concat!(
            "{{ __m128i p01 = _mm_mul_epu32(GPR_VEC(ctx, {rs}), GPR_VEC(ctx, {rt})); \n",
            "   __m128i p23 = _mm_mul_epu32(_mm_srli_si128(GPR_VEC(ctx, {rs}), 8), _mm_srli_si128(GPR_VEC(ctx, {rt}), 8)); \n",
            "   uint64_t acc = {start}; \n",
            "   acc {op}= _mm_cvtsi128_si64(p01); \n",
            "   acc {op}= _mm_cvtsi128_si64(_mm_srli_si128(p01, 8)); \n",
            "   acc {op}= _mm_cvtsi128_si64(p23); \n",
            "   acc {op}= _mm_cvtsi128_si64(_mm_srli_si128(p23, 8)); \n",
            "   ctx->lo = (uint32_t)acc; ctx->hi = (uint32_t)(acc >> 32); \n",
            "   SET_GPR_U64(ctx, {rd}, acc); }}"
        ),
        rs = inst.rs,
        rt = inst.rt,
        rd = inst.rd,
        start = start,
        op = op
    )
}

/// `translatePMADDW`.
fn pmaddw(inst: &Instruction) -> String {
    accumulate_words(inst, "Ps2HiLoToU64(ctx->hi, ctx->lo)", "+")
}

/// `translatePMSUBW`.
fn pmsubw(inst: &Instruction) -> String {
    accumulate_words(inst, "Ps2HiLoToU64(ctx->hi, ctx->lo)", "-")
}

/// `translatePMULTW`.
fn pmultw(inst: &Instruction) -> String {
    accumulate_words(inst, "0", "+")
}

/// `translatePMADDUW` (the text is the same as PMADDW in the C++).
fn pmadduw(inst: &Instruction) -> String {
    accumulate_words(inst, "Ps2HiLoToU64(ctx->hi, ctx->lo)", "+")
}

/// `translatePDIVW`: only the first word, `rs[0] / rt[0]`.
fn pdivw(inst: &Instruction) -> String {
    format!(
        concat!(
            "{{ int32_t rs0 = GPR_S32(ctx, {rs}); int32_t rt0 = GPR_S32(ctx, {rt}); \n",
            "   if (rt0 != 0) {{ \n",
            "       if (rt0 == -1 && rs0 == INT32_MIN) {{ ctx->lo = (uint32_t)INT32_MIN; ctx->hi = 0; }} \n",
            "       else {{ ctx->lo = (uint32_t)(rs0 / rt0); ctx->hi = (uint32_t)(rs0 % rt0); }} \n",
            "   }} else {{ ctx->lo = (rs0 < 0) ? 1 : -1; ctx->hi = (uint32_t)rs0; }} \n",
            "   SET_GPR_U32(ctx, {rd}, ctx->lo); }}"
        ),
        rs = inst.rs,
        rt = inst.rt,
        rd = inst.rd
    )
}

/// `translatePMADDH`: halfword multiply-add into HI/LO and `rd`.
fn pmaddh(inst: &Instruction) -> String {
    halfword_pairs(inst, "+")
}

/// `translatePMSUBH`.
fn pmsubh(inst: &Instruction) -> String {
    halfword_pairs(inst, "-")
}

fn halfword_pairs(inst: &Instruction, op: &str) -> String {
    format!(
        concat!(
            "{{ __m128i prod = _mm_madd_epi16(GPR_VEC(ctx, {rs}), GPR_VEC(ctx, {rt})); \n",
            "   int32_t p0 = _mm_cvtsi128_si32(prod); \n",
            "   int32_t p1 = _mm_cvtsi128_si32(_mm_srli_si128(prod, 4)); \n",
            "   int32_t p2 = _mm_cvtsi128_si32(_mm_srli_si128(prod, 8)); \n",
            "   int32_t p3 = _mm_cvtsi128_si32(_mm_srli_si128(prod, 12)); \n",
            "   int64_t acc = Ps2HiLoToU64(ctx->hi, ctx->lo); \n",
            "   acc {op}= (int64_t)p0 + (int64_t)p1 + (int64_t)p2 + (int64_t)p3; \n",
            "   ctx->lo = (uint32_t)acc; ctx->hi = (uint32_t)(acc >> 32); \n",
            "   SET_GPR_U64(ctx, {rd}, acc); }}"
        ),
        rs = inst.rs,
        rt = inst.rt,
        rd = inst.rd,
        op = op
    )
}

/// `translatePHMADH`.
fn phmadh(inst: &Instruction) -> String {
    format!(
        concat!(
            "{{ __m128i evens = _mm_shuffle_epi32(GPR_VEC(ctx, {rs}), _MM_SHUFFLE(2,0,2,0)); \n",
            "   __m128i odds  = _mm_shuffle_epi32(GPR_VEC(ctx, {rt}), _MM_SHUFFLE(3,1,3,1)); \n",
            "   __m128i prod_ev = _mm_mullo_epi16(evens, _mm_shuffle_epi32(GPR_VEC(ctx, {rs}), _MM_SHUFFLE(2,0,2,0))); \n",
            "   __m128i prod_od = _mm_mullo_epi16(odds,  _mm_shuffle_epi32(GPR_VEC(ctx, {rt}), _MM_SHUFFLE(3,1,3,1))); \n",
            "   __m128i sum_pairs = _mm_add_epi16(prod_ev, prod_od); \n",
            "   int32_t h0 = _mm_extract_epi16(sum_pairs, 0) + _mm_extract_epi16(sum_pairs, 1); \n",
            "   int32_t h1 = _mm_extract_epi16(sum_pairs, 2) + _mm_extract_epi16(sum_pairs, 3); \n",
            "   int32_t h2 = _mm_extract_epi16(sum_pairs, 4) + _mm_extract_epi16(sum_pairs, 5); \n",
            "   int32_t h3 = _mm_extract_epi16(sum_pairs, 6) + _mm_extract_epi16(sum_pairs, 7); \n",
            "   int64_t acc = Ps2HiLoToU64(ctx->hi, ctx->lo); \n",
            "   acc += (int64_t)h0 + (int64_t)h1 + (int64_t)h2 + (int64_t)h3; \n",
            "   ctx->lo = (uint32_t)acc; ctx->hi = (uint32_t)(acc >> 32); \n",
            "   SET_GPR_U64(ctx, {rd}, acc); }}"
        ),
        rs = inst.rs,
        rt = inst.rt,
        rd = inst.rd
    )
}

/// `translatePHMSBH`.
fn phmsbh(inst: &Instruction) -> String {
    format!(
        concat!(
            "{{ __m128i evens = _mm_shuffle_epi32(GPR_VEC(ctx, {rs}), _MM_SHUFFLE(2,0,2,0)); \n",
            "   __m128i odds  = _mm_shuffle_epi32(GPR_VEC(ctx, {rt}), _MM_SHUFFLE(3,1,3,1)); \n",
            "   __m128i prod_ev = _mm_mullo_epi16(evens, _mm_shuffle_epi32(GPR_VEC(ctx, {rs}), _MM_SHUFFLE(2,0,2,0))); \n",
            "   __m128i prod_od = _mm_mullo_epi16(odds,  _mm_shuffle_epi32(GPR_VEC(ctx, {rt}), _MM_SHUFFLE(3,1,3,1))); \n",
            "   __m128i sub_pairs = _mm_sub_epi16(prod_od, prod_ev); \n",
            "   int32_t h0 = _mm_extract_epi16(sub_pairs, 0) + _mm_extract_epi16(sub_pairs, 1); \n",
            "   int32_t h1 = _mm_extract_epi16(sub_pairs, 2) + _mm_extract_epi16(sub_pairs, 3); \n",
            "   int32_t h2 = _mm_extract_epi16(sub_pairs, 4) + _mm_extract_epi16(sub_pairs, 5); \n",
            "   int32_t h3 = _mm_extract_epi16(sub_pairs, 6) + _mm_extract_epi16(sub_pairs, 7); \n",
            "   int64_t acc = Ps2HiLoToU64(ctx->hi, ctx->lo); \n",
            "   acc += (int64_t)h0 + (int64_t)h1 + (int64_t)h2 + (int64_t)h3; \n",
            "   ctx->lo = (uint32_t)acc; ctx->hi = (uint32_t)(acc >> 32); \n",
            "   SET_GPR_U64(ctx, {rd}, acc); }}"
        ),
        rs = inst.rs,
        rt = inst.rt,
        rd = inst.rd
    )
}

/// `translatePMULTH`: GOW-Port, exact PMULTH through the runtime helper.
fn pmulth(inst: &Instruction) -> String {
    format!(
        "{{ extern void ps2PmulthExec(R5900Context *ctx, int rs, int rt, int rd); ps2PmulthExec(ctx, {}, {}, {}); }}",
        inst.rs, inst.rt, inst.rd
    )
}

/// `translatePDIVBW`.
fn pdivbw(inst: &Instruction) -> String {
    format!(
        concat!(
            "{{\n",
            "    __m128i rsVec = GPR_VEC(ctx, {rs});\n",
            "    __m128i rtVec = GPR_VEC(ctx, {rt});\n",
            "    alignas(16) int32_t rsWords[4];\n",
            "    alignas(16) int32_t rtWords[4];\n",
            "    _mm_store_si128((__m128i*)rsWords, rsVec);\n",
            "    _mm_store_si128((__m128i*)rtWords, rtVec);\n",
            "    int32_t div = rtWords[0];\n",
            "    int32_t q0 = 0, q1 = 0, q2 = 0, q3 = 0;\n",
            "    if (div != 0) {{\n",
            "        q0 = rsWords[0] / div; ctx->lo = (uint32_t)q0; ctx->hi = (uint32_t)(rsWords[0] % div);\n",
            "        q1 = rsWords[1] / div;\n",
            "        q2 = rsWords[2] / div;\n",
            "        q3 = rsWords[3] / div;\n",
            "    }} else {{\n",
            "        ctx->lo = (rsWords[0] < 0) ? 1 : -1;\n",
            "        ctx->hi = (uint32_t)rsWords[0];\n",
            "    }}\n",
            "    SET_GPR_VEC(ctx, {rd}, _mm_set_epi32(q3, q2, q1, q0));\n",
            "}}"
        ),
        rs = inst.rs,
        rt = inst.rt,
        rd = inst.rd
    )
}

/// `translatePMULTUW`: HI/LO from the first product only, `rd` gets the low halves of all four.
fn pmultuw(inst: &Instruction) -> String {
    format!(
        concat!(
            "{{ __m128i p01 = _mm_mul_epu32(GPR_VEC(ctx, {rs}), GPR_VEC(ctx, {rt})); \n",
            "   __m128i p23 = _mm_mul_epu32(_mm_srli_si128(GPR_VEC(ctx, {rs}), 8), _mm_srli_si128(GPR_VEC(ctx, {rt}), 8)); \n",
            "   uint64_t res0 = _mm_cvtsi128_si64(p01); uint64_t res1 = _mm_cvtsi128_si64(_mm_srli_si128(p01, 8)); \n",
            "   uint64_t res2 = _mm_cvtsi128_si64(p23); uint64_t res3 = _mm_cvtsi128_si64(_mm_srli_si128(p23, 8)); \n",
            "   ctx->lo = (uint32_t)res0; ctx->hi = (uint32_t)(res0 >> 32); \n",
            "   SET_GPR_VEC(ctx, {rd}, _mm_set_epi32((uint32_t)res3, (uint32_t)res2, (uint32_t)res1, (uint32_t)res0)); }}"
        ),
        rs = inst.rs,
        rt = inst.rt,
        rd = inst.rd
    )
}

/// `translatePDIVUW`: first element only; division by zero gives `0xFFFFFFFF`.
fn pdivuw(inst: &Instruction) -> String {
    format!(
        concat!(
            "{{ uint32_t rs0 = GPR_U32(ctx, {rs}); uint32_t rt0 = GPR_U32(ctx, {rt}); \n",
            "   if (rt0 != 0) {{ ctx->lo = rs0 / rt0; ctx->hi = rs0 % rt0; }} \n",
            "   else {{ ctx->lo = 0xFFFFFFFF; ctx->hi = rs0; }} \n",
            "   SET_GPR_U32(ctx, {rd}, ctx->lo); }}"
        ),
        rs = inst.rs,
        rt = inst.rt,
        rd = inst.rd
    )
}

/// `translatePCPYH`: broadcast the low 16 bits of each 64-bit half.
fn pcpyh(inst: &Instruction) -> String {
    format!(
        concat!(
            "{{ __m128i src = GPR_VEC(ctx, {rt}); uint16_t l = _mm_extract_epi16(src, 0); uint16_t h = _mm_extract_epi16(src, 4); \n",
            "   SET_GPR_VEC(ctx, {rd}, _mm_set_epi16(h,h,h,h, l,l,l,l)); }}"
        ),
        rt = inst.rt,
        rd = inst.rd
    )
}
