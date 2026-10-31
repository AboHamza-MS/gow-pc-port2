//! The code generator: the state the emitters and the translators share.
//!
//! Port of `CodeGenerator` (`code_generator.{h,cpp}`). It owns the symbol table,
//! the renames chosen for the output, the relocation-bound call names, the
//! configured jump tables and the resume entry points, and it implements
//! [`FamilyTranslator`] so every family translator can call back into it exactly
//! like the C++ translators call `CodeGenerator::translate*`.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use ps2_config::JumpTable;
use ps2_r5900::Instruction;

use crate::flow::analyzer::{AnalysisResult, ControlFlowAnalyzer};
use crate::reporter::Reporter;
use crate::translate::instruction::{self, FamilyTranslator};
use crate::translate::memory::MemoryAccessHint;
use crate::translate::{cop0, fpu, mmi, mmi_helpers, mmi_tables, regimm, special, vu, vu_helpers};
use crate::types::{Function, Section, Symbol};

/// C++ keywords: a function named like one gets a `ps2_` prefix (`kKeywords`).
const KEYWORDS: &[&str] = &[
    "alignas",
    "alignof",
    "and",
    "and_eq",
    "asm",
    "auto",
    "bitand",
    "bitor",
    "bool",
    "break",
    "case",
    "catch",
    "char",
    "char8_t",
    "char16_t",
    "char32_t",
    "class",
    "compl",
    "concept",
    "const",
    "consteval",
    "constexpr",
    "constinit",
    "const_cast",
    "continue",
    "co_await",
    "co_return",
    "co_yield",
    "decltype",
    "default",
    "delete",
    "do",
    "double",
    "dynamic_cast",
    "else",
    "enum",
    "explicit",
    "export",
    "extern",
    "false",
    "float",
    "for",
    "friend",
    "goto",
    "if",
    "inline",
    "int",
    "long",
    "mutable",
    "namespace",
    "new",
    "noexcept",
    "not",
    "not_eq",
    "nullptr",
    "operator",
    "or",
    "or_eq",
    "private",
    "protected",
    "public",
    "register",
    "reinterpret_cast",
    "requires",
    "return",
    "short",
    "signed",
    "sizeof",
    "static",
    "static_assert",
    "static_cast",
    "struct",
    "switch",
    "template",
    "this",
    "thread_local",
    "throw",
    "true",
    "try",
    "typedef",
    "typeid",
    "typename",
    "union",
    "unsigned",
    "using",
    "virtual",
    "void",
    "volatile",
    "wchar_t",
    "while",
    "xor",
    "xor_eq",
];

pub fn is_cxx_keyword(name: &str) -> bool {
    KEYWORDS.contains(&name)
}

/// `isReservedCxxIdentifier`: `__x` or `_X`.
pub fn is_reserved_cxx_identifier(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.len() >= 2 && bytes[0] == b'_' && (bytes[1] == b'_' || bytes[1].is_ascii_uppercase())
}

/// `sanitizeIdentifierBody`: anything that is not `[A-Za-z0-9_]` becomes `_`,
/// and a leading digit gets an underscore in front. Works on bytes, like the
/// C++ `std::isalnum` loop on `char`.
pub fn sanitize_identifier_body(name: &str) -> String {
    let mut sanitized: Vec<u8> = Vec::with_capacity(name.len() + 1);
    for &byte in name.as_bytes() {
        if byte.is_ascii_alphanumeric() || byte == b'_' {
            sanitized.push(byte);
        } else {
            sanitized.push(b'_');
        }
    }

    if sanitized.is_empty() {
        return String::new();
    }

    let first = sanitized[0];
    if !(first.is_ascii_alphabetic() || first == b'_') {
        sanitized.insert(0, b'_');
    }

    // Only ASCII bytes were pushed.
    String::from_utf8(sanitized).unwrap_or_default()
}

/// `CodeGenerator::BootstrapInfo`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BootstrapInfo {
    pub valid: bool,
    pub entry: u32,
    pub bss_start: u32,
    pub bss_end: u32,
    pub gp: u32,
    pub entry_name: String,
}

