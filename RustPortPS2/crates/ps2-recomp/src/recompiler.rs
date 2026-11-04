//! The recompiler driver.
//!
//! Port of `PS2Recompiler` (`ps2_recompiler.cpp`): load the configuration and
//! the ELF, decode every function, discover the extra entry points, and write
//! the generated tree (one file per function, the two headers and the
//! registration file).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use ps2_config::RecompilerConfig;
use ps2_elf::{parse_number, ElfFile, FunctionMap, Relocation};
use ps2_r5900::opcodes::*;
use ps2_r5900::Instruction;

use crate::calls::{is_stub_name, resolve_stub_name, resolve_stub_target, resolve_syscall_name, StubTarget};
use crate::emit::function::emit_function;
use crate::emit::function_table::emit_function_table;
use crate::generator::{
    is_cxx_keyword, is_reserved_cxx_identifier, sanitize_identifier_body, BootstrapInfo, CodeGenerator, GenerationError,
};
use crate::reporter::Reporter;
use crate::types::{ElfTables, Function, Section, Symbol};

type DecodedFunctions = HashMap<u32, Vec<Instruction>>;

/// Failure of one of the pipeline stages.
#[derive(Debug)]
pub struct RecompileError(pub String);

impl std::fmt::Display for RecompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RecompileError {}

impl From<GenerationError> for RecompileError {
    fn from(error: GenerationError) -> Self {
        Self(error.0)
    }
}

/// `operator<<(std::ostream&, const std::filesystem::path&)`: the path in double quotes,
/// with `"` and `\` escaped by a backslash.
fn quoted_path(path: &Path) -> String {
    let text = path.display().to_string();
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        if c == '"' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

fn should_generate_code_for_function(function: &Function) -> bool {
    function.is_recompiled || function.is_stub || function.is_skipped
}

fn resolve_output_worker_count(configured: u32) -> usize {
    if configured > 0 {
        return configured as usize;
    }
    match std::thread::available_parallelism() {
        Ok(count) if count.get() >= 2 => count.get() - 1,
        _ => 1,
    }
}

fn write_combined_output_preamble(out: &mut String) {
    out.push_str("#include <stdexcept>\n");
    out.push_str("#include <ps2_recompiled_functions.h>\n\n");
    out.push_str("#include \"ps2_runtime_macros.h\"\n");
    out.push_str("#include \"ps2_runtime.h\"\n");
    out.push_str("#include <ps2_recompiled_stubs.h>\n");
    out.push_str("#include \"ps2_syscalls.h\"\n");
    out.push_str("#include \"ps2_stubs.h\"\n");
    out.push_str("#ifdef _DEBUG\n");
    out.push_str("#include \"ps2_log.h\"\n");
    out.push_str("#endif\n");
    out.push('\n');
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PatchClass {
    Generic,
    Syscall,
    Cop0,
    Cache,
}

fn classify_patched_instruction(raw: u32) -> PatchClass {
    let opcode = (raw >> 26) & 0x3F;
    if opcode == OPCODE_SPECIAL && (raw & 0x3F) == SPECIAL_SYSCALL {
        PatchClass::Syscall
    } else if opcode == OPCODE_COP0 {
        PatchClass::Cop0
    } else if opcode == OPCODE_CACHE {
        PatchClass::Cache
    } else {
        PatchClass::Generic
    }
}

fn should_apply_configured_patch(class: PatchClass, config: &RecompilerConfig) -> bool {
    match class {
        PatchClass::Syscall => config.patch_syscalls,
        PatchClass::Cop0 => config.patch_cop0,
        PatchClass::Cache => config.patch_cache,
        PatchClass::Generic => true,
    }
}

fn escape_c_string_literal(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            other => escaped.push(other),
        }
    }
    escaped
}

/// `std::isspace` in the "C" locale.
fn is_c_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r')
}

fn trim_ascii_whitespace(value: &str) -> &str {
    value.trim_matches(is_c_space)
}

/// `tryParseU32AddressLiteral`: `std::stoul(literal, &count, 0)` that must consume the
/// whole literal and fit in 32 bits (`unsigned long` is 32 bits on the Windows build).
fn try_parse_u32_address_literal(literal: &str) -> Option<u32> {
    if literal.is_empty() {
        return None;
    }
    let rest = literal.strip_prefix('+').unwrap_or(literal);
    let (radix, digits) = if let Some(hex) = rest.strip_prefix("0x").or_else(|| rest.strip_prefix("0X")) {
        (16, hex)
    } else if rest.len() > 1 && rest.starts_with('0') {
        (8, &rest[1..])
    } else {
        (10, rest)
    };
    if digits.is_empty() {
        // "0x" alone parses the leading "0" and leaves the "x" unconsumed.
        return None;
    }
    u32::from_str_radix(digits, radix).ok()
}

#[derive(Default)]
struct FunctionSelector {
    name: String,
    start: Option<u32>,
}

fn parse_function_selector(raw_selector: &str) -> FunctionSelector {
    let mut selector = FunctionSelector::default();
    let trimmed = trim_ascii_whitespace(raw_selector);
    if trimmed.is_empty() {
        return selector;
    }

    if let Some(at) = trimmed.rfind('@') {
        selector.name = trim_ascii_whitespace(&trimmed[..at]).to_owned();

        let address_literal = trim_ascii_whitespace(&trimmed[at + 1..]);
        if let Some(address) = try_parse_u32_address_literal(address_literal) {
            selector.start = Some(address);
            return selector;
        }

        // for now backward compatibility
        selector.name = trimmed.to_owned();
        return selector;
    }

    if let Some(address) = try_parse_u32_address_literal(trimmed) {
        selector.start = Some(address);
        return selector;
    }

    selector.name = trimmed.to_owned();
    selector
}

fn is_entry_function_name(name: &str) -> bool {
    name.starts_with("entry_")
}

fn is_correctness_critical_function_name(name: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "__ct__",
        "__sinit_",
        "_GLOBAL__sub_I_",
        "GLOBAL__sub_I_",
        "__static_initialization_and_destruction_0",
        "__do_global_ctors",
    ];
    PREFIXES.iter().any(|prefix| name.starts_with(prefix))
}

