//! Control flow analysis of one function.
//!
//! Port of `ControlFlowAnalyzer::analyze` (`control_flow_analyzer.cpp`): which
//! addresses inside a function need a label, which ones are resume points for
//! the scheduler, which indirect jumps are jump tables, and which targets belong
//! to some other function. Everything is kept in ordered collections: the C++
//! uses `unordered_*`, but every consumer sorts or only probes membership.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use ps2_r5900::opcodes::*;
use ps2_r5900::Instruction;

use crate::reporter::Reporter;
use crate::translate::instruction::build_absolute_jump_target;
use crate::types::{Function, Section};

/// `ControlFlowAnalysisResult`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AnalysisResult {
    pub entry_points: BTreeSet<u32>,
    pub external_entry_points: BTreeSet<u32>,
    pub resume_entry_points: BTreeSet<u32>,
    pub indirect_fallback_entry_points: BTreeSet<u32>,
    pub jump_table_targets: BTreeMap<u32, Vec<u32>>,
}

/// `ControlFlowAnalyzer`.
pub struct ControlFlowAnalyzer<'a> {
    sections: &'a [Section],
    configured_jump_tables: &'a HashMap<u32, Vec<u32>>,
    reporter: Option<&'a Reporter>,
}

impl<'a> ControlFlowAnalyzer<'a> {
    pub fn new(
        sections: &'a [Section],
        configured_jump_tables: &'a HashMap<u32, Vec<u32>>,
        reporter: Option<&'a Reporter>,
    ) -> Self {
        Self { sections, configured_jump_tables, reporter }
    }

    fn is_executable_address(&self, address: u32) -> bool {
        self.sections.iter().any(|section| section.is_code && section.contains(address))
    }

