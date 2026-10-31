//! `COP2` (VU0) opcodes: the transfer and control part.
//!
//! Port of `PS2Recomp/ps2xRecomp/src/lib/vu_translator.cpp` for the formats the
//! file handles itself — `qmfc2`/`qmtc2` (the vector registers as raw bits),
//! `cfc2`/`ctc2` (VI0..VI15 plus the VU special control registers, with the
//! read-only ones refusing the write) and the `bc2` comment.
//!
//! The macro block (`format >= COP2_CO`) dispatches to dozens of
//! `translateVU_*` calls in the C++; it is delegated as a single hook and lands
//! with `vu_translation_helpers.cpp`.

use ps2_r5900::opcodes::*;
use ps2_r5900::Instruction;

use crate::translate::instruction::FamilyTranslator;

/// Translate one `COP2` instruction.
pub fn translate(hooks: &mut impl FamilyTranslator, inst: &Instruction) -> String {
    let format = inst.rs;
    let rt = inst.rt;
    let rd = inst.rd;

    match format {
        COP2_QMFC2 => format!("SET_GPR_VEC(ctx, {rt}, _mm_castps_si128(ctx->vu0_vf[{rd}]));"),
        COP2_CFC2 => {
            // CFC2/CTC2 reuse the 5-bit field for VI0..VI15 and the VU control registers.
            if rd < 16 {
                return format!("SET_GPR_U32(ctx, {rt}, static_cast<uint32_t>(ctx->vi[{rd}]));");
            }
            match rd {
                VU0_CR_STATUS => format!("SET_GPR_U32(ctx, {rt}, ctx->vu0_status);"),
                VU0_CR_MAC => format!("SET_GPR_U32(ctx, {rt}, ctx->vu0_mac_flags);"),
                VU0_CR_CLIP => format!("SET_GPR_U32(ctx, {rt}, ctx->vu0_clip_flags & 0x00FFFFFFu);"),
                VU0_CR_R => format!(
                    "SET_GPR_U32(ctx, {rt}, static_cast<uint32_t>(_mm_cvtsi128_si32(_mm_castps_si128(ctx->vu0_r))));"
                ),
                VU0_CR_I => format!(
                    "{{ uint32_t bits; std::memcpy(&bits, &ctx->vu0_i, sizeof(bits)); SET_GPR_U32(ctx, {rt}, bits); }}"
                ),
                VU0_CR_Q => format!(
                    "{{ uint32_t bits; std::memcpy(&bits, &ctx->vu0_q, sizeof(bits)); SET_GPR_U32(ctx, {rt}, bits); }}"
                ),
                VU0_CR_TPC => format!("SET_GPR_U32(ctx, {rt}, ctx->vu0_tpc >> 3);"),
                VU0_CR_CMSAR0 => format!("SET_GPR_U32(ctx, {rt}, ctx->vu0_cmsar0);"),
                VU0_CR_FBRST => format!("SET_GPR_U32(ctx, {rt}, ctx->vu0_fbrst);"),
                VU0_CR_VPU_STAT => format!("SET_GPR_U32(ctx, {rt}, ctx->vu0_vpu_stat);"),
                VU0_CR_CMSAR1 => format!("SET_GPR_U32(ctx, {rt}, ctx->vu0_cmsar1);"),
                _ => format!("// Unimplemented CFC2 VU control register: {rd}"),
            }
        }
        COP2_QMTC2 => format!("ctx->vu0_vf[{rd}] = _mm_castsi128_ps(GPR_VEC(ctx, {rt}));"),
        COP2_CTC2 => {
            if rd < 16 {
                if rd == 0 {
                    return "// CTC2 write to VI0 ignored".to_owned();
                }
                return format!("ctx->vi[{rd}] = static_cast<uint16_t>(GPR_U32(ctx, {rt}));");
            }
            match rd {
                VU0_CR_STATUS => format!("ctx->vu0_status = static_cast<uint16_t>(GPR_U32(ctx, {rt}) & 0xFFFFu);"),
                VU0_CR_MAC | VU0_CR_TPC | VU0_CR_VPU_STAT => {
                    format!("// CTC2 write to read-only VU control register {rd} ignored")
                }
                VU0_CR_CLIP => format!("ctx->vu0_clip_flags = GPR_U32(ctx, {rt}) & 0x00FFFFFFu;"),
                VU0_CR_R => format!(
                    "ctx->vu0_r = _mm_castsi128_ps(_mm_set1_epi32(static_cast<int32_t>(0x3F800000u | (GPR_U32(ctx, {rt}) & 0x007FFFFFu))));"
                ),
                VU0_CR_I => format!(
                    "{{ uint32_t tmp = GPR_U32(ctx, {rt}); std::memcpy(&ctx->vu0_i, &tmp, sizeof(tmp)); }}"
                ),
                VU0_CR_Q => format!(
                    "{{ uint32_t tmp = GPR_U32(ctx, {rt}); std::memcpy(&ctx->vu0_q, &tmp, sizeof(tmp)); }}"
                ),
                VU0_CR_CMSAR0 => format!("ctx->vu0_cmsar0 = GPR_U32(ctx, {rt});"),
                VU0_CR_FBRST => format!("ctx->vu0_fbrst = GPR_U32(ctx, {rt}) & 0x00000C0Cu;"),
                VU0_CR_CMSAR1 => format!("ctx->vu0_cmsar1 = GPR_U32(ctx, {rt});"),
                _ => format!("// Unimplemented CTC2 VU control register: {rd}"),
            }
        }
        COP2_BC => format!("// BC2 (Condition: 0x{rt:X}) - Handled by branch logic"),
        f if (COP2_CO..=COP2_CO + 15).contains(&f) => hooks.translate_vu_macro(inst),
        _ => hooks.emit_unhandled(inst, &format!("Unhandled COP2 format: 0x{format:X}")),
    }
}