/// The innermost function that really holds an instruction at `address`.
///
/// `skip_entries` mirrors the three C++ lambdas: the entry-discovery pass accepts
/// `entry_*` containers, the reslice and registration passes do not.
fn find_containing_function(
    functions: &[Function],
    decoded: &DecodedFunctions,
    address: u32,
    skip_entries: bool,
) -> Option<usize> {
    let mut best: Option<usize> = None;
    for (index, function) in functions.iter().enumerate() {
        if !function.is_recompiled || function.is_stub || function.is_skipped {
            continue;
        }
        if skip_entries && is_entry_function_name(&function.name) {
            continue;
        }
        if address < function.start || address >= function.end {
            continue;
        }
        let Some(instructions) = decoded.get(&function.start) else {
            continue;
        };
        if !instructions.iter().any(|candidate| candidate.address == address) {
            continue;
        }
        if best.is_none_or(|best| function.start > functions[best].start) {
            best = Some(index);
        }
    }
    best
}

/// `decodeFunction`.
fn decode_function(
    elf: &ElfFile<'_>,
    config: &RecompilerConfig,
    reporter: &Reporter,
    function: &mut Function,
    decoded: &mut DecodedFunctions,
) -> bool {
    let mut instructions: Vec<Instruction> = Vec::new();
    let mut truncated = false;

    let start = function.start;
    let end = function.end;

    let mut address = start;
    while address < end {
        if !elf.is_valid_address(address) {
            let message = format!("Invalid address 0x{address:x} (truncating decode)");
            reporter.warning_at("decode", &function.name, address, &message);
            truncated = true;
            break;
        }

        let Some(mut raw_instruction) = elf.read_word(address) else {
            let message =
                format!("Error decoding instruction: Invalid address for readWord: {address} (truncating decode)");
            reporter.warning_at("decode", &function.name, address, &message);
            truncated = true;
            break;
        };
        let original_instruction = raw_instruction;

        if let Some(patch) = config.patches.get(&address) {
            let class = classify_patched_instruction(original_instruction);
            if should_apply_configured_patch(class, config) {
                match parse_number(patch) {
                    Ok(value) => {
                        raw_instruction = value;
                        reporter.info("patch", &format!("Applied patch at 0x{address:x}"));
                    }
                    Err(error) => {
                        let message = format!(
                            "Invalid patch value at 0x{address:x} ({patch}): {error}. Using original instruction."
                        );
                        reporter.warning_at("patch", &function.name, address, &message);
                    }
                }
            }
        }

        let mut inst = ps2_r5900::decode(address, raw_instruction, !config.low_memory_mode);

        if let Some(&mmio) = config.mmio_by_instruction_address.get(&address) {
            inst.is_mmio = true;
            inst.mmio_address = mmio;
        }

        instructions.push(inst);
        address = address.wrapping_add(4);
    }

    if instructions.is_empty() {
        let message = format!("No decodable instructions found at 0x{:x}", function.start);
        reporter.warning_at("decode", &function.name, function.start, &message);
        return false;
    }

    if truncated {
        function.end = instructions.last().map_or(function.end, |last| last.address.wrapping_add(4));
    }

    decoded.insert(function.start, instructions);
    true
}

#[derive(Default)]
struct EntryDiscoveryStats {
    discovered_count: usize,
    pass_count: usize,
}

struct PendingEntry {
    target: u32,
    containing_start: Option<u32>,
    containing_end: u32,
}

/// `discoverAdditionalEntryPointsImpl`.
fn discover_additional_entry_points_impl(
    functions: &mut Vec<Function>,
    decoded: &mut DecodedFunctions,
    sections: &[Section],
    code_generator: Option<&CodeGenerator>,
    decode_external: &mut dyn FnMut(&mut Function, &mut DecodedFunctions) -> bool,
    seed_entry_addresses: &BTreeSet<u32>,
) -> EntryDiscoveryStats {
    let mut existing_starts: BTreeSet<u32> = functions.iter().map(|f| f.start).collect();

    let is_executable_address =
        |address: u32| sections.iter().any(|section| section.is_code && section.contains(address));
    let executable_section_end = |address: u32| -> Option<u32> {
        sections
            .iter()
            .find(|section| section.is_code && section.contains(address))
            .map(|section| section.address.wrapping_add(section.size))
    };

    let mut stats = EntryDiscoveryStats::default();

    loop {
        stats.pass_count += 1;

        let mut pending_entries: Vec<PendingEntry> = Vec::new();
        let mut new_entries: Vec<Function> = Vec::new();
        let mut pending_starts: BTreeSet<u32> = BTreeSet::new();

        let mut queue_pending_entry =
            |target: u32, functions: &[Function], decoded: &DecodedFunctions, existing_starts: &BTreeSet<u32>| {
                if !is_executable_address(target) {
                    return;
                }
                if existing_starts.contains(&target) || pending_starts.contains(&target) {
                    return;
                }

                let mut pending = PendingEntry { target, containing_start: None, containing_end: 0 };
                if let Some(index) = find_containing_function(functions, decoded, target, false) {
                    pending.containing_start = Some(functions[index].start);
                    pending.containing_end = functions[index].end;
                }

                pending_entries.push(pending);
                pending_starts.insert(target);
            };

        if stats.pass_count == 1 {
            for &target in seed_entry_addresses {
                queue_pending_entry(target, functions, decoded, &existing_starts);
            }
        }

        for function in functions.iter() {
            if !function.is_recompiled || function.is_stub || function.is_skipped {
                continue;
            }
            if is_entry_function_name(&function.name) {
                continue;
            }
            let Some(instructions) = decoded.get(&function.start) else {
                continue;
            };

            let analysis = match code_generator {
                Some(generator) => generator.collect_internal_branch_targets(function, instructions, Some(functions)),
                None => Default::default(),
            };

            for &target in &analysis.external_entry_points {
                queue_pending_entry(target, functions, decoded, &existing_starts);
            }
            for &target in &analysis.resume_entry_points {
                queue_pending_entry(target, functions, decoded, &existing_starts);
            }
        }

        if pending_entries.is_empty() {
            break;
        }

        pending_entries.sort_by_key(|pending| pending.target);

        let mut boundary_starts: Vec<u32> = existing_starts.iter().copied().collect();
        boundary_starts.extend(pending_starts.iter().copied());
        boundary_starts.sort_unstable();
        boundary_starts.dedup();

        let find_next_boundary_start = |address: u32| -> Option<u32> {
            let index = boundary_starts.partition_point(|&start| start <= address);
            boundary_starts.get(index).copied()
        };

        let mut successful_starts: BTreeSet<u32> = BTreeSet::new();

        for pending in &pending_entries {
            let target = pending.target;

            let mut entry_function = Function {
                name: format!("entry_{target:x}"),
                start: target,
                end: 0,
                is_recompiled: true,
                is_stub: false,
                is_skipped: false,
            };

            if let Some(containing_start) = pending.containing_start {
                let Some(containing_instructions) = decoded.get(&containing_start) else {
                    continue;
                };
                let Some(slice_begin) = containing_instructions.iter().position(|c| c.address == target) else {
                    continue;
                };

                let mut slice_end_address = pending.containing_end;
                if let Some(next_start) = find_next_boundary_start(target) {
                    if next_start < slice_end_address {
                        slice_end_address = next_start;
                    }
                }

                let first = &containing_instructions[slice_begin];
                let is_simple_return_thunk_start =
                    first.opcode == OPCODE_SPECIAL && first.function == SPECIAL_JR && first.rs == 31;
                if is_simple_return_thunk_start && target <= u32::MAX - 8 {
                    let return_thunk_end = target + 8;
                    if return_thunk_end < slice_end_address {
                        slice_end_address = return_thunk_end;
                    }
                }

                if slice_end_address <= target {
                    continue;
                }

                let slice_end = containing_instructions[slice_begin..]
                    .iter()
                    .position(|c| c.address >= slice_end_address)
                    .map_or(containing_instructions.len(), |offset| slice_begin + offset);
                if slice_end == slice_begin {
                    continue;
                }

                let sliced: Vec<Instruction> = containing_instructions[slice_begin..slice_end].to_vec();
                decoded.insert(target, sliced);
                entry_function.end = slice_end_address;
            } else {
                let Some(section_end) = executable_section_end(target) else {
                    continue;
                };

                let mut entry_end = section_end;
                if let Some(next_start) = find_next_boundary_start(target) {
                    if next_start < entry_end {
                        entry_end = next_start;
                    }
                }
                if entry_end <= target {
                    continue;
                }

                entry_function.end = entry_end;
                if !decode_external(&mut entry_function, decoded) {
                    continue;
                }
            }

            new_entries.push(entry_function);
            successful_starts.insert(target);
        }

        if new_entries.is_empty() {
            break;
        }

        stats.discovered_count += new_entries.len();
        functions.extend(new_entries);
        existing_starts.extend(successful_starts);
        functions.sort_by_key(|function| function.start);
    }

    stats
}

