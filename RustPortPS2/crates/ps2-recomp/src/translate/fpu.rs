//! `COP1` (FPU) opcodes.
//!
//! Port of `PS2Recomp/ps2xRecomp/src/lib/fpu_translator.cpp`, including the GOW
//! port's deviations from the hardware (marked in the C++): no infinities
//! (`ps2FpuDivide`), `SQRT.S` reading the radicand from `ft`, and
//! `RSQRT.S` computing `fs/sqrt(ft)`.

use ps2_r5900::opcodes::*;
use ps2_r5900::Instruction;

use crate::translate::instruction::FamilyTranslator;

/// Translate one `COP1` instruction.
pub fn translate(hooks: &mut impl FamilyTranslator, inst: &Instruction) -> String {
    let format = inst.rs;
    let ft = inst.rt;
    let fs = inst.rd;
    let fd = inst.sa;
    let function = inst.function;

    match format {
        COP1_MF => format!(
            "{{ uint32_t bits; std::memcpy(&bits, &ctx->f[{fs}], sizeof(bits)); SET_GPR_U32(ctx, {ft}, bits); }}"
        ),
        COP1_MT => {
            format!("{{ uint32_t bits = GPR_U32(ctx, {ft}); std::memcpy(&ctx->f[{fs}], &bits, sizeof(bits)); }}")
        }
        COP1_CF => {
            if fs == 31 {
                // FCR31 holds the status and control bits.
                format!("SET_GPR_U32(ctx, {ft}, ctx->fcr31);")
            } else if fs == 0 {
                // FCR0 is the FPU implementation register.
                format!("SET_GPR_U32(ctx, {ft}, 0x00000000);")
            } else {
                format!("SET_GPR_U32(ctx, {ft}, 0); // Unimplemented FCR{fs}")
            }
        }
        COP1_CT => {
            if fs == 31 {
                format!("ctx->fcr31 = GPR_U32(ctx, {ft}) & 0x0183FFFF;")
            } else {
                format!("// CTC1 to FCR{fs} ignored")
            }
        }
        COP1_BC => "// FPU branch instruction - handled elsewhere".to_owned(),
        COP1_S => translate_s(hooks, inst, ft, fs, fd, function),
        COP1_W => {
            if function == COP1_W_CVT_S {
                format!(
                    "{{ int32_t tmp; std::memcpy(&tmp, &ctx->f[{fs}], sizeof(tmp)); ctx->f[{fd}] = FPU_CVT_S_W(tmp); }}"
                )
            } else {
                hooks.emit_unhandled(inst, &format!("Unhandled FPU.W instruction: function 0x{function:X}"))
            }
        }
        _ => hooks
            .emit_unhandled(inst, &format!("Unhandled FPU instruction: format 0x{format:X}, function 0x{function:X}")),
    }
}

