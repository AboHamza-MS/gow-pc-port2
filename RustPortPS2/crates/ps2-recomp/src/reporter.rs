//! Port of `RecompilerReporter` (`recompiler_reporter.{h,cpp}`).
//!
//! The report is diagnostic, not part of the generated tree, but it is kept
//! identical so the C++ and Rust logs can be diffed during the parity runs.

use std::fmt::Write as _;
use std::io::Write as _;
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl Severity {
    fn name(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Event {
    pub severity: Severity,
    pub category: String,
    pub message: String,
    pub function_name: String,
    pub address: u32,
    pub has_address: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Counters {
    pub functions_discovered: usize,
    pub symbols_discovered: usize,
    pub sections_discovered: usize,
    pub relocations_discovered: usize,
    pub functions_processed: usize,
    pub functions_recompiled: usize,
    pub functions_stubbed: usize,
    pub functions_skipped: usize,
    pub decode_failures: usize,
    pub additional_entry_points: usize,
    pub generated_functions: usize,
    pub unhandled_instructions: usize,
    pub indirect_fallback_promotions: usize,
    pub indirect_fallback_entries: usize,
    pub correctness_critical_guest_fallbacks: usize,
    pub correctness_critical_failures: usize,
}

#[derive(Default)]
struct State {
    counters: Counters,
    events: Vec<Event>,
}

/// Thread-safe collector of counters and events.
#[derive(Default)]
pub struct Reporter {
    state: Mutex<State>,
}

fn hex_address(address: u32) -> String {
    format!("0x{address:x}")
}

impl Reporter {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn progress(&self, message: &str) {
        let _guard = self.lock();
        println!("[recompiler] {message}");
        let _ = std::io::stdout().flush();
    }

    fn add_event(&self, severity: Severity, category: &str, message: &str, function: &str, address: Option<u32>) {
        self.lock().events.push(Event {
            severity,
            category: category.to_owned(),
            message: message.to_owned(),
            function_name: function.to_owned(),
            address: address.unwrap_or(0),
            has_address: address.is_some(),
        });
    }

    pub fn info(&self, category: &str, message: &str) {
        self.add_event(Severity::Info, category, message, "", None);
    }
    pub fn warning(&self, category: &str, message: &str) {
        self.add_event(Severity::Warning, category, message, "", None);
    }
    pub fn error(&self, category: &str, message: &str) {
        self.add_event(Severity::Error, category, message, "", None);
    }
    pub fn warning_at(&self, category: &str, function: &str, address: u32, message: &str) {
        self.add_event(Severity::Warning, category, message, function, Some(address));
    }
    pub fn error_at(&self, category: &str, function: &str, address: u32, message: &str) {
        self.add_event(Severity::Error, category, message, function, Some(address));
    }

    pub fn record_discovered(&self, functions: usize, symbols: usize, sections: usize, relocations: usize) {
        let mut state = self.lock();
        state.counters.functions_discovered = functions;
        state.counters.symbols_discovered = symbols;
        state.counters.sections_discovered = sections;
        state.counters.relocations_discovered = relocations;
    }
    pub fn record_function_processed(&self) {
        self.lock().counters.functions_processed += 1;
    }
    pub fn record_function_recompiled(&self) {
        self.lock().counters.functions_recompiled += 1;
    }
    pub fn record_function_stubbed(&self) {
        self.lock().counters.functions_stubbed += 1;
    }
    pub fn record_function_skipped(&self) {
        self.lock().counters.functions_skipped += 1;
    }
    pub fn record_decode_failure(&self) {
        self.lock().counters.decode_failures += 1;
    }
    pub fn record_additional_entry_points(&self, count: usize) {
        self.lock().counters.additional_entry_points += count;
    }
    pub fn record_generated_functions(&self, count: usize) {
        self.lock().counters.generated_functions += count;
    }
    pub fn record_correctness_critical_guest_fallback(&self) {
        self.lock().counters.correctness_critical_guest_fallbacks += 1;
    }
    pub fn record_correctness_critical_failure(&self) {
        self.lock().counters.correctness_critical_failures += 1;
    }

    pub fn record_indirect_fallback_promotion(&self, function: &str, jump_addresses: &[u32], promoted: usize) {
        let mut message = String::from("unresolved JR/JALR at");
        for address in jump_addresses {
            let _ = write!(message, " {}", hex_address(*address));
        }
        let _ = write!(message, "; promoted {promoted} fallback entr{}", if promoted == 1 { "y" } else { "ies" });

        let mut state = self.lock();
        state.counters.indirect_fallback_promotions += 1;
        state.counters.indirect_fallback_entries += promoted;
        state.events.push(Event {
            severity: Severity::Warning,
            category: "control-flow".to_owned(),
            message,
            function_name: function.to_owned(),
            address: jump_addresses.first().copied().unwrap_or(0),
            has_address: !jump_addresses.is_empty(),
        });
    }

    pub fn record_unhandled_instruction(&self, function: &str, address: u32, raw: u32, message: &str) {
        let mut state = self.lock();
        state.counters.unhandled_instructions += 1;
        state.events.push(Event {
            severity: Severity::Error,
            category: "unhandled-instruction".to_owned(),
            message: format!("{message} raw=0x{raw:x}"),
            function_name: function.to_owned(),
            address,
            has_address: true,
        });
    }

    pub fn counters(&self) -> Counters {
        self.lock().counters.clone()
    }

    pub fn has_errors(&self) -> bool {
        self.lock().events.iter().any(|event| event.severity == Severity::Error)
    }

    pub fn has_warnings(&self) -> bool {
        self.lock().events.iter().any(|event| event.severity == Severity::Warning)
    }

    /// The text `printSummary` writes.
    pub fn summary(&self) -> String {
        let state = self.lock();
        let c = &state.counters;
        let mut out = String::new();
        let _ = writeln!(out, "\n========== PS2Recomp report ==========");
        let _ = writeln!(out, "Functions discovered: {}", c.functions_discovered);
        let _ = writeln!(
            out,
            "Symbols: {}, sections: {}, relocations: {}",
            c.symbols_discovered, c.sections_discovered, c.relocations_discovered
        );
        let _ = writeln!(
            out,
            "Functions processed: {}, recompiled: {}, stubs: {}, skipped: {}, decode failures: {}",
            c.functions_processed, c.functions_recompiled, c.functions_stubbed, c.functions_skipped, c.decode_failures
        );
        let _ = writeln!(out, "Additional entrypoints: {}", c.additional_entry_points);
        let _ = writeln!(out, "Generated functions: {}", c.generated_functions);
        let _ = writeln!(
            out,
            "Indirect fallback promotions: {} ({} fallback entries)",
            c.indirect_fallback_promotions, c.indirect_fallback_entries
        );
        let _ = writeln!(out, "Unhandled instructions: {}", c.unhandled_instructions);
        let _ = writeln!(
            out,
            "Correctness-critical guest fallbacks: {}, failures: {}",
            c.correctness_critical_guest_fallbacks, c.correctness_critical_failures
        );

        let warnings = state.events.iter().filter(|e| e.severity == Severity::Warning).count();
        let errors = state.events.iter().filter(|e| e.severity == Severity::Error).count();
        let _ = writeln!(out, "Warnings: {warnings}, errors: {errors}");

        if !state.events.is_empty() {
            let _ = writeln!(out, "\nEvents:");
            for event in &state.events {
                let _ = write!(out, "  [{}] {}", event.severity.name(), event.category);
                if !event.function_name.is_empty() {
                    let _ = write!(out, " function={}", event.function_name);
                }
                if event.has_address {
                    let _ = write!(out, " addr={}", hex_address(event.address));
                }
                let _ = writeln!(out, " - {}", event.message);
            }
        }
        let _ = writeln!(out, "======================================");
        out
    }

    pub fn print_summary(&self) {
        print!("{}", self.summary());
        let _ = std::io::stdout().flush();
    }
}