/// `collectInternalEntryTargetsImpl`.
fn collect_internal_entry_targets_impl(
    functions: &[Function],
    decoded: &DecodedFunctions,
    entry_addresses: &BTreeSet<u32>,
    targets_by_owner: &mut BTreeMap<u32, Vec<u32>>,
) -> usize {
    let function_starts: HashSet<u32> = functions.iter().map(|f| f.start).collect();

    let mut added = 0usize;
    for &entry_address in entry_addresses {
        if function_starts.contains(&entry_address) {
            continue;
        }

        let mut owner: Option<&Function> = None;
        for function in functions {
            if !function.is_recompiled
                || function.is_stub
                || function.is_skipped
                || entry_address <= function.start
                || entry_address >= function.end
            {
                continue;
            }
            let Some(instructions) = decoded.get(&function.start) else {
                continue;
            };
            if !instructions.iter().any(|instruction| instruction.address == entry_address) {
                continue;
            }
            if owner.is_none_or(|owner| function.start > owner.start) {
                owner = Some(function);
            }
        }

        let Some(owner) = owner else {
            continue;
        };

        let targets = targets_by_owner.entry(owner.start).or_default();
        if !targets.contains(&entry_address) {
            targets.push(entry_address);
            added += 1;
        }
    }

    added
}

/// `PS2Recompiler::clampFilenameLength`.
pub fn clamp_filename_length(base_name: &str, extension: &str, max_length: usize) -> String {
    if max_length == 0 {
        // Better go over the limit than create files with an empty path.
        return format!("{base_name}{extension}");
    }

    if base_name.len() + extension.len() <= max_length {
        return format!("{base_name}{extension}");
    }

    let mut name_part = base_name.to_owned();
    let mut preserved_suffix = String::new();

    if let Some(suffix_pos) = name_part.rfind("_0x") {
        preserved_suffix = name_part[suffix_pos..].to_owned();
        name_part.truncate(suffix_pos);
    }

    // `size_t` arithmetic in the C++: a too-small limit wraps around instead of clamping to zero.
    let available = max_length.wrapping_sub(extension.len()).wrapping_sub(preserved_suffix.len());

    if available == 0 {
        return format!("{preserved_suffix}{extension}");
    }

    if name_part.len() > available {
        // The names are ASCII after sanitizing, so a byte cut is a character cut.
        let mut cut = available;
        while !name_part.is_char_boundary(cut) {
            cut -= 1;
        }
        name_part.truncate(cut);
    }

    format!("{name_part}{preserved_suffix}{extension}")
}

/// `PS2Recompiler::sanitizeFunctionName` (note: differs from the generator's).
fn sanitize_function_name(name: &str) -> String {
    let sanitized = sanitize_identifier_body(name);
    if sanitized.is_empty() {
        return sanitized;
    }
    if sanitized == "main" {
        return "ps2_main".to_owned();
    }
    if is_cxx_keyword(&sanitized) || is_reserved_cxx_identifier(&sanitized) {
        return format!("ps2_{sanitized}");
    }
    sanitized
}

/// `PS2Recompiler`.
pub struct Recompiler<'a> {
    config: RecompilerConfig,
    reporter: Arc<Reporter>,
    elf: ElfFile<'a>,
    code_generator: CodeGenerator,

    functions: Vec<Function>,
    symbols: Vec<Symbol>,
    sections: Arc<Vec<Section>>,
    relocations: Vec<Relocation>,

    decoded_functions: DecodedFunctions,
    skip_functions: HashSet<String>,
    skip_function_starts: HashSet<u32>,
    stub_functions: HashSet<String>,
    stub_function_starts: BTreeSet<u32>,
    stub_handler_bindings_by_start: HashMap<u32, String>,
    entry_point_hint_starts: BTreeSet<u32>,
    correctness_critical_function_starts: BTreeSet<u32>,
    generated_stubs: BTreeMap<u32, String>,
    function_renames: HashMap<u32, String>,
    resume_entry_targets_by_owner: BTreeMap<u32, Vec<u32>>,
    bootstrap_info: BootstrapInfo,
}

