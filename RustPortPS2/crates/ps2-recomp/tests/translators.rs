use std::sync::Arc;

use ps2_r5900::decode;
use ps2_recomp::generator::CodeGenerator;

/// The text the translators produce for one instruction word at `0x100000`.
fn translate(raw: u32) -> String {
    let mut generator = CodeGenerator::new(&[], Arc::new(Vec::new()));
    generator.translate_instruction(&decode(0x0010_0000, raw, false))
}

// The expected strings below were produced by the C++ recompiler on SCUS_973.99.

#[test]
fn mmi2_pcpyld() {
    assert_eq!(translate(0x7109_1B89), "SET_GPR_VEC(ctx, 3, PS2_PCPYLD(GPR_VEC(ctx, 8), GPR_VEC(ctx, 9)));");
}

#[test]
fn vu0_vmove_with_a_full_mask() {
    assert_eq!(
        translate(0x4BE3_033C),
        "{ __m128i mask = _mm_set_epi32(-1, -1, -1, -1); ctx->vu0_vf[3] = _mm_blendv_ps(ctx->vu0_vf[3], ctx->vu0_vf[0], _mm_castsi128_ps(mask)); }"
    );
}

#[test]
fn vu0_vadd_blends_with_ps2_vblend() {
    assert_eq!(
        translate(0x4BE2_0868),
        "{ __m128 res = PS2_VADD(ctx->vu0_vf[1], ctx->vu0_vf[2]); __m128i mask = _mm_set_epi32(-1, -1, -1, -1); ctx->vu0_vf[1] = PS2_VBLEND(ctx->vu0_vf[1], res, _mm_castsi128_ps(mask)); }"
    );
}

#[test]
fn vu0_accumulator_form_with_a_broadcast_field() {
    assert_eq!(
        translate(0x4BE3_09BC),
        "{ __m128 res = PS2_VMUL(ctx->vu0_vf[1], _mm_shuffle_ps(ctx->vu0_vf[3], ctx->vu0_vf[3], _MM_SHUFFLE(0,0,0,0))); ctx->vu0_acc = _mm_blendv_ps(ctx->vu0_acc, res, _mm_castsi128_ps(_mm_set_epi32(-1, -1, -1, -1))); }"
    );
}

#[test]
fn special_div_keeps_the_cpp_spacing() {
    let text = translate(0x0088_001A); // div a0, t0... rs=4, rt=8
    assert!(text.starts_with(
        "{ int32_t divisor = GPR_S32(ctx, 8);    int32_t dividend = GPR_S32(ctx, 4);    if (divisor != 0) {"
    ));
}

#[test]
fn an_unknown_mmi_subfunction_is_reported_and_throws_at_runtime() {
    // MMI0 with a sub-function the table lacks (0x1C): "Unhandled MMI0 instruction".
    let text = translate((0x1C << 26) | (0x1C << 6) | 0x08);
    assert_eq!(
        text,
        "throw std::runtime_error(\"Unhandled MMI0 instruction: function 0x1C at 0x100000 raw=0x70000708\");"
    );
}
