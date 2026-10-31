//! A minimal JSON reader/writer, enough for the parity manifests.
//!
//! The manifests only use objects, arrays, strings and non-negative integers, so
//! that is all this module understands; anything else is an error.

use std::collections::BTreeMap;
use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Json {
    Number(u64),
    String(String),
    Array(Vec<Json>),
    Object(BTreeMap<String, Json>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(map) => map.get(key),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Json::Number(number) => Some(*number),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Array(items) => Some(items),
            _ => None,
        }
    }
}

pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

struct Parser<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl Parser<'_> {
    fn skip_space(&mut self) {
        while self.position < self.bytes.len() && self.bytes[self.position].is_ascii_whitespace() {
            self.position += 1;
        }
    }

    fn error(&self, message: &str) -> String {
        format!("{message} at byte {}", self.position)
    }

    fn expect(&mut self, byte: u8) -> Result<(), String> {
        self.skip_space();
        if self.bytes.get(self.position) == Some(&byte) {
            self.position += 1;
            Ok(())
        } else {
            Err(self.error(&format!("expected '{}'", byte as char)))
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            let Some(&byte) = self.bytes.get(self.position) else {
                return Err(self.error("unterminated string"));
            };
            self.position += 1;
            match byte {
                b'"' => return Ok(out),
                b'\\' => {
                    let Some(&escaped) = self.bytes.get(self.position) else {
                        return Err(self.error("unterminated escape"));
                    };
                    self.position += 1;
                    match escaped {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let digits = self
                                .bytes
                                .get(self.position..self.position + 4)
                                .and_then(|slice| std::str::from_utf8(slice).ok())
                                .and_then(|text| u32::from_str_radix(text, 16).ok())
                                .and_then(char::from_u32)
                                .ok_or_else(|| self.error("bad \\u escape"))?;
                            self.position += 4;
                            out.push(digits);
                        }
                        _ => return Err(self.error("unknown escape")),
                    }
                }
                other => {
                    // Copy a whole UTF-8 sequence.
                    let start = self.position - 1;
                    let len = match other {
                        0x00..=0x7F => 1,
                        0xC0..=0xDF => 2,
                        0xE0..=0xEF => 3,
                        _ => 4,
                    };
                    let end = (start + len).min(self.bytes.len());
                    out.push_str(std::str::from_utf8(&self.bytes[start..end]).map_err(|_| self.error("bad utf-8"))?);
                    self.position = end;
                }
            }
        }
    }

    fn value(&mut self) -> Result<Json, String> {
        self.skip_space();
        match self.bytes.get(self.position) {
            Some(b'{') => {
                self.position += 1;
                let mut map = BTreeMap::new();
                self.skip_space();
                if self.bytes.get(self.position) == Some(&b'}') {
                    self.position += 1;
                    return Ok(Json::Object(map));
                }
                loop {
                    let key = self.string()?;
                    self.expect(b':')?;
                    let value = self.value()?;
                    map.insert(key, value);
                    self.skip_space();
                    match self.bytes.get(self.position) {
                        Some(b',') => self.position += 1,
                        Some(b'}') => {
                            self.position += 1;
                            return Ok(Json::Object(map));
                        }
                        _ => return Err(self.error("expected ',' or '}'")),
                    }
                }
            }
            Some(b'[') => {
                self.position += 1;
                let mut items = Vec::new();
                self.skip_space();
                if self.bytes.get(self.position) == Some(&b']') {
                    self.position += 1;
                    return Ok(Json::Array(items));
                }
                loop {
                    items.push(self.value()?);
                    self.skip_space();
                    match self.bytes.get(self.position) {
                        Some(b',') => self.position += 1,
                        Some(b']') => {
                            self.position += 1;
                            return Ok(Json::Array(items));
                        }
                        _ => return Err(self.error("expected ',' or ']'")),
                    }
                }
            }
            Some(b'"') => Ok(Json::String(self.string()?)),
            Some(byte) if byte.is_ascii_digit() => {
                let start = self.position;
                while self.bytes.get(self.position).is_some_and(u8::is_ascii_digit) {
                    self.position += 1;
                }
                std::str::from_utf8(&self.bytes[start..self.position])
                    .ok()
                    .and_then(|text| text.parse().ok())
                    .map(Json::Number)
                    .ok_or_else(|| self.error("bad number"))
            }
            _ => Err(self.error("unexpected character")),
        }
    }
}

/// Parse a complete JSON document.
pub fn parse(text: &str) -> Result<Json, String> {
    let mut parser = Parser { bytes: text.as_bytes(), position: 0 };
    let value = parser.value()?;
    parser.skip_space();
    if parser.position != parser.bytes.len() {
        return Err(parser.error("trailing characters"));
    }
    Ok(value)
}