impl<'a> Recompiler<'a> {
    /// `initialize`: parse the configuration's selectors, the ELF and the function map.
    pub fn initialize(
        config: RecompilerConfig,
        config_path: &str,
        config_warnings: &[String],
        elf_bytes: &'a [u8],
        reporter: Arc<Reporter>,
    ) -> Result<Self, RecompileError> {
        reporter.info("config", &format!("Parsing toml file: {config_path}"));
        for warning in config_warnings {
            reporter.warning("config", warning);
        }

        let mut skip_functions = HashSet::new();
        let mut skip_function_starts = HashSet::new();
        let mut stub_functions = HashSet::new();
        let mut stub_function_starts = BTreeSet::new();
        let mut stub_handler_bindings_by_start: HashMap<u32, String> = HashMap::new();
        let mut entry_point_hint_starts = BTreeSet::new();

        reporter.progress("parsing config");

        for name in &config.skip_functions {
            let selector = parse_function_selector(name);
            if !selector.name.is_empty() {
                skip_functions.insert(selector.name.clone());
            }
            if let Some(start) = selector.start {
                skip_function_starts.insert(start);
            }
        }
        for name in &config.stub_implementations {
            let selector = parse_function_selector(name);
            if !selector.name.is_empty() {
                stub_functions.insert(selector.name.clone());
            }
            if let Some(start) = selector.start {
                stub_function_starts.insert(start);
                if !selector.name.is_empty() {
                    if let Some(previous) = stub_handler_bindings_by_start.get(&start) {
                        if *previous != selector.name {
                            reporter.warning(
                                "config",
                                &format!(
                                    "Multiple stub handler bindings for 0x{start:x} (keeping latest '{}', previous '{previous}')",
                                    selector.name
                                ),
                            );
                        }
                    }
                    stub_handler_bindings_by_start.insert(start, selector.name.clone());
                }
            }
        }
        for hint in &config.entry_point_hints {
            let selector = parse_function_selector(hint);
            if let Some(start) = selector.start {
                entry_point_hint_starts.insert(start);
            }
        }

        reporter.progress("parsing ELF");
        let elf = ElfFile::parse(elf_bytes).map_err(|error| {
            reporter.error("elf", &format!("Failed to parse ELF file: {}", config.input_path));
            RecompileError(format!("{error}"))
        })?;
        for warning in elf.warnings() {
            reporter.warning("elf", warning);
        }

        let mut function_map: Option<FunctionMap> = None;
        if !config.ghidra_map_path.is_empty() {
            match std::fs::read_to_string(&config.ghidra_map_path) {
                Ok(text) => {
                    let map = FunctionMap::from_ghidra_csv(&text, &elf);
                    if map.is_empty() {
                        reporter.warning(
                            "ghidra-map",
                            &format!(
                                "Loaded 0 functions from Ghidra map after filtering ({} non-executable, {} invalid range).",
                                map.rejected_outside_executable(),
                                map.rejected_invalid_range()
                            ),
                        );
                    } else {
                        reporter.info("ghidra-map", &format!("Loaded {} functions from Ghidra map", map.len()));
                        if map.rejected_outside_executable() > 0 {
                            reporter.warning(
                                "ghidra-map",
                                &format!(
                                    "Ignored {} Ghidra function(s) outside executable sections.",
                                    map.rejected_outside_executable()
                                ),
                            );
                        }
                        if map.rejected_invalid_range() > 0 {
                            reporter.warning(
                                "ghidra-map",
                                &format!(
                                    "Ignored {} Ghidra function(s) with invalid ranges after section clamping.",
                                    map.rejected_invalid_range()
                                ),
                            );
                        }
                    }
                    function_map = Some(map);
                }
                Err(_) => {
                    reporter.warning(
                        "ghidra-map",
                        &format!("Could not open Ghidra function map: {}", config.ghidra_map_path),
                    );
                }
            }
        }

        let ps2_functions = match &function_map {
            Some(map) => ps2_elf::extract_functions_with_map(&elf, map),
            None => {
                // No map: the heuristic scan is the only source of function starts.
                let extras = crate::scan::scan_function_starts(&elf);
                ps2_elf::extract_functions(&elf, &extras, None)
            }
        };
        let functions: Vec<Function> = ps2_functions.iter().map(Function::from).collect();
        let tables = ElfTables::from_elf(&elf);
        let relocations: Vec<Relocation> = elf.relocations().to_vec();

        let mut recompiler = Self {
            config,
            reporter: reporter.clone(),
            code_generator: CodeGenerator::new(&tables.symbols, Arc::new(tables.sections.clone())),
            elf,
            functions,
            symbols: tables.symbols,
            sections: Arc::new(tables.sections),
            relocations,
            decoded_functions: HashMap::new(),
            skip_functions,
            skip_function_starts,
            stub_functions,
            stub_function_starts,
            stub_handler_bindings_by_start,
            entry_point_hint_starts,
            correctness_critical_function_starts: BTreeSet::new(),
            generated_stubs: BTreeMap::new(),
            function_renames: HashMap::new(),
            resume_entry_targets_by_owner: BTreeMap::new(),
            bootstrap_info: BootstrapInfo::default(),
        };
        recompiler.collect_correctness_critical_function_starts();

        if recompiler.functions.is_empty() {
            reporter.error("elf", "No functions found in ELF file.");
            return Err(RecompileError("No functions found in ELF file.".to_owned()));
        }

        {
            let entry = recompiler.elf.entry();
            reporter.info("elf", &format!("ELF entry point: 0x{entry:x}"));
            let mut bss_start = u32::MAX;
            let mut bss_end = 0u32;
            for section in recompiler.sections.iter() {
                if section.is_bss && section.size > 0 {
                    bss_start = bss_start.min(section.address);
                    bss_end = bss_end.max(section.address.wrapping_add(section.size));
                }
            }

            let gp = recompiler.symbols.iter().find(|symbol| symbol.name == "_gp").map_or(0, |symbol| symbol.address);

            if bss_start != u32::MAX {
                reporter.info(
                    "elf",
                    &format!(
                        "BSS range: 0x{bss_start:x} - 0x{bss_end:x} (size 0x{:x}), gp=0x{gp:x}",
                        bss_end.wrapping_sub(bss_start)
                    ),
                );
            } else {
                reporter.info("elf", &format!("No BSS found, gp=0x{gp:x}"));
            }

            if entry != 0 {
                recompiler.bootstrap_info.valid = true;
                recompiler.bootstrap_info.entry = entry;
                if bss_start != u32::MAX && bss_end > bss_start {
                    recompiler.bootstrap_info.bss_start = bss_start;
                    recompiler.bootstrap_info.bss_end = bss_end;
                } else {
                    recompiler.bootstrap_info.bss_start = 0;
                    recompiler.bootstrap_info.bss_end = 0;
                }
                recompiler.bootstrap_info.gp = gp;
            }
        }

        reporter.record_discovered(
            recompiler.functions.len(),
            recompiler.symbols.len(),
            recompiler.sections.len(),
            recompiler.relocations.len(),
        );
        reporter.progress(&format!(
            "extracted {} functions, {} symbols, {} sections, {} relocations",
            recompiler.functions.len(),
            recompiler.symbols.len(),
            recompiler.sections.len(),
            recompiler.relocations.len()
        ));

        recompiler.code_generator.set_reporter(reporter.clone());
        let mut relocation_call_names: HashMap<u32, String> = HashMap::with_capacity(recompiler.relocations.len());
        for relocation in &recompiler.relocations {
            if relocation.symbol_name.is_empty() {
                continue;
            }
            match relocation_call_names.get(&relocation.offset) {
                None => {
                    relocation_call_names.insert(relocation.offset, relocation.symbol_name.clone());
                }
                Some(existing) if *existing != relocation.symbol_name => {
                    reporter.warning(
                        "relocation",
                        &format!(
                            "multiple relocation symbols at 0x{:x} (keeping '{existing}', ignoring '{}')",
                            relocation.offset, relocation.symbol_name
                        ),
                    );
                }
                Some(_) => {}
            }
        }
        recompiler.code_generator.set_relocation_call_names(relocation_call_names);
        recompiler.code_generator.set_bootstrap_info(recompiler.bootstrap_info.clone());
        recompiler.code_generator.set_configured_jump_tables(&recompiler.config.jump_tables);
        recompiler.code_generator.set_emit_instruction_comments(true);
        recompiler.code_generator.set_codegen(recompiler.config.codegen); // GOW-Port: decision D2

        std::fs::create_dir_all(&recompiler.config.output_path)
            .map_err(|error| RecompileError(format!("{}: {error}", recompiler.config.output_path)))?;

        Ok(recompiler)
    }

