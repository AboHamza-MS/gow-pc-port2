//! Small helpers shared by the tools: CSV rows, Python-style number text, an insertion-ordered counter and a child
//! process with a deadline.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The game's recompiler configuration, function map and VU1 data, relative to the repository root.
pub const GAME_PROFILE: &str = "RustPortPS2/games/scus-97399";

/// Splits one CSV line (RFC 4180 quoting, no embedded newlines).
pub fn csv_row(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) if chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            ('"', _) => quoted = !quoted,
            (',', false) => fields.push(std::mem::take(&mut field)),
            _ => field.push(c),
        }
    }
    fields.push(field);
    fields
}

/// `int(text, 16)` (an optional `0x`, surrounding blanks, an optional sign).
pub fn parse_hex(text: &str) -> Option<i64> {
    let text = text.trim();
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let digits = digits.strip_prefix("0x").or_else(|| digits.strip_prefix("0X")).unwrap_or(digits);
    let value = i64::from_str_radix(digits, 16).ok()?;
    Some(if negative { -value } else { value })
}

/// `int(text, 0)`: `0x` hexadecimal, `0o` octal, `0b` binary or decimal.
pub fn parse_int_auto(text: &str) -> Option<i64> {
    let text = text.trim();
    let lower = text.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("0x") {
        i64::from_str_radix(rest, 16).ok()
    } else if let Some(rest) = lower.strip_prefix("0o") {
        i64::from_str_radix(rest, 8).ok()
    } else if let Some(rest) = lower.strip_prefix("0b") {
        i64::from_str_radix(rest, 2).ok()
    } else {
        text.parse().ok()
    }
}

/// `repr(float)`: always with a decimal point (`12.0`), shortest digits that round-trip.
pub fn py_float(value: f64) -> String {
    format!("{value:?}")
}

/// A mapping that remembers insertion order and counts, like `collections.Counter` on a `dict`.
#[derive(Default)]
pub struct Counter<K: std::hash::Hash + Eq + Clone> {
    order: Vec<K>,
    counts: HashMap<K, u64>,
}

impl<K: std::hash::Hash + Eq + Clone> Counter<K> {
    pub fn new() -> Self {
        Self { order: Vec::new(), counts: HashMap::new() }
    }

    pub fn add(&mut self, key: K, amount: u64) {
        match self.counts.get_mut(&key) {
            Some(count) => *count += amount,
            None => {
                self.order.push(key.clone());
                self.counts.insert(key, amount);
            }
        }
    }

    pub fn total(&self) -> u64 {
        self.counts.values().sum()
    }

    /// `Counter.most_common(n)`: by count, descending; equal counts keep insertion order.
    pub fn most_common(&self, n: usize) -> Vec<(K, u64)> {
        let mut items: Vec<(K, u64)> = self.order.iter().map(|k| (k.clone(), self.counts[k])).collect();
        items.sort_by_key(|item| std::cmp::Reverse(item.1));
        items.truncate(n);
        items
    }

    pub fn items(&self) -> impl Iterator<Item = (&K, u64)> {
        self.order.iter().map(|k| (k, self.counts[k]))
    }
}

/// The repository root: `--root` if given, otherwise the closest parent of the current directory that has
/// `RustPortPS2/games/scus-97399/funcmap.csv`.
pub fn find_root(explicit: Option<&str>) -> PathBuf {
    if let Some(root) = explicit {
        return PathBuf::from(root);
    }
    let start = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut directory: &Path = &start;
    loop {
        if directory.join(GAME_PROFILE).join("funcmap.csv").is_file() {
            return directory.to_path_buf();
        }
        match directory.parent() {
            Some(parent) => directory = parent,
            None => return start,
        }
    }
}

/// The first argument that is neither a `--flag` nor the value of one of the `valued` options.
pub fn first_positional<'a>(args: &'a [String], valued: &[&str]) -> Option<&'a String> {
    let mut skip = false;
    for arg in args {
        if skip {
            skip = false;
        } else if valued.contains(&arg.as_str()) {
            skip = true;
        } else if !arg.starts_with("--") {
            return Some(arg);
        }
    }
    None
}

/// The value after `--name`, if present.
pub fn option(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|arg| arg == name).and_then(|index| args.get(index + 1)).cloned()
}

pub fn flag(args: &[String], name: &str) -> bool {
    args.iter().any(|arg| arg == name)
}

/// Output of a finished child process.
pub struct Finished {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Runs `command` and kills it when `seconds` pass; `None` when it did not finish in time.
pub fn run_with_timeout(mut command: Command, seconds: u64) -> std::io::Result<Option<Finished>> {
    let mut child = command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let mut stdout = child.stdout.take().expect("piped");
    let mut stderr = child.stderr.take().expect("piped");
    let out = std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = stdout.read_to_end(&mut buffer);
        buffer
    });
    let err = std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = stderr.read_to_end(&mut buffer);
        buffer
    });
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let stdout = String::from_utf8_lossy(&out.join().unwrap_or_default()).into_owned();
    let stderr = String::from_utf8_lossy(&err.join().unwrap_or_default()).into_owned();
    Ok(status.map(|status| Finished { code: status.code().unwrap_or(-1), stdout, stderr }))
}

/// `html.escape(text)` with quotes.
pub fn html_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            _ => out.push(c),
        }
    }
    out
}

/// `str.splitlines()` for the line breaks the tools' logs use (`\n`, `\r\n`, `\r`).
pub fn split_lines(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut lines = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => {
                lines.push(&text[start..i]);
                start = i + 1;
            }
            b'\r' => {
                lines.push(&text[start..i]);
                if bytes.get(i + 1) == Some(&b'\n') {
                    i += 1;
                }
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}
