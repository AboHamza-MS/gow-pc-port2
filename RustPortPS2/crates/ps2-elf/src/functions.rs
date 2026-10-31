//! Function boundary extraction.
//!
//! One-to-one port of `ElfParser::extractFunctions` from
//! `PS2Recomp/ps2xRecomp/src/lib/elf_parser.cpp`. The input is the symbol table
//! plus the "extra" functions (the Ghidra map, and later the debug/heuristic
//! scan), and the output is the ordered function list the code generator walks.
//!
//! Two rules drive the result and are easy to get wrong:
//!
//! - a symbol with a size, or a *named* extra with an explicit range, is
//!   **authoritative** for its start address and its end wins;
//! - an unnamed/auto-generated candidate that starts *inside* an authoritative
//!   range is dropped, because it is an interior label, not a function.
//!
//! When a Ghidra map is loaded, the C++ side also drops auto-generated
//! symbols that the map does not contain, and the heuristic scanner's
//! `sub_XXXXXXXX` entries are erased (`loadGhidraFunctionMap`), so with a map
//! the extras are the map itself. The heuristic and DWARF extras are ported
//! separately; they only matter when no map is loaded.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::funcmap::{clamp_function_end, is_auto_generated_name, FunctionMap};
use crate::ElfFile;

/// One recompiled function, mirroring `ps2recomp::Function` (minus
/// `isRecompiled`, which the C++ side sets during `recompile`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function {
    pub name: String,
    pub start: u32,
    pub end: u32,
    pub is_stub: bool,
    pub is_skipped: bool,
}

/// The extras the map contributes: one function per accepted boundary.
pub fn extras_from_map(map: &FunctionMap) -> Vec<Function> {
    map.entries()
        .iter()
        .map(|entry| Function {
            name: entry.name.clone(),
            start: entry.start,
            end: entry.end,
            is_stub: false,
            is_skipped: false,
        })
        .collect()
}

/// `extractFunctions` for the common case: the map is the only extra source.
pub fn extract_functions_with_map(elf: &ElfFile<'_>, map: &FunctionMap) -> Vec<Function> {
    let extras = extras_from_map(map);
    extract_functions(elf, &extras, Some(map))
}

/// Merge symbols and `extras` into the final, address-ordered function list.
///
/// `map` changes one thing: auto-generated symbols that the map does not list
/// are skipped, because the map is the better source of boundaries.
pub fn extract_functions(elf: &ElfFile<'_>, extras: &[Function], map: Option<&FunctionMap>) -> Vec<Function> {
    let has_map = map.is_some_and(|map| !map.is_empty());
    let map_starts: BTreeSet<u32> = match map {
        Some(map) if has_map => map.entries().iter().map(|entry| entry.start).collect(),
        _ => BTreeSet::new(),
    };

    // 1. Authoritative ends by start address.
    let mut authoritative: BTreeMap<u32, u32> = BTreeMap::new();
    for symbol in elf.symbols() {
        if !symbol.is_function() || symbol.is_imported() || symbol.size == 0 {
            continue;
        }
        if has_map && is_auto_generated_name(&symbol.name) && !map_starts.contains(&symbol.address) {
            continue;
        }
        let Some(section) = elf.find_function_section_by_address(symbol.address) else {
            continue;
        };
        let end = clamp_function_end(section, symbol.address, symbol_end(symbol.address, symbol.size));
        if end <= symbol.address {
            continue;
        }
        let entry = authoritative.entry(symbol.address).or_insert(end);
        if end > *entry {
            *entry = end;
        }
    }

    for extra in extras {
        if extra.start == 0 || extra.end <= extra.start || extra.name.is_empty() || is_auto_generated_name(&extra.name)
        {
            continue;
        }
        let Some(section) = elf.find_function_section_by_address(extra.start) else {
            continue;
        };
        let end = clamp_function_end(section, extra.start, extra.end);
        if end <= extra.start {
            continue;
        }
        let entry = authoritative.entry(extra.start).or_insert(end);
        if end > *entry {
            *entry = end;
        }
    }

    let ranges: Vec<(u32, u32)> = authoritative.iter().map(|(start, end)| (*start, *end)).collect();

    // 2. Insert every symbol, then every extra, merging by start address.
    let mut state = MergeState {
        elf,
        authoritative: &authoritative,
        ranges: &ranges,
        functions: Vec::with_capacity(elf.symbols().len() + extras.len()),
        index_by_start: HashMap::new(),
    };
    for symbol in elf.symbols() {
        if !symbol.is_function() || symbol.is_imported() {
            continue;
        }
        if elf.find_function_section_by_address(symbol.address).is_none() {
            continue;
        }
        state.add_or_merge(Function {
            name: symbol.name.clone(),
            start: symbol.address,
            end: if symbol.size > 0 { symbol_end(symbol.address, symbol.size) } else { 0 },
            is_stub: false,
            is_skipped: false,
        });
    }
    for extra in extras {
        state.add_or_merge(extra.clone());
    }

    // 3. Order by address and settle the ends that are still open.
    let mut functions = state.functions;
    functions.sort_by_key(|function| function.start);
    finish_ends(elf, &authoritative, &mut functions);
    functions
}