    pub fn reporter(&self) -> &Reporter {
        &self.reporter
    }

    pub fn print_report(&self) {
        self.reporter.print_summary();
    }

    fn collect_correctness_critical_function_starts(&mut self) {
        self.correctness_critical_function_starts.clear();
        for section in self.sections.iter() {
            if section.name != ".ctors" && section.name != ".init_array" && section.name != ".preinit_array" {
                continue;
            }
            let Some(data) = &section.data else {
                continue;
            };
            if section.size < 4 {
                continue;
            }
            let mut offset = 0usize;
            while offset + 4 <= section.size as usize && offset + 4 <= data.len() {
                let target = u32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]);
                if target != 0 && target != 0xFFFF_FFFF {
                    self.correctness_critical_function_starts.insert(target);
                }
                offset += 4;
            }
        }
    }

    fn is_stub_function(&self, function: &Function) -> bool {
        self.stub_function_starts.contains(&function.start)
            || self.stub_functions.contains(&function.name)
            || is_stub_name(&function.name)
    }

    fn should_skip_function(&self, function: &Function) -> bool {
        self.skip_function_starts.contains(&function.start) || self.skip_functions.contains(&function.name)
    }

    fn is_correctness_critical_function(&self, function: &Function) -> bool {
        is_correctness_critical_function_name(&function.name)
            || self.correctness_critical_function_starts.contains(&function.start)
    }

    fn has_resolved_stub_handler(&self, function: &Function) -> bool {
        let handler_name = match self.stub_handler_bindings_by_start.get(&function.start) {
            Some(binding) if !binding.is_empty() => binding.as_str(),
            _ => function.name.as_str(),
        };
        resolve_stub_target(handler_name) != StubTarget::Unknown
    }

    /// `recompile`.
    pub fn recompile(&mut self) -> bool {
        self.reporter.progress(&format!("recompiling {} functions", self.functions.len()));

        let mut failed_count = 0usize;
        let mut correctness_critical_failure_count = 0usize;

        let initializer_starts: Vec<u32> = self.correctness_critical_function_starts.iter().copied().collect();
        for initializer_start in initializer_starts {
            if self.functions.iter().any(|function| function.start == initializer_start) {
                continue;
            }
            let binding = self.stub_handler_bindings_by_start.get(&initializer_start);
            if let Some(binding) = binding {
                if resolve_stub_target(binding) != StubTarget::Unknown {
                    let manual_initializer = Function {
                        name: format!("manual_initializer_{binding}"),
                        start: initializer_start,
                        end: initializer_start + 4,
                        ..Function::default()
                    };
                    self.reporter.info(
                        "correctness-critical",
                        &format!("Synthesized initializer entry for resolved handler '{binding}'"),
                    );
                    self.functions.push(manual_initializer);
                    continue;
                }
            }

            correctness_critical_failure_count += 1;
            self.reporter.record_correctness_critical_failure();
            self.reporter.error_at(
                "correctness-critical",
                ".ctors/.init_array",
                initializer_start,
                "Initializer table target has no discovered guest function or manual handler",
            );
        }

        for index in 0..self.functions.len() {
            self.reporter.record_function_processed();
            let snapshot = self.functions[index].clone();
            let correctness_critical = self.is_correctness_critical_function(&snapshot);

            if self.is_stub_function(&snapshot) {
                if self.has_resolved_stub_handler(&snapshot) {
                    let function = &mut self.functions[index];
                    function.is_stub = true;
                    function.is_skipped = false;
                    self.reporter.record_function_stubbed();
                    continue;
                }

                if correctness_critical {
                    self.reporter.record_correctness_critical_guest_fallback();
                }
                self.reporter.warning_at(
                    "stub",
                    &snapshot.name,
                    snapshot.start,
                    "Configured stub has no runtime handler; recompiling the original guest function",
                );
            }

            if self.should_skip_function(&snapshot) {
                if !correctness_critical {
                    let function = &mut self.functions[index];
                    function.is_skipped = true;
                    function.is_stub = false;
                    self.reporter.record_function_skipped();
                    continue;
                }

                self.reporter.record_correctness_critical_guest_fallback();
                self.reporter.warning_at(
                    "correctness-critical",
                    &snapshot.name,
                    snapshot.start,
                    "Initializer skip ignored; recompiling the original guest function",
                );
            }

            let decoded_ok = decode_function(
                &self.elf,
                &self.config,
                &self.reporter,
                &mut self.functions[index],
                &mut self.decoded_functions,
            );
            if !decoded_ok {
                failed_count += 1;
                self.reporter.record_decode_failure();
                self.reporter.record_function_skipped();
                self.functions[index].is_skipped = true;
                if correctness_critical {
                    correctness_critical_failure_count += 1;
                    self.reporter.record_correctness_critical_failure();
                    self.reporter.error_at(
                        "correctness-critical",
                        &snapshot.name,
                        snapshot.start,
                        "Initializer could not be recompiled and has no resolved manual handler",
                    );
                } else {
                    self.reporter.warning_at(
                        "decode",
                        &snapshot.name,
                        snapshot.start,
                        "Skipping function due decode failure",
                    );
                }
                continue;
            }

            let function = &mut self.functions[index];
            function.is_stub = false;
            function.is_skipped = false;
            function.is_recompiled = true;
            self.reporter.record_function_recompiled();
        }

        self.discover_additional_entry_points();

        if failed_count > 0 {
            self.reporter.warning(
                "decode",
                &format!("Recompile completed with {failed_count} function(s) skipped due decode issues."),
            );
        }

        self.reporter.progress("recompilation pass completed");
        correctness_critical_failure_count == 0
    }

    /// `discoverAdditionalEntryPoints`.
    fn discover_additional_entry_points(&mut self) {
        self.resume_entry_targets_by_owner.clear();

        let mut guest_fallback_entry_addresses: BTreeSet<u32> = self.entry_point_hint_starts.clone();
        for &address in &self.stub_function_starts {
            let binding = self.stub_handler_bindings_by_start.get(&address);
            if binding.is_none_or(|binding| resolve_stub_target(binding) == StubTarget::Unknown) {
                guest_fallback_entry_addresses.insert(address);
            }
        }

        // Prefer the existing wrapper when a configured entry lies inside a decoded function. If
        // Ghidra/analyzer omitted the whole routine, synthesize a standalone guest function bounded
        // by the next known function instead of leaving a valid executable target unregistered.
        collect_internal_entry_targets_impl(
            &self.functions,
            &self.decoded_functions,
            &guest_fallback_entry_addresses,
            &mut self.resume_entry_targets_by_owner,
        );

        let mut covered_entry_addresses: BTreeSet<u32> = self.functions.iter().map(|f| f.start).collect();
        for targets in self.resume_entry_targets_by_owner.values() {
            covered_entry_addresses.extend(targets.iter().copied());
        }

        let standalone_entry_addresses: BTreeSet<u32> = guest_fallback_entry_addresses
            .iter()
            .copied()
            .filter(|address| !covered_entry_addresses.contains(address))
            .collect();

        if !standalone_entry_addresses.is_empty() {
            let elf = &self.elf;
            let config = &self.config;
            let reporter = &self.reporter;
            let mut decode_external = |function: &mut Function, decoded: &mut DecodedFunctions| {
                decode_function(elf, config, reporter, function, decoded)
            };
            let configured_stats = discover_additional_entry_points_impl(
                &mut self.functions,
                &mut self.decoded_functions,
                &self.sections,
                None,
                &mut decode_external,
                &standalone_entry_addresses,
            );
            if configured_stats.discovered_count > 0 {
                self.reporter.record_additional_entry_points(configured_stats.discovered_count);
                self.reporter.progress(&format!(
                    "synthesized {} standalone configured guest entry point(s)",
                    configured_stats.discovered_count
                ));
            }
        }

        for function in &self.functions {
            if !function.is_recompiled || function.is_stub || function.is_skipped {
                continue;
            }
            if is_entry_function_name(&function.name) {
                continue;
            }
            let Some(instructions) = self.decoded_functions.get(&function.start) else {
                continue;
            };

            let analysis =
                self.code_generator.collect_internal_branch_targets(function, instructions, Some(&self.functions));

            {
                let owner_targets = self.resume_entry_targets_by_owner.entry(function.start).or_default();
                owner_targets.extend(analysis.resume_entry_points.iter().copied());
                owner_targets.extend(analysis.indirect_fallback_entry_points.iter().copied());
            }

            for &target in &analysis.external_entry_points {
                let Some(owner_index) =
                    find_containing_function(&self.functions, &self.decoded_functions, target, true)
                else {
                    continue;
                };
                let owner = &self.functions[owner_index];
                if owner.start == target {
                    continue;
                }
                self.resume_entry_targets_by_owner.entry(owner.start).or_default().push(target);
            }
        }

        let mut total_targets = 0usize;
        self.resume_entry_targets_by_owner.retain(|_, targets| {
            targets.sort_unstable();
            targets.dedup();
            if targets.is_empty() {
                return false;
            }
            total_targets += targets.len();
            true
        });

        self.code_generator.set_resume_entry_targets(self.resume_entry_targets_by_owner.clone());

        if total_targets > 0 {
            self.reporter.record_additional_entry_points(total_targets);
            self.reporter.progress(&format!(
                "collected {total_targets} resumable entry point(s) across {} owner function(s)",
                self.resume_entry_targets_by_owner.len()
            ));
        }
    }

    fn get_output_path(&self, function: &Function) -> PathBuf {
        let (mut safe_name, used_renamed_name) = match self.function_renames.get(&function.start) {
            Some(renamed) if !renamed.is_empty() => (renamed.clone(), true),
            _ => (sanitize_function_name(&function.name), false),
        };

        safe_name = safe_name
            .chars()
            .map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '$') { '_' } else { c })
            .collect();

        if safe_name.is_empty() {
            safe_name = format!("func_{:x}", function.start);
        }

        if !used_renamed_name {
            let suffix = format!("_0x{:x}", function.start);
            if !safe_name.ends_with(&suffix) {
                safe_name.push_str(&suffix);
            }
        }

        let mut output_path = PathBuf::from(&self.config.output_path);
        output_path.push(clamp_filename_length(&safe_name, ".cpp", 100));
        output_path
    }

    fn write_to_file(&self, path: &Path, content: &str) -> bool {
        // `std::ofstream` in text mode: CRLF on Windows, LF elsewhere.
        let data: std::borrow::Cow<'_, str> =
            if cfg!(windows) { std::borrow::Cow::Owned(content.replace('\n', "\r\n")) } else { content.into() };
        // GOW-Port: a file whose bytes would not change is left alone, timestamp included. Rewriting all ~6,400 files on
        // every translation made the next build recompile the whole game even when the generated code was the same. The
        // bytes on disk are identical either way, so the parity gate is unaffected.
        if std::fs::read(path).is_ok_and(|existing| existing == data.as_bytes()) {
            return true;
        }
        match std::fs::write(path, data.as_bytes()) {
            Ok(()) => true,
            Err(_) => {
                self.reporter.error("file", &format!("Failed to open file for writing: {}", path.display()));
                false
            }
        }
    }

    fn generate_function_header(&self) -> bool {
        let mut ss = String::new();
        ss.push_str("#ifndef PS2_RECOMPILED_FUNCTIONS_H\n");
        ss.push_str("#define PS2_RECOMPILED_FUNCTIONS_H\n\n");
        ss.push_str("#include <cstdint>\n\n");
        ss.push_str("struct R5900Context;\n");
        ss.push_str("class PS2Runtime;\n\n");

        for function in &self.functions {
            if !should_generate_code_for_function(function) {
                continue;
            }
            let final_name = self.code_generator.get_function_name(function.start);
            let _ = writeln!(ss, "void {final_name}(uint8_t* rdram, R5900Context* ctx, PS2Runtime *runtime);");
        }

        ss.push_str("\n#endif // PS2_RECOMPILED_FUNCTIONS_H\n");

        let header_path = Path::new(&self.config.output_path).join("ps2_recompiled_functions.h");
        self.write_to_file(&header_path, &ss);
        self.reporter.progress(&format!("generated function header file: {}", quoted_path(&header_path)));
        true
    }

    fn generate_stub_header(&self) -> bool {
        let mut ss = String::new();
        ss.push_str("#pragma once\n\n");
        ss.push_str("#include <cstdint>\n");
        ss.push_str("#include \"ps2_runtime.h\"\n");
        ss.push_str("#include \"ps2_syscalls.h\"\n\n");

        let mut stub_names: HashSet<String> = HashSet::new();
        for function in &self.functions {
            if !function.is_stub && !function.is_skipped {
                continue;
            }
            let generated_name = self.code_generator.get_function_name(function.start);
            if generated_name.is_empty() {
                continue;
            }
            if !stub_names.insert(generated_name.clone()) {
                continue;
            }
            let _ = writeln!(ss, "void {generated_name}(uint8_t* rdram, R5900Context* ctx, PS2Runtime* runtime);");
        }

        let header_path = Path::new(&self.config.output_path).join("ps2_recompiled_stubs.h");
        self.write_to_file(&header_path, &ss);
        self.reporter.progress(&format!("generated stub header file: {}", quoted_path(&header_path)));
        true
    }

    /// `generateOutput`.
    pub fn generate_output(&mut self) -> Result<(), RecompileError> {
        let result = self.generate_output_inner();
        if let Err(error) = &result {
            self.reporter.error("output", &error.0);
            self.reporter.print_summary();
        }
        result
    }

    fn make_name(function: &Function) -> String {
        let mut sanitized = sanitize_function_name(&function.name);
        if sanitized.is_empty() {
            sanitized = "func".to_owned();
        }

        if sanitized.starts_with("entry_") && sanitized == format!("entry_{:x}", function.start) {
            return format!("{sanitized}_0x{:x}", function.end);
        }

        format!("{sanitized}_0x{:x}", function.start)
    }

    fn build_stub_source(&self, function: &Function) -> String {
        let generated_name = self.code_generator.get_function_name(function.start);
        let mut stub = String::new();
        let _ = writeln!(stub, "void {generated_name}(uint8_t* rdram, R5900Context* ctx, PS2Runtime *runtime) {{");
        stub.push_str("#ifdef _DEBUG\n");
        let _ = writeln!(stub, "    PS_LOG_ENTRY(\"{generated_name}\");");
        stub.push_str("#endif\n");
        stub.push_str("    ctx->pc = getRegU32(ctx, 31);\n");
        stub.push_str("    ");

        if function.is_skipped {
            let _ = write!(
                stub,
                "ps2_stubs::TODO_NAMED(\"{}\", rdram, ctx, runtime); ",
                escape_c_string_literal(&function.name)
            );
        } else {
            let mut dispatch_name = function.name.clone();
            if let Some(binding) = self.stub_handler_bindings_by_start.get(&function.start) {
                if !binding.is_empty() {
                    dispatch_name = binding.clone();
                }
            }

            if let Some(syscall) = resolve_syscall_name(&dispatch_name) {
                let _ = write!(stub, "ps2_syscalls::{syscall}(rdram, ctx, runtime); ");
            } else if let Some(stub_name) = resolve_stub_name(&dispatch_name) {
                let _ = write!(stub, "ps2_stubs::{stub_name}(rdram, ctx, runtime); ");
            } else {
                if dispatch_name.is_empty() {
                    dispatch_name = function.name.clone();
                }
                let _ = write!(
                    stub,
                    "ps2_stubs::TODO_NAMED(\"{}\", rdram, ctx, runtime); ",
                    escape_c_string_literal(&dispatch_name)
                );
            }
        }

        stub.push_str("\n}");
        stub
    }

    fn generate_output_inner(&mut self) -> Result<(), RecompileError> {
        self.function_renames.clear();

        for function in &self.functions {
            if !should_generate_code_for_function(function) {
                continue;
            }
            self.function_renames.insert(function.start, Self::make_name(function));
        }

        self.code_generator.set_renamed_functions(self.function_renames.clone());

        if self.bootstrap_info.valid {
            if let Some(entry) = self.functions.iter().find(|f| f.start == self.bootstrap_info.entry) {
                self.bootstrap_info.entry_name = match self.function_renames.get(&entry.start) {
                    Some(renamed) => renamed.clone(),
                    None => sanitize_function_name(&entry.name),
                };
            }
            self.code_generator.set_bootstrap_info(self.bootstrap_info.clone());
        }

        self.generated_stubs.clear();
        for function in &self.functions {
            if function.is_stub || function.is_skipped {
                let source = self.build_stub_source(function);
                self.generated_stubs.insert(function.start, source);
            }
        }

        if !self.generate_function_header() {
            return Err(RecompileError("Failed to generate function header".to_owned()));
        }

        let output_functions: Vec<&Function> =
            self.functions.iter().filter(|function| should_generate_code_for_function(function)).collect();

        self.reporter.record_generated_functions(output_functions.len());

        let output_worker_count = if self.config.low_memory_mode {
            1
        } else {
            resolve_output_worker_count(self.config.output_worker_threads)
        };
        if output_functions.len() > 1 && output_worker_count > 1 {
            self.reporter.progress(&format!("generating function output with {output_worker_count} worker(s)"));
        }

        let generated_stubs = &self.generated_stubs;
        let decoded_functions = &self.decoded_functions;
        let reporter = &self.reporter;

        let generate_function_code =
            |generator: &mut CodeGenerator, function: &Function, use_headers: bool| -> Result<String, RecompileError> {
                let result: Result<String, RecompileError> = (|| {
                    if function.is_stub || function.is_skipped {
                        let stub = generated_stubs
                            .get(&function.start)
                            .ok_or_else(|| RecompileError(format!("missing stub for 0x{:x}", function.start)))?;
                        if !use_headers {
                            return Ok(stub.clone());
                        }
                        let mut stub_file = String::new();
                        stub_file.push_str("#include \"ps2_runtime.h\"\n");
                        stub_file.push_str("#include \"ps2_syscalls.h\"\n");
                        stub_file.push_str("#include \"ps2_stubs.h\"\n");
                        stub_file.push_str("#ifdef _DEBUG\n");
                        stub_file.push_str("#include \"ps2_log.h\"\n");
                        stub_file.push_str("#endif\n");
                        stub_file.push('\n');
                        stub_file.push_str(stub);
                        stub_file.push('\n');
                        return Ok(stub_file);
                    }

                    let instructions = decoded_functions
                        .get(&function.start)
                        .ok_or_else(|| RecompileError(format!("missing decoded function 0x{:x}", function.start)))?;
                    Ok(emit_function(generator, function, instructions, use_headers)?)
                })();

                if let Err(error) = &result {
                    reporter.error_at(
                        "codegen",
                        &function.name,
                        function.start,
                        &format!("Error generating code: {}", error.0),
                    );
                }
                result
            };

        if self.config.single_file_output {
            let output_path = Path::new(&self.config.output_path).join("ps2_recompiled_functions.cpp");
            let mut combined = String::new();
            write_combined_output_preamble(&mut combined);

            let mut generator = self.code_generator.clone();
            for function in &output_functions {
                combined.push_str(&generate_function_code(&mut generator, function, false)?);
                combined.push_str("\n\n");
            }

            if !self.write_to_file(&output_path, &combined) {
                return Err(RecompileError(format!("Failed to open combined output: {}", output_path.display())));
            }
            self.reporter.progress(&format!("wrote combined output to {}", quoted_path(&output_path)));
        } else {
            let output_paths: Vec<PathBuf> =
                output_functions.iter().map(|function| self.get_output_path(function)).collect();

            let next_index = AtomicUsize::new(0);
            let first_error: Mutex<Option<RecompileError>> = Mutex::new(None);
            let base_generator = self.code_generator.clone();

            std::thread::scope(|scope| {
                for _ in 0..output_worker_count.max(1) {
                    scope.spawn(|| {
                        let mut generator = base_generator.clone();
                        loop {
                            if first_error.lock().map(|e| e.is_some()).unwrap_or(true) {
                                return;
                            }
                            let index = next_index.fetch_add(1, Ordering::SeqCst);
                            if index >= output_functions.len() {
                                return;
                            }

                            let mut step = || -> Result<(), RecompileError> {
                                let code = generate_function_code(&mut generator, output_functions[index], true)?;
                                let path = &output_paths[index];
                                if let Some(parent) = path.parent() {
                                    std::fs::create_dir_all(parent)
                                        .map_err(|error| RecompileError(format!("{}: {error}", parent.display())))?;
                                }
                                if !self.write_to_file(path, &code) {
                                    return Err(RecompileError(format!(
                                        "Failed to write function output: {}",
                                        path.display()
                                    )));
                                }
                                Ok(())
                            };

                            if let Err(error) = step() {
                                if let Ok(mut slot) = first_error.lock() {
                                    if slot.is_none() {
                                        *slot = Some(error);
                                    }
                                }
                                return;
                            }
                        }
                    });
                }
            });

            if let Some(error) = first_error.into_inner().ok().flatten() {
                return Err(error);
            }

            self.reporter.progress(&format!("wrote individual function files to {}", self.config.output_path));
        }

        self.decoded_functions.clear();

        let register_functions = emit_function_table(&self.code_generator, &self.functions)?;
        self.generated_stubs.clear();

        let register_path = Path::new(&self.config.output_path).join("register_functions.cpp");
        if !self.write_to_file(&register_path, &register_functions) {
            return Err(RecompileError(format!(
                "Failed to write function registration file: {}",
                register_path.display()
            )));
        }
        self.reporter.progress(&format!("generated function registration file: {}", quoted_path(&register_path)));

        if !self.generate_stub_header() {
            return Err(RecompileError("Failed to generate stub header".to_owned()));
        }

        Ok(())
    }
}