/// Failure of one function's generation (the C++ throws `std::runtime_error`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationError(pub String);

impl std::fmt::Display for GenerationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for GenerationError {}

/// `CodeGenerator`. Cloning is cheap (shared tables), which is what the output
/// workers rely on: each one owns a clone, like the C++ copy constructor.
#[derive(Clone)]
pub struct CodeGenerator {
    symbols: Arc<HashMap<u32, Symbol>>,
    renamed_functions: Arc<HashMap<u32, String>>,
    relocation_call_names: Arc<HashMap<u32, String>>,
    config_jump_table_targets: Arc<HashMap<u32, Vec<u32>>>,
    resume_entry_targets_by_owner: Arc<BTreeMap<u32, Vec<u32>>>,
    sections: Arc<Vec<Section>>,
    bootstrap: BootstrapInfo,
    emit_instruction_comments: bool,
    reporter: Option<Arc<Reporter>>,
    current_function_name: String,
}

impl CodeGenerator {
    pub fn new(symbols: &[Symbol], sections: Arc<Vec<Section>>) -> Self {
        let mut by_address = HashMap::with_capacity(symbols.len());
        for symbol in symbols {
            // `emplace` keeps the first symbol seen at an address.
            by_address.entry(symbol.address).or_insert_with(|| symbol.clone());
        }
        Self {
            symbols: Arc::new(by_address),
            renamed_functions: Arc::default(),
            relocation_call_names: Arc::default(),
            config_jump_table_targets: Arc::default(),
            resume_entry_targets_by_owner: Arc::default(),
            sections,
            bootstrap: BootstrapInfo::default(),
            emit_instruction_comments: true,
            reporter: None,
            current_function_name: String::new(),
        }
    }

    pub fn set_renamed_functions(&mut self, renames: HashMap<u32, String>) {
        self.renamed_functions = Arc::new(renames);
    }

    pub fn set_bootstrap_info(&mut self, info: BootstrapInfo) {
        self.bootstrap = info;
    }

    pub fn set_relocation_call_names(&mut self, names: HashMap<u32, String>) {
        self.relocation_call_names = Arc::new(names);
    }

    pub fn set_configured_jump_tables(&mut self, jump_tables: &[JumpTable]) {
        let mut targets: HashMap<u32, Vec<u32>> = HashMap::new();
        for table in jump_tables {
            let entry = targets.entry(table.address).or_default();
            entry.extend(table.entries.iter().map(|e| e.target));
        }
        for list in targets.values_mut() {
            list.sort_unstable();
            list.dedup();
        }
        self.config_jump_table_targets = Arc::new(targets);
    }

    pub fn set_resume_entry_targets(&mut self, by_owner: BTreeMap<u32, Vec<u32>>) {
        let mut by_owner = by_owner;
        for list in by_owner.values_mut() {
            list.sort_unstable();
            list.dedup();
        }
        self.resume_entry_targets_by_owner = Arc::new(by_owner);
    }

    pub fn set_emit_instruction_comments(&mut self, emit: bool) {
        self.emit_instruction_comments = emit;
    }

    pub fn set_reporter(&mut self, reporter: Arc<Reporter>) {
        self.reporter = Some(reporter);
    }

    pub fn reporter(&self) -> Option<&Reporter> {
        self.reporter.as_deref()
    }

    pub fn emit_instruction_comments(&self) -> bool {
        self.emit_instruction_comments
    }

    pub fn bootstrap(&self) -> &BootstrapInfo {
        &self.bootstrap
    }

    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    pub fn relocation_call_name(&self, address: u32) -> Option<&str> {
        self.relocation_call_names.get(&address).map(String::as_str)
    }

    pub fn resume_entry_targets_by_owner(&self) -> &BTreeMap<u32, Vec<u32>> {
        &self.resume_entry_targets_by_owner
    }

    pub fn set_current_function_name(&mut self, name: &str) {
        self.current_function_name.clear();
        self.current_function_name.push_str(name);
    }

    pub fn find_symbol_by_address(&self, address: u32) -> Option<&Symbol> {
        self.symbols.get(&address)
    }

