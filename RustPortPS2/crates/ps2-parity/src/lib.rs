//! The parity gate: compare generated trees and read/write their manifests.
//!
//! The Rust recompiler is accepted only when it produces the same bytes as the
//! C++ one (see `docs/PARITY.md`). The generated C++ derives from the game and
//! must never be committed, so the gate commits a manifest (path, size, SHA-256
//! per file plus the report counters) and checks any tree against it.

pub mod fixture;
pub mod json;
pub mod sha256;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub use sha256::sha256_hex;

/// One file of a tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub size: u64,
    pub sha256: String,
}

/// The report counters the gate records next to the hashes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Counts {
    pub functions: u64,
    pub stubs: u64,
    pub skips: u64,
    pub unresolved: u64,
}

/// A parity manifest (`tests/parity/manifests/<elf-sha256>-<os>.json`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Manifest {
    pub generator: String,
    pub generator_version: String,
    pub operating_system: String,
    pub elf_sha256: String,
    pub files: BTreeMap<String, FileEntry>,
    pub counts: Counts,
}

/// Every regular file under `root`, by `/`-separated relative path.
pub fn collect_tree(root: &Path) -> io::Result<BTreeMap<String, FileEntry>> {
    let mut files = BTreeMap::new();
    let mut pending: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                pending.push(path);
                continue;
            }
            let data = fs::read(&path)?;
            let relative = path
                .strip_prefix(root)
                .map_err(io::Error::other)?
                .components()
                .map(|component| component.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            files.insert(relative, FileEntry { size: data.len() as u64, sha256: sha256_hex(&data) });
        }
    }
    Ok(files)
}

/// What differs between two sets of files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Comparison {
    pub only_in_a: Vec<String>,
    pub only_in_b: Vec<String>,
    pub different: Vec<String>,
    pub common: usize,
}

impl Comparison {
    pub fn is_identical(&self) -> bool {
        self.only_in_a.is_empty() && self.only_in_b.is_empty() && self.different.is_empty()
    }
}

pub fn compare_files(a: &BTreeMap<String, FileEntry>, b: &BTreeMap<String, FileEntry>) -> Comparison {
    let mut comparison = Comparison::default();
    for (path, entry) in a {
        match b.get(path) {
            None => comparison.only_in_a.push(path.clone()),
            Some(other) => {
                comparison.common += 1;
                if entry != other {
                    comparison.different.push(path.clone());
                }
            }
        }
    }
    comparison.only_in_b = b.keys().filter(|path| !a.contains_key(*path)).cloned().collect();
    comparison
}

/// The first line where two byte strings differ: `(line number, line of a, line of b)`.
pub fn first_difference(a: &[u8], b: &[u8]) -> Option<(usize, String, String)> {
    let mut lines_a = a.split(|&byte| byte == b'\n');
    let mut lines_b = b.split(|&byte| byte == b'\n');
    let mut number = 0usize;
    loop {
        number += 1;
        match (lines_a.next(), lines_b.next()) {
            (None, None) => return None,
            (Some(x), Some(y)) if x == y => {}
            (x, y) => {
                let show = |line: Option<&[u8]>| {
                    line.map_or_else(|| "<end of file>".to_owned(), |l| String::from_utf8_lossy(l).into_owned())
                };
                return Some((number, show(x), show(y)));
            }
        }
    }
}

impl Manifest {
    /// The manifest as the committed JSON (stable key and file order).
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        let _ = writeln!(out, "  \"generator\": {},", json::escape(&self.generator));
        let _ = writeln!(out, "  \"generator_version\": {},", json::escape(&self.generator_version));
        let _ = writeln!(out, "  \"operating_system\": {},", json::escape(&self.operating_system));
        let _ = writeln!(out, "  \"elf_sha256\": {},", json::escape(&self.elf_sha256));
        out.push_str("  \"files\": [\n");
        let mut first = true;
        for (path, entry) in &self.files {
            if !first {
                out.push_str(",\n");
            }
            first = false;
            let _ = write!(
                out,
                "    {{ \"path\": {}, \"size\": {}, \"sha256\": {} }}",
                json::escape(path),
                entry.size,
                json::escape(&entry.sha256)
            );
        }
        out.push_str("\n  ],\n");
        let _ = writeln!(
            out,
            "  \"counts\": {{ \"functions\": {}, \"stubs\": {}, \"skips\": {}, \"unresolved\": {} }}",
            self.counts.functions, self.counts.stubs, self.counts.skips, self.counts.unresolved
        );
        out.push_str("}\n");
        out
    }

    /// Parse a manifest written by [`Manifest::to_json`].
    pub fn from_json(text: &str) -> Result<Self, String> {
        let root = json::parse(text)?;
        let text_field = |key: &str| -> Result<String, String> {
            root.get(key).and_then(json::Json::as_str).map(str::to_owned).ok_or_else(|| format!("missing \"{key}\""))
        };

        let mut files = BTreeMap::new();
        for item in root.get("files").and_then(json::Json::as_array).ok_or("missing \"files\"")? {
            let path = item.get("path").and_then(json::Json::as_str).ok_or("file without \"path\"")?;
            let size = item.get("size").and_then(json::Json::as_u64).ok_or("file without \"size\"")?;
            let sha256 = item.get("sha256").and_then(json::Json::as_str).ok_or("file without \"sha256\"")?;
            files.insert(path.to_owned(), FileEntry { size, sha256: sha256.to_owned() });
        }

        let counts = root.get("counts").ok_or("missing \"counts\"")?;
        let count = |key: &str| counts.get(key).and_then(json::Json::as_u64).unwrap_or(0);

        Ok(Self {
            generator: text_field("generator")?,
            generator_version: text_field("generator_version")?,
            operating_system: text_field("operating_system")?,
            elf_sha256: text_field("elf_sha256")?,
            files,
            counts: Counts {
                functions: count("functions"),
                stubs: count("stubs"),
                skips: count("skips"),
                unresolved: count("unresolved"),
            },
        })
    }
}

/// Read the report counters out of a `ps2_recomp` log (`printSummary` output).
pub fn counts_from_log(log: &str) -> Counts {
    let mut counts = Counts::default();
    let number_after = |line: &str, label: &str| -> Option<u64> {
        let rest = &line[line.find(label)? + label.len()..];
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        digits.parse().ok()
    };
    for line in log.lines() {
        if let Some(value) = number_after(line, "Generated functions: ") {
            counts.functions = value;
        }
        if line.starts_with("Functions processed:") {
            counts.stubs = number_after(line, "stubs: ").unwrap_or(0);
            counts.skips = number_after(line, "skipped: ").unwrap_or(0);
        }
        if let Some(value) = number_after(line, "Indirect fallback promotions: ") {
            counts.unresolved = value;
        }
    }
    counts
}

/// The operating system name the manifests use.
pub fn current_operating_system() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}