fn symbol_end(address: u32, size: u32) -> u32 {
    let end = u64::from(address) + u64::from(size);
    if end > u64::from(u32::MAX) {
        u32::MAX
    } else {
        end as u32
    }
}

/// `isInsideAuthoritativeRange`: is `start` strictly inside a range?
fn is_inside_authoritative_range(ranges: &[(u32, u32)], start: u32) -> bool {
    if ranges.is_empty() {
        return false;
    }
    // First range that starts after `start`, then the one before it.
    let index = ranges.partition_point(|range| range.0 <= start);
    if index == 0 {
        return false;
    }
    let range = ranges[index - 1];
    start > range.0 && start < range.1
}

struct MergeState<'a, 'b> {
    elf: &'a ElfFile<'b>,
    authoritative: &'a BTreeMap<u32, u32>,
    ranges: &'a [(u32, u32)],
    functions: Vec<Function>,
    index_by_start: HashMap<u32, usize>,
}

impl MergeState<'_, '_> {
    fn add_or_merge(&mut self, new_function: Function) {
        if new_function.start == 0 {
            return;
        }
        if self.elf.find_function_section_by_address(new_function.start).is_none() {
            return;
        }

        let inside_authoritative = is_inside_authoritative_range(self.ranges, new_function.start);
        let has_own_authoritative = self.authoritative.contains_key(&new_function.start);
        let has_auto_name = new_function.name.is_empty() || is_auto_generated_name(&new_function.name);

        // An interior label is not a function.
        if inside_authoritative && (!has_own_authoritative || has_auto_name) {
            return;
        }

        match self.index_by_start.get(&new_function.start).copied() {
            None => {
                let start = new_function.start;
                self.functions.push(new_function);
                self.index_by_start.insert(start, self.functions.len() - 1);
                self.apply_authoritative_end(self.functions.len() - 1);
            }
            Some(index) => {
                let new_name = new_function.name.clone();
                let existing = &mut self.functions[index];
                if !new_name.is_empty()
                    && (existing.name.is_empty()
                        || (is_auto_generated_name(&existing.name) && !is_auto_generated_name(&new_name)))
                {
                    existing.name = new_name;
                }
                existing.is_stub |= new_function.is_stub;
                existing.is_skipped |= new_function.is_skipped;

                if self.authoritative.contains_key(&existing.start) {
                    self.apply_authoritative_end(index);
                } else if new_function.end > self.functions[index].end {
                    self.functions[index].end = new_function.end;
                }
            }
        }
    }

    fn apply_authoritative_end(&mut self, index: usize) {
        let start = self.functions[index].start;
        let Some(end) = self.authoritative.get(&start).copied() else {
            return;
        };
        let end = match self.elf.find_function_section_by_address(start) {
            Some(section) => clamp_function_end(section, start, end),
            None => end,
        };
        self.functions[index].end = end;
    }
}

/// `extractFunctions`' last pass: authoritative ends win, otherwise an open end
/// becomes the next function's start (same section) or the section end.
fn finish_ends(elf: &ElfFile<'_>, authoritative: &BTreeMap<u32, u32>, functions: &mut [Function]) {
    for index in 0..functions.len() {
        let start = functions[index].start;
        if let Some(end) = authoritative.get(&start).copied() {
            functions[index].end = end;
            continue;
        }
        if functions[index].end > start {
            continue;
        }

        let section = elf.find_function_section_by_address(start);
        let section_end = match section {
            Some(section) => section.address.wrapping_add(section.size),
            None => start.wrapping_add(4),
        };
        let next_start = match functions.get(index + 1).map(|function| function.start) {
            Some(candidate) if section.is_some() && candidate > start && candidate < section_end => candidate,
            _ => section_end,
        };
        functions[index].end = if next_start > start { next_start } else { start.wrapping_add(4) };
    }
}