    /// `getFunctionName`: the rename chosen for the output, else the sanitized symbol.
    pub fn get_function_name(&self, address: u32) -> String {
        if let Some(renamed) = self.renamed_functions.get(&address) {
            return renamed.clone();
        }
        match self.find_symbol_by_address(address) {
            Some(symbol) if symbol.is_function => Self::sanitize_function_name(&symbol.name),
            _ => String::new(),
        }
    }

    /// `CodeGenerator::sanitizeFunctionName`.
    pub fn sanitize_function_name(name: &str) -> String {
        let sanitized = sanitize_identifier_body(name);
        if sanitized.is_empty() {
            return sanitized;
        }

        // ugly but will do for now
        if sanitized == "main" {
            return "ps2_main".to_owned();
        }

        if is_cxx_keyword(&sanitized) {
            return format!("ps2_{sanitized}");
        }

        if sanitized.starts_with('_') {
            return format!("ps2{sanitized}");
        }

        if !is_reserved_cxx_identifier(&sanitized) {
            return sanitized;
        }

        format!("ps2_{sanitized}")
    }

    /// `collectInternalBranchTargets`.
    pub fn collect_internal_branch_targets(
        &self,
        function: &Function,
        instructions: &[Instruction],
        all_functions: Option<&[Function]>,
    ) -> AnalysisResult {
        ControlFlowAnalyzer::new(&self.sections, &self.config_jump_table_targets, self.reporter.as_deref()).analyze(
            function,
            instructions,
            all_functions,
        )
    }

    /// `translateInstruction(inst)` (no memory hint).
    pub fn translate_instruction(&mut self, inst: &Instruction) -> String {
        instruction::translate(self, inst, MemoryAccessHint::default())
    }

    /// `translateInstruction(inst, memoryHint)`.
    pub fn translate_instruction_with_hint(&mut self, inst: &Instruction, hint: MemoryAccessHint) -> String {
        instruction::translate(self, inst, hint)
    }
}

impl FamilyTranslator for CodeGenerator {
    fn translate_special(&mut self, inst: &Instruction) -> String {
        special::translate(self, inst)
    }

    fn translate_regimm(&mut self, inst: &Instruction) -> String {
        regimm::translate(self, inst)
    }

    fn translate_cop0(&mut self, inst: &Instruction) -> String {
        cop0::translate(self, inst)
    }

    fn translate_fpu(&mut self, inst: &Instruction) -> String {
        fpu::translate(self, inst)
    }

    fn translate_vu(&mut self, inst: &Instruction) -> String {
        vu::translate(self, inst)
    }

    fn translate_mmi(&mut self, inst: &Instruction) -> String {
        mmi::translate(self, inst)
    }

    fn emit_unhandled(&mut self, inst: &Instruction, reason: &str) -> String {
        if let Some(reporter) = &self.reporter {
            reporter.record_unhandled_instruction(&self.current_function_name, inst.address, inst.raw, reason);
        }
        format!("throw std::runtime_error(\"{reason} at 0x{:X} raw=0x{:08X}\");", inst.address, inst.raw)
    }

    fn translate_mmi0(&mut self, inst: &Instruction) -> String {
        mmi_tables::mmi0(self, inst)
    }

    fn translate_mmi1(&mut self, inst: &Instruction) -> String {
        mmi_tables::mmi1(self, inst)
    }

    fn translate_mmi2(&mut self, inst: &Instruction) -> String {
        mmi_helpers::mmi2(self, inst)
    }

    fn translate_mmi3(&mut self, inst: &Instruction) -> String {
        mmi_helpers::mmi3(self, inst)
    }

    fn translate_vu_macro(&mut self, inst: &Instruction) -> String {
        vu_helpers::translate_macro(self, inst)
    }

    fn translate_pmfhl(&mut self, inst: &Instruction) -> String {
        mmi_helpers::pmfhl(self, inst)
    }

    fn translate_pmthl(&mut self, inst: &Instruction) -> String {
        mmi_helpers::pmthl(self, inst)
    }
}
