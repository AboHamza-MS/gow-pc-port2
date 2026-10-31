//! Text helpers shared by every translator.
//!
//! Port of `PS2Recomp/ps2xRecomp/include/ps2recomp/codegen_helpers.h`.
//!
//! The float literal is the delicate one: the C++ side prints it with
//! `fmt::format("{:.9g}", value)`, and Rust has no `%g`. [`format_g`] implements
//! the printf rules (nine significant digits, `%e` when the exponent is below -4
//! or at least the precision, trailing zeros of the fraction removed) and
//! [`format_float_literal`] adds the `.0` and `f` the C++ side adds.

/// `formatFloatLiteral`: a C++ float literal, with the C++ side's quirks.
///
/// A NaN prints as `INFINITY` (the C++ checks `isfinite` and then `value < 0`,
/// which is false for NaN).
pub fn format_float_literal(value: f32) -> String {
    if !value.is_finite() {
        return if value < 0.0 { "-INFINITY".to_owned() } else { "INFINITY".to_owned() };
    }

    let mut literal = format_g(f64::from(value), 9);
    if !literal.contains(['.', 'e', 'E']) {
        literal.push_str(".0");
    }
    literal.push('f');
    literal
}

/// `vuMaskExpr`: the `__m128` mask of a VU destination field.
pub fn vu_mask_expr(dest_mask: u8) -> String {
    format!(
        "_mm_castsi128_ps(_mm_set_epi32({}, {}, {}, {}))",
        if dest_mask & 0x1 != 0 { -1 } else { 0 },
        if dest_mask & 0x2 != 0 { -1 } else { 0 },
        if dest_mask & 0x4 != 0 { -1 } else { 0 },
        if dest_mask & 0x8 != 0 { -1 } else { 0 }
    )
}

/// printf's `%.*g`: `precision` significant digits, `%e` outside `[-4, precision)`.
pub fn format_g(value: f64, precision: usize) -> String {
    let precision = precision.max(1);

    // Formatting with `precision - 1` decimals in scientific notation rounds to
    // the requested number of significant digits and reveals the exponent of
    // the rounded value, which is the exponent printf uses to pick the style.
    let scientific = format!("{:.*e}", precision - 1, value);
    let (mantissa, exponent) = scientific.split_once('e').expect("Rust always prints the exponent");
    let exponent: i32 = exponent.parse().expect("the exponent is an integer");

    if exponent < -4 || exponent >= precision as i32 {
        let mantissa = trim_fraction(mantissa);
        let sign = if exponent < 0 { '-' } else { '+' };
        format!("{mantissa}e{sign}{:02}", exponent.unsigned_abs())
    } else {
        let decimals = (precision as i32 - 1 - exponent).max(0) as usize;
        let fixed = format!("{value:.decimals$}");
        trim_fraction(&fixed).to_owned()
    }
}

/// Drop the trailing zeros of the fractional part, and the point if nothing is
/// left after it.
fn trim_fraction(text: &str) -> &str {
    if !text.contains('.') {
        return text;
    }
    let trimmed = text.trim_end_matches('0');
    trimmed.strip_suffix('.').unwrap_or(trimmed)
}