fn translate_s(
    hooks: &mut impl FamilyTranslator,
    inst: &Instruction,
    ft: u32,
    fs: u32,
    fd: u32,
    function: u32,
) -> String {
    // The comparison family shares one template; only the predicate changes.
    let predicate = match function {
        COP1_S_C_UN => Some("FPU_C_UN_S"),
        COP1_S_C_EQ => Some("FPU_C_EQ_S"),
        COP1_S_C_UEQ => Some("FPU_C_UEQ_S"),
        COP1_S_C_OLT => Some("FPU_C_OLT_S"),
        COP1_S_C_ULT => Some("FPU_C_ULT_S"),
        COP1_S_C_OLE => Some("FPU_C_OLE_S"),
        COP1_S_C_ULE => Some("FPU_C_ULE_S"),
        COP1_S_C_NGLE => Some("FPU_C_NGLE_S"),
        COP1_S_C_SEQ => Some("FPU_C_SEQ_S"),
        COP1_S_C_NGL => Some("FPU_C_NGL_S"),
        COP1_S_C_LT => Some("FPU_C_LT_S"),
        COP1_S_C_NGE => Some("FPU_C_NGE_S"),
        COP1_S_C_LE => Some("FPU_C_LE_S"),
        COP1_S_C_NGT => Some("FPU_C_NGT_S"),
        _ => None,
    };
    if let Some(predicate) = predicate {
        return format!(
            "ctx->fcr31 = ({predicate}(ctx->f[{fs}], ctx->f[{ft}])) ? (ctx->fcr31 | 0x800000) : (ctx->fcr31 & ~0x800000);"
        );
    }

    match function {
        COP1_S_ADD => format!("ctx->f[{fd}] = FPU_ADD_S(ctx->f[{fs}], ctx->f[{ft}]);"),
        COP1_S_SUB => format!("ctx->f[{fd}] = FPU_SUB_S(ctx->f[{fs}], ctx->f[{ft}]);"),
        COP1_S_MUL => format!("ctx->f[{fd}] = FPU_MUL_S(ctx->f[{fs}], ctx->f[{ft}]);"),
        // GOW-Port: no infinities; divide by zero gives +/-FLT_MAX and sets D/I.
        COP1_S_DIV => format!("ctx->f[{fd}] = ps2FpuDivide(ctx->fcr31, ctx->f[{fs}], ctx->f[{ft}]);"),
        // GOW-Port: the R5900 encodes the radicand in FT.
        COP1_S_SQRT => format!("ctx->f[{fd}] = ps2FpuSqrt(ctx->fcr31, ctx->f[{ft}]);"),
        COP1_S_ABS => format!("ctx->f[{fd}] = FPU_ABS_S(ctx->f[{fs}]);"),
        COP1_S_MOV => format!("ctx->f[{fd}] = FPU_MOV_S(ctx->f[{fs}]);"),
        COP1_S_NEG => format!("ctx->f[{fd}] = FPU_NEG_S(ctx->f[{fs}]);"),
        COP1_S_ROUND_W => {
            format!("{{ int32_t tmp = FPU_ROUND_W_S(ctx->f[{fs}]); std::memcpy(&ctx->f[{fd}], &tmp, sizeof(tmp)); }}")
        }
        COP1_S_TRUNC_W => {
            format!("{{ int32_t tmp = FPU_TRUNC_W_S(ctx->f[{fs}]); std::memcpy(&ctx->f[{fd}], &tmp, sizeof(tmp)); }}")
        }
        COP1_S_CEIL_W => {
            format!("{{ int32_t tmp = FPU_CEIL_W_S(ctx->f[{fs}]); std::memcpy(&ctx->f[{fd}], &tmp, sizeof(tmp)); }}")
        }
        COP1_S_FLOOR_W => {
            format!("{{ int32_t tmp = FPU_FLOOR_W_S(ctx->f[{fs}]); std::memcpy(&ctx->f[{fd}], &tmp, sizeof(tmp)); }}")
        }
        COP1_S_CVT_W => {
            format!("{{ int32_t tmp = FPU_CVT_W_S(ctx->f[{fs}]); std::memcpy(&ctx->f[{fd}], &tmp, sizeof(tmp)); }}")
        }
        // GOW-Port: computes fs/sqrt(ft), not the inverse of sqrt(fs).
        COP1_S_RSQRT => format!("ctx->f[{fd}] = ps2FpuRsqrt(ctx->fcr31, ctx->f[{fs}], ctx->f[{ft}]);"),
        COP1_S_ADDA => format!("FPU_SET_ACC(ctx, FPU_ADD_S(ctx->f[{fs}], ctx->f[{ft}]));"),
        COP1_S_SUBA => format!("FPU_SET_ACC(ctx, FPU_SUB_S(ctx->f[{fs}], ctx->f[{ft}]));"),
        COP1_S_MULA => format!("FPU_SET_ACC(ctx, FPU_MUL_S(ctx->f[{fs}], ctx->f[{ft}]));"),
        COP1_S_MADD => {
            format!("ctx->f[{fd}] = FPU_ADD_S(ctx->f_acc, FPU_MUL_S(ctx->f[{fs}], ctx->f[{ft}]));")
        }
        COP1_S_MSUB => {
            format!("ctx->f[{fd}] = FPU_SUB_S(ctx->f_acc, FPU_MUL_S(ctx->f[{fs}], ctx->f[{ft}]));")
        }
        COP1_S_MADDA => {
            format!("FPU_SET_ACC(ctx, FPU_ADD_S(ctx->f_acc, FPU_MUL_S(ctx->f[{fs}], ctx->f[{ft}])));")
        }
        COP1_S_MSUBA => {
            format!("FPU_SET_ACC(ctx, FPU_SUB_S(ctx->f_acc, FPU_MUL_S(ctx->f[{fs}], ctx->f[{ft}])));")
        }
        COP1_S_MAX => format!("ctx->f[{fd}] = std::max(ctx->f[{fs}], ctx->f[{ft}]);"),
        COP1_S_MIN => format!("ctx->f[{fd}] = std::min(ctx->f[{fs}], ctx->f[{ft}]);"),
        COP1_S_C_F | COP1_S_C_SF => "ctx->fcr31 &= ~0x800000;".to_owned(),
        _ => hooks.emit_unhandled(inst, &format!("Unhandled FPU.S instruction: function 0x{function:X}")),
    }
}
