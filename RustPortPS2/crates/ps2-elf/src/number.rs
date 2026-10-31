//! `std::stoul(text, nullptr, 0)` emulation.
//!
//! The recompiler's inputs (Ghidra map, TOML patches, `mmio` keys) accept
//! addresses as decimal, octal (`0...`) or hexadecimal (`0x...`) strings, and
//! the C++ side parses all of them with base 0. This module keeps that
//! behaviour, including the C++ quirks: surrounding whitespace is skipped and
//! trailing characters are ignored.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseNumberError {
    /// No digit of the selected radix: `std::stoul` throws `invalid_argument`.
    NoDigits { text: String },
    /// The value does not fit in 32 bits: `std::stoul` throws `out_of_range`.
    OutOfRange { text: String },
    /// A negative value is never a valid address; `std::stoul` would wrap it.
    Negative { text: String },
}

impl fmt::Display for ParseNumberError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDigits { text } => write!(f, "not a number: {text:?}"),
            Self::OutOfRange { text } => write!(f, "number out of range: {text:?}"),
            Self::Negative { text } => write!(f, "negative number: {text:?}"),
        }
    }
}

impl std::error::Error for ParseNumberError {}

/// Parse an address the way `std::stoul(text, nullptr, 0)` does.
pub fn parse_number(text: &str) -> Result<u32, ParseNumberError> {
    let without_space = text.trim_start();
    let (negative, rest) = match without_space.as_bytes().first() {
        Some(b'+') => (false, &without_space[1..]),
        Some(b'-') => (true, &without_space[1..]),
        _ => (false, without_space),
    };

    let (radix, digits) = if let Some(hex) = rest.strip_prefix("0x").or_else(|| rest.strip_prefix("0X")) {
        (16u32, hex)
    } else if rest.len() > 1 && rest.starts_with('0') {
        (8u32, &rest[1..])
    } else {
        (10u32, rest)
    };

    let mut value: u64 = 0;
    let mut count: usize = 0;
    for character in digits.chars() {
        let Some(digit) = character.to_digit(radix) else {
            break;
        };
        value = value
            .checked_mul(u64::from(radix))
            .and_then(|value| value.checked_add(u64::from(digit)))
            .ok_or_else(|| ParseNumberError::OutOfRange { text: text.to_owned() })?;
        count += 1;
    }

    if count == 0 {
        return Err(ParseNumberError::NoDigits { text: text.to_owned() });
    }
    if value > u64::from(u32::MAX) {
        return Err(ParseNumberError::OutOfRange { text: text.to_owned() });
    }
    if negative {
        return Err(ParseNumberError::Negative { text: text.to_owned() });
    }
    Ok(value as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_three_radices() {
        assert_eq!(parse_number("0x00296C48").unwrap(), 0x0029_6C48);
        assert_eq!(parse_number("0X10").unwrap(), 16);
        assert_eq!(parse_number("6414").unwrap(), 6414);
        assert_eq!(parse_number("010").unwrap(), 8);
        assert_eq!(parse_number("0").unwrap(), 0);
    }

    #[test]
    fn keeps_the_cpp_quirks() {
        // Trailing junk is ignored by std::stoul.
        assert_eq!(parse_number("0x20 comments").unwrap(), 0x20);
        assert_eq!(parse_number("  42").unwrap(), 42);
        assert_eq!(parse_number("+7").unwrap(), 7);
    }

    #[test]
    fn rejects_invalid_input() {
        assert!(matches!(parse_number(""), Err(ParseNumberError::NoDigits { .. })));
        assert!(matches!(parse_number("zz"), Err(ParseNumberError::NoDigits { .. })));
        // "08" is octal with an invalid digit, which std::stoul also rejects.
        assert!(matches!(parse_number("08"), Err(ParseNumberError::NoDigits { .. })));
        assert!(matches!(parse_number("-1"), Err(ParseNumberError::Negative { .. })));
        assert!(matches!(parse_number("0x100000000"), Err(ParseNumberError::OutOfRange { .. })));
        // `std::stoul` does not accept digit separators: it stops at the `_`.
        assert_eq!(parse_number("0x1_0").unwrap(), 1);
    }
}
