//! The `COP2` transfer/control templates.

use ps2_r5900::opcodes::*;
use ps2_r5900::Instruction;
use ps2_recomp::translate::instruction::{translate as translate_generic, FamilyTranslator};
use ps2_recomp::translate::memory::MemoryAccessHint;
use ps2_recomp::translate::vu::translate;

#[derive(Default)]
struct Hooks {
    macro_calls: usize,
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
    fn translate_vu(&mut self, inst: &Instruction) -> String {
        translate(self, inst)
    }
    fn translate_mmi(&mut self, _: &Instruction) -> String {
        String::new()
    }
    fn emit_unhandled(&mut self, _: &Instruction, reason: &str) -> String {
        format!("<unhandled: {reason}>")
    }
    fn translate_vu_macro(&mut self, _: &Instruction) -> String {
        self.macro_calls += 1;
        "<macro>".to_owned()
    }
}

fn cop2(format: u32, rt: u32, rd: u32, function: u32) -> String {
    let raw = (OPCODE_COP2 << 26) | (format << 21) | (rt << 16) | (rd << 11) | function;
    translate_generic(&mut Hooks::default(), &ps2_r5900::decode(0x0010_0000, raw, false), MemoryAccessHint::default())
}

#[test]
fn qmfc2_and_qmtc2_move_the_vector_bits() {
    assert_eq!(cop2(COP2_QMFC2, 2, 5, 0), "SET_GPR_VEC(ctx, 2, _mm_castps_si128(ctx->vu0_vf[5]));");
    assert_eq!(cop2(COP2_QMTC2, 2, 5, 0), "ctx->vu0_vf[5] = _mm_castsi128_ps(GPR_VEC(ctx, 2));");
}

#[test]
fn vi_registers_go_through_the_indexed_array() {
    assert_eq!(cop2(COP2_CFC2, 2, 7, 0), "SET_GPR_U32(ctx, 2, static_cast<uint32_t>(ctx->vi[7]));");
    assert_eq!(cop2(COP2_CTC2, 2, 7, 0), "ctx->vi[7] = static_cast<uint16_t>(GPR_U32(ctx, 2));");
    assert_eq!(cop2(COP2_CTC2, 2, 0, 0), "// CTC2 write to VI0 ignored");
}

#[test]
fn the_control_registers_have_their_own_masks() {
    assert_eq!(cop2(COP2_CFC2, 2, VU0_CR_STATUS, 0), "SET_GPR_U32(ctx, 2, ctx->vu0_status);");
    assert_eq!(cop2(COP2_CFC2, 2, VU0_CR_CLIP, 0), "SET_GPR_U32(ctx, 2, ctx->vu0_clip_flags & 0x00FFFFFFu);");
    assert_eq!(cop2(COP2_CTC2, 2, VU0_CR_FBRST, 0), "ctx->vu0_fbrst = GPR_U32(ctx, 2) & 0x00000C0Cu;");
    assert_eq!(cop2(COP2_CTC2, 2, VU0_CR_TPC, 0), "// CTC2 write to read-only VU control register 26 ignored");
}

#[test]
fn the_macro_block_is_delegated_as_one_hook() {
    let macro_text = cop2(COP2_CO + 3, 2, 3, 0x1F);
    assert_eq!(macro_text, "<macro>");
    assert_eq!(cop2(COP2_BC, 2, 0, 0), "// BC2 (Condition: 0x2) - Handled by branch logic");
    assert_eq!(cop2(0x3, 0, 0, 0), "<unhandled: Unhandled COP2 format: 0x3>");
}