    /// The innermost recompiled, non-entry function of `all` that contains `address`.
    fn find_containing_external_function<'f>(&self, address: u32, all: Option<&'f [Function]>) -> Option<&'f Function> {
        let all = all?;
        if !self.is_executable_address(address) {
            return None;
        }

        let mut best: Option<&Function> = None;
        for candidate in all {
            if !candidate.is_recompiled || candidate.is_stub || candidate.is_skipped {
                continue;
            }
            if candidate.name.starts_with("entry_") {
                continue;
            }
            if address < candidate.start || address >= candidate.end {
                continue;
            }
            if best.is_none_or(|best| candidate.start > best.start) {
                best = Some(candidate);
            }
        }
        best
    }

    /// `analyze`.
    pub fn analyze(
        &self,
        function: &Function,
        instructions: &[Instruction],
        all_functions: Option<&[Function]>,
    ) -> AnalysisResult {
        let mut result = AnalysisResult::default();
        let instruction_addresses: BTreeSet<u32> = instructions.iter().map(|inst| inst.address).collect();
        let mut indirect_jumps: Vec<usize> = Vec::new();

        let queue_external_entry_target = |result: &mut AnalysisResult, target: u32| {
            let Some(containing) = self.find_containing_external_function(target, all_functions) else {
                return;
            };
            if containing.start == function.start || target == containing.start {
                return;
            }
            result.external_entry_points.insert(target);
        };

        let in_function =
            |target: u32| target >= function.start && target < function.end && instruction_addresses.contains(&target);

        let queue_resume_entry_target = |result: &mut AnalysisResult, resume: u32| {
            if in_function(resume) {
                result.entry_points.insert(resume);
                result.resume_entry_points.insert(resume);
            }
        };

        let queue_loop_resume_entry_target = |result: &mut AnalysisResult, target: u32, source_pc: u32| {
            if target > source_pc || target == function.start {
                return;
            }
            queue_resume_entry_target(result, target);
        };

        for (index, inst) in instructions.iter().enumerate() {
            if inst.opcode == OPCODE_SPECIAL
                && ((inst.function == SPECIAL_JR && inst.rs != 31) || inst.function == SPECIAL_JALR)
            {
                indirect_jumps.push(index);
            }
        }
        let has_indirect_register_jump = !indirect_jumps.is_empty();

        for inst in instructions {
            // A guest-installed syscall handler runs as a separate invocation, so the scheduler
            // resumes this thread at syscall+4 and needs an entry point there (no delay slot).
            if inst.opcode == OPCODE_SPECIAL && inst.function == SPECIAL_SYSCALL {
                queue_resume_entry_target(&mut result, inst.address.wrapping_add(4));
            }

            let is_static_jump = inst.opcode == OPCODE_J || inst.opcode == OPCODE_JAL;
            if inst.is_branch && inst.opcode != OPCODE_J && inst.opcode != OPCODE_JAL {
                let offset_bytes = (inst.simmediate as u16 as i16 as i32) << 2;
                let target = inst.address.wrapping_add(4).wrapping_add(offset_bytes as u32);

                if in_function(target) {
                    result.entry_points.insert(target);
                    queue_loop_resume_entry_target(&mut result, target, inst.address);
                } else {
                    queue_external_entry_target(&mut result, target);
                }
            } else if is_static_jump {
                let target = build_absolute_jump_target(inst.address, inst.target);
                if in_function(target) {
                    result.entry_points.insert(target);
                    queue_loop_resume_entry_target(&mut result, target, inst.address);
                } else {
                    queue_external_entry_target(&mut result, target);
                }
                if inst.opcode == OPCODE_JAL {
                    queue_resume_entry_target(&mut result, inst.address.wrapping_add(8));
                }
            }
        }

        if has_indirect_register_jump {
            let mut needs_indirect_fallback = false;
            for &jump_index in &indirect_jumps {
                let jr_inst = &instructions[jump_index];
                if jr_inst.function == SPECIAL_JALR {
                    queue_resume_entry_target(&mut result, jr_inst.address.wrapping_add(8));
                }

                let found_table = self.resolve_jump_table(
                    function,
                    instructions,
                    jump_index,
                    &instruction_addresses,
                    &mut result,
                    &queue_external_entry_target,
                );
                if !found_table {
                    needs_indirect_fallback = true;
                }
            }

            if needs_indirect_fallback {
                if let Some(reporter) = self.reporter {
                    let jump_addresses: Vec<u32> = indirect_jumps.iter().map(|&i| instructions[i].address).collect();
                    reporter.record_indirect_fallback_promotion(
                        &function.name,
                        &jump_addresses,
                        instruction_addresses.len(),
                    );
                }

                for &address in &instruction_addresses {
                    if address >= function.start && address < function.end {
                        result.entry_points.insert(address);
                        // Keep labels and runtime registration for unresolved JR/JALR targets
                        // without emitting a local switch over every possible target.
                        result.indirect_fallback_entry_points.insert(address);
                    }
                }
            }
        }

        result
    }

    /// The jump-table recognition for one `jr`/`jalr`; true when targets were found.
    #[allow(clippy::too_many_arguments)]
    fn resolve_jump_table(
        &self,
        function: &Function,
        instructions: &[Instruction],
        jump_index: usize,
        instruction_addresses: &BTreeSet<u32>,
        result: &mut AnalysisResult,
        queue_external_entry_target: &impl Fn(&mut AnalysisResult, u32),
    ) -> bool {
        let jr_inst = &instructions[jump_index];
        let jr_reg = jr_inst.rs;
        let in_function =
            |target: u32| target >= function.start && target < function.end && instruction_addresses.contains(&target);

        let jr_index = jump_index as i64;
        let mut found_table = false;

        // The nearest `lw`/`lwu` into the jump register, within 20 instructions.
        let mut lw_index: i64 = -1;
        let mut base_reg = 0u32;
        let mut lw_offset = 0i32;
        let mut i = jr_index - 1;
        while i >= 0 && i >= jr_index - 20 {
            let inst = &instructions[i as usize];
            if (inst.opcode == OPCODE_LW || inst.opcode == OPCODE_LWU) && inst.rt == jr_reg {
                lw_index = i;
                base_reg = inst.rs;
                lw_offset = inst.simmediate as i32;
                break;
            }
            i -= 1;
        }

        if lw_index == -1 {
            return false;
        }

        // The `addu base, table, index` feeding the load, within 10 instructions.
        let mut addu_index: i64 = -1;
        let mut table_base_reg = 0u32;
        let mut index_reg = 0u32;
        let mut i = lw_index - 1;
        while i >= 0 && i >= lw_index - 10 {
            let inst = &instructions[i as usize];
            if inst.opcode == OPCODE_SPECIAL && inst.function == SPECIAL_ADDU && inst.rd == base_reg {
                addu_index = i;
                table_base_reg = inst.rs;
                index_reg = inst.rt;
                break;
            }
            i -= 1;
        }

        let mut table_address = 0u32;
        let mut found_table_address = false;

        if addu_index != -1 {
            let mut i = addu_index - 1;
            while i >= 0 && i >= addu_index - 20 {
                let inst = &instructions[i as usize];
                if inst.opcode == OPCODE_LUI && (inst.rt == table_base_reg || inst.rt == index_reg) {
                    let high = inst.immediate << 16;
                    let mut low = 0u32;
                    for low_inst in &instructions[(i + 1) as usize..addu_index as usize] {
                        if low_inst.rs == inst.rt && low_inst.rt == inst.rt {
                            if low_inst.opcode == OPCODE_ADDIU {
                                low = low_inst.simmediate;
                            } else if low_inst.opcode == OPCODE_ORI {
                                low = low_inst.immediate;
                            }
                        }
                    }
                    table_address = high.wrapping_add(low);
                    found_table_address = true;
                    break;
                }
                i -= 1;
            }
        }

        if !found_table_address {
            return false;
        }
        table_address = table_address.wrapping_add(lw_offset as u32);

        if let Some(configured) = self.configured_jump_tables.get(&table_address) {
            let mut jr_targets: Vec<u32> = Vec::with_capacity(configured.len());
            for &target in configured {
                if in_function(target) {
                    jr_targets.push(target);
                } else {
                    queue_external_entry_target(result, target);
                }
            }

            if !jr_targets.is_empty() {
                jr_targets.sort_unstable();
                jr_targets.dedup();
                result.jump_table_targets.insert(jr_inst.address, jr_targets.clone());
                for target in jr_targets {
                    result.entry_points.insert(target);
                }
                found_table = true;
            }
        }

        // The unshifted index register: the `sll` that scaled it, within 10 instructions.
        let mut unshifted_index_reg = 0u32;
        let mut i = addu_index - 1;
        while i >= 0 && i >= addu_index - 10 {
            let inst = &instructions[i as usize];
            if inst.opcode == OPCODE_SPECIAL
                && inst.function == SPECIAL_SLL
                && (inst.rd == table_base_reg || inst.rd == index_reg)
            {
                unshifted_index_reg = inst.rt;
                break;
            }
            i -= 1;
        }

        // The bounds check (`sltiu`/`slti`) on it, within 30 instructions.
        let mut num_cases = 0u32;
        if unshifted_index_reg != 0 {
            let mut i = addu_index - 1;
            while i >= 0 && i >= addu_index - 30 {
                let inst = &instructions[i as usize];
                if (inst.opcode == OPCODE_SLTIU || inst.opcode == OPCODE_SLTI) && inst.rs == unshifted_index_reg {
                    num_cases = inst.immediate;
                    break;
                }
                i -= 1;
            }
        }

        if !found_table && num_cases > 0 && num_cases <= 1000 {
            let rodata = self.sections.iter().find(|section| section.contains(table_address));

            if let Some(data) = rodata.and_then(|section| section.data.as_deref().map(|data| (section, data))) {
                let (section, bytes) = data;
                let mut jr_targets: Vec<u32> = Vec::new();
                let mut valid_jump_table = true;
                let mut unique_targets: BTreeSet<u32> = BTreeSet::new();
                for i in 0..num_cases {
                    let address = table_address.wrapping_add(i.wrapping_mul(4));
                    if address >= section.address
                        && u64::from(address) + 4 <= u64::from(section.address) + u64::from(section.size)
                    {
                        let offset = (address - section.address) as usize;
                        let target = match bytes.get(offset..offset + 4) {
                            Some(word) => u32::from_le_bytes([word[0], word[1], word[2], word[3]]),
                            None => {
                                valid_jump_table = false;
                                break;
                            }
                        };
                        if in_function(target) {
                            if unique_targets.insert(target) {
                                jr_targets.push(target);
                            }
                        } else {
                            queue_external_entry_target(result, target);
                        }
                    } else {
                        valid_jump_table = false;
                        break;
                    }
                }
                if valid_jump_table && !jr_targets.is_empty() {
                    result.jump_table_targets.insert(jr_inst.address, jr_targets.clone());
                    for target in jr_targets {
                        result.entry_points.insert(target);
                    }
                    found_table = true;
                }
            }
        }

        found_table
    }
}
