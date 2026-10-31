//! The `COP1` (FPU) templates.

use ps2_r5900::opcodes::*;
use ps2_r5900::Instruction;
use ps2_recomp::translate::fpu::translate;
use ps2_recomp::translate::instruction::{translate as translate_generic, FamilyTranslator};
use ps2_recomp::translate::memory::MemoryAccessHint;

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
    fn translate_fpu(&mut self, inst: &Instruction) -> String {
        translate(self, inst)
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

fn cop1(format: u32, ft: u32, fs: u32, fd: u32, function: u32) -> String {
    let raw = (OPCODE_COP1 << 26) | (format << 21) | (ft << 16) | (fs << 11) | (fd << 6) | function;
    translate_generic(&mut Hooks, &ps2_r5900::decode(0x0010_0000, raw, false), MemoryAccessHint::default())
}

#[test]
fn mfc1_and_mtc1_move_the_bits() {
    assert_eq!(
        cop1(COP1_MF, 2, 12, 0, 0),
        "{ uint32_t bits; std::memcpy(&bits, &ctx->f[12], sizeof(bits)); SET_GPR_U32(ctx, 2, bits); }"
    );
    assert_eq!(
        cop1(COP1_MT, 2, 12, 0, 0),
        "{ uint32_t bits = GPR_U32(ctx, 2); std::memcpy(&ctx->f[12], &bits, sizeof(bits)); }"
    );
}

#[test]
fn cfc1_reads_fcr31_fcr0_and_complains_about_the_rest() {
    assert_eq!(cop1(COP1_CF, 2, 31, 0, 0), "SET_GPR_U32(ctx, 2, ctx->fcr31);");
    assert_eq!(cop1(COP1_CF, 2, 0, 0, 0), "SET_GPR_U32(ctx, 2, 0x00000000);");
    assert_eq!(cop1(COP1_CF, 2, 7, 0, 0), "SET_GPR_U32(ctx, 2, 0); // Unimplemented FCR7");
}

#[test]
fn ctc1_masks_fcr31() {
    assert_eq!(cop1(COP1_CT, 4, 31, 0, 0), "ctx->fcr31 = GPR_U32(ctx, 4) & 0x0183FFFF;");
    assert_eq!(cop1(COP1_CT, 4, 5, 0, 0), "// CTC1 to FCR5 ignored");
}

#[test]
fn the_gow_deviations_are_kept() {
    // div.s $f0, $f12, $f13: through ps2FpuDivide, not a raw division.
    assert_eq!(cop1(COP1_S, 13, 12, 0, COP1_S_DIV), "ctx->f[0] = ps2FpuDivide(ctx->fcr31, ctx->f[12], ctx->f[13]);");
    // sqrt.s reads the radicand from ft.
    assert_eq!(cop1(COP1_S, 13, 12, 0, COP1_S_SQRT), "ctx->f[0] = ps2FpuSqrt(ctx->fcr31, ctx->f[13]);");
}

#[test]
fn a_comparison_sets_the_condition_bit() {
    assert_eq!(
        cop1(COP1_S, 13, 12, 0, COP1_S_C_EQ),
        "ctx->fcr31 = (FPU_C_EQ_S(ctx->f[12], ctx->f[13])) ? (ctx->fcr31 | 0x800000) : (ctx->fcr31 & ~0x800000);"
    );
    assert_eq!(cop1(COP1_S, 13, 12, 0, COP1_S_C_F), "ctx->fcr31 &= ~0x800000;");
}

#[test]
fn conversions_and_accumulators() {
    assert_eq!(cop1(COP1_S, 13, 12, 0, COP1_S_MUL), "ctx->f[0] = FPU_MUL_S(ctx->f[12], ctx->f[13]);");
    assert_eq!(
        cop1(COP1_S, 13, 12, 0, COP1_S_MADD),
        "ctx->f[0] = FPU_ADD_S(ctx->f_acc, FPU_MUL_S(ctx->f[12], ctx->f[13]));"
    );
    assert_eq!(
        cop1(COP1_W, 0, 12, 0, COP1_W_CVT_S),
        "{ int32_t tmp; std::memcpy(&tmp, &ctx->f[12], sizeof(tmp)); ctx->f[0] = FPU_CVT_S_W(tmp); }"
    );
}

#[test]
fn unknown_formats_and_functions_are_reported() {
    assert_eq!(cop1(COP1_S, 13, 12, 0, 0x0B), "<unhandled: Unhandled FPU.S instruction: function 0xB>");
    assert_eq!(cop1(0x3, 0, 0, 0, 0), "<unhandled: Unhandled FPU instruction: format 0x3, function 0x0>");
}
