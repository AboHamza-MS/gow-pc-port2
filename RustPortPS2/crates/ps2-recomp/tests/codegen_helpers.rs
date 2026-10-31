//! `formatFloatLiteral` and `vuMaskExpr`, pinned against the C++ output.
//!
//! The vectors come from `tools/float_vectors.cpp`, which prints
//! `formatFloatLiteral` for a list of bit patterns using the C library's
//! `%.9g` (what `fmt::format("{:.9g}")` implements). Regenerate them with
//! `cl /std:c++20 float_vectors.cpp && float_vectors.exe` after a change.

use ps2_recomp::codegen_helpers::{format_float_literal, vu_mask_expr};

const VECTORS: &[(u32, &str)] = &[
    (0x00000000, "0.0f"),
    (0x80000000, "-0.0f"),
    (0x3F800000, "1.0f"),
    (0xBF800000, "-1.0f"),
    (0x3F000000, "0.5f"),
    (0x3E800000, "0.25f"),
    (0x3DCCCCCD, "0.100000001f"),
    (0x40000000, "2.0f"),
    (0x3FC00000, "1.5f"),
    (0x3EAAAAAB, "0.333333343f"),
    (0x40490FDB, "3.14159274f"),
    (0x402DF854, "2.71828175f"),
    (0x4B800000, "16777216.0f"),
    (0x4E6E6B28, "1e+09f"),
    (0x501502F9, "1e+10f"),
    (0x4CEB79A3, "123456792.0f"),
    (0x3901742E, "0.00012345679f"),
    (0x38D1B717, "9.99999975e-05f"),
    (0x3727C5AC, "9.99999975e-06f"),
    (0x322BCC77, "9.99999994e-09f"),
    (0x34210FB0, "1.50000005e-07f"),
    (0xB2D6BF95, "-2.50000003e-08f"),
    (0x7F7FFFFF, "3.40282347e+38f"),
    (0x00800000, "1.17549435e-38f"),
    (0x00000001, "1.40129846e-45f"),
    (0x3F800000, "1.0f"),
    (0x3EAAAAAB, "0.333333343f"),
    (0xBEAAAAAB, "-0.333333343f"),
    (0x449A522B, "1234.56775f"),
    (0x3AA1D139, "0.00123456784f"),
    (0x66FF0C2E, "6.02214064e+23f"),
    (0x7F800000, "INFINITY"),
    (0xFF800000, "-INFINITY"),
    (0x7FC00000, "INFINITY"),
];

#[test]
fn float_literals_match_the_cpp_output() {
    for (bits, expected) in VECTORS {
        let value = f32::from_bits(*bits);
        assert_eq!(&format_float_literal(value), expected, "bits {bits:08X}");
    }
}

#[test]
fn a_nan_prints_as_infinity_like_the_cpp_does() {
    // `!isfinite(value)` and then `value < 0.0f`, which is false for every NaN,
    // including the ones with the sign bit set.
    assert_eq!(format_float_literal(f32::NAN), "INFINITY");
    assert_eq!(format_float_literal(-f32::NAN), "INFINITY");
    assert_eq!(format_float_literal(-f32::INFINITY), "-INFINITY");
}

#[test]
fn vu_masks_use_minus_one_for_the_selected_fields() {
    assert_eq!(vu_mask_expr(0x0), "_mm_castsi128_ps(_mm_set_epi32(0, 0, 0, 0))");
    assert_eq!(vu_mask_expr(0x1), "_mm_castsi128_ps(_mm_set_epi32(-1, 0, 0, 0))");
    assert_eq!(vu_mask_expr(0x8), "_mm_castsi128_ps(_mm_set_epi32(0, 0, 0, -1))");
    assert_eq!(vu_mask_expr(0x6), "_mm_castsi128_ps(_mm_set_epi32(0, -1, -1, 0))");
    assert_eq!(vu_mask_expr(0xF), "_mm_castsi128_ps(_mm_set_epi32(-1, -1, -1, -1))");
}
