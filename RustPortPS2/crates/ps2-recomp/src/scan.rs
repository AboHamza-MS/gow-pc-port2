//! Heuristic discovery of function starts in a stripped ELF.
//!
//! Port of `ScanFunctionStartsFallback` and its helpers in `elf_parser.cpp`:
//! `jal` targets, code addresses materialized with `lui` + `addiu`/`ori`/`lw`
//! that flow into a call, a store or a jump, clustered function pointer tables
//! in data, and back-to-back leaf thunks. The retail ELF carries no symbols, so
//! without a Ghidra map this is the only source of function boundaries.
//!
//! The DWARF pass of the C++ (`libdwarf`) is not ported: the retail game has no
//! debug sections, and an ELF that does is reported by the driver.

use std::collections::{BTreeSet, HashSet};

use ps2_elf::{ElfFile, Function, Section};
use ps2_r5900::opcodes::*;

const INSTRUCTION_SIZE: u32 = 4;
const JUMP_REGION_MASK: u32 = 0xF000_0000;
const IMMEDIATE_SIGN_BIT: u32 = 0x8000;

const GPR_FP_REG: u32 = 30;
const GPR_GP_REG: u32 = 28;
const GPR_SP_REG: u32 = 29;
const GPR_RA_REG: u32 = 31;

const CALLEE_PRESERVED_GPR_MASK: u32 = (1 << GPR_S0)
    | (1 << GPR_S1)
    | (1 << GPR_S2)
    | (1 << GPR_S3)
    | (1 << GPR_S4)
    | (1 << GPR_S5)
    | (1 << GPR_S6)
    | (1 << GPR_S7)
    | (1 << GPR_GP_REG)
    | (1 << GPR_SP_REG)
    | (1 << GPR_FP_REG);

/// Retail EE code also uses t0-t3 as extended call arguments for local helpers.
const ARGUMENT_MASK: u32 = (1 << GPR_A0)
    | (1 << GPR_A1)
    | (1 << GPR_A2)
    | (1 << GPR_A3)
    | (1 << GPR_T0)
    | (1 << GPR_T1)
    | (1 << GPR_T2)
    | (1 << GPR_T3);

fn opcode(raw: u32) -> u32 {
    (raw >> 26) & 0x3F
}
fn rs(raw: u32) -> u32 {
    (raw >> 21) & 0x1F
}
fn rt(raw: u32) -> u32 {
    (raw >> 16) & 0x1F
}
fn rd(raw: u32) -> u32 {
    (raw >> 11) & 0x1F
}
fn sa(raw: u32) -> u32 {
    (raw >> 6) & 0x1F
}
fn function(raw: u32) -> u32 {
    raw & 0x3F
}
fn immediate(raw: u32) -> u32 {
    raw & 0xFFFF
}
fn simmediate(raw: u32) -> i32 {
    i32::from(raw as u16 as i16)
}

/// `MakeAutoFunctionName`.
fn auto_function_name(address: u32) -> String {
    format!("sub_{address:08X}")
}

/// `FindCodeSectionByAddress`.
fn find_code_section<'s, 'a>(sections: &'s [Section<'a>], address: u32) -> Option<&'s Section<'a>> {
    sections.iter().find(|section| section.is_code && section.contains(address))
}

/// `FindSectionByAddress`.
fn find_section<'s, 'a>(sections: &'s [Section<'a>], address: u32) -> Option<&'s Section<'a>> {
    sections.iter().find(|section| section.contains(address))
}

/// `ReadSectionWord`.
fn read_section_word(section: &Section<'_>, offset: u32) -> Option<u32> {
    let data = section.data?;
    if offset > section.size || section.size - offset < INSTRUCTION_SIZE {
        return None;
    }
    let start = offset as usize;
    let word = data.get(start..start + 4)?;
    Some(u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
}

/// `ReadWordByAddress`: real sections first, then the synthetic `LOAD` ones.
fn read_word_by_address(sections: &[Section<'_>], address: u32) -> Option<u32> {
    for pass in 0..2 {
        for section in sections {
            let is_synthetic_load = section.name.starts_with("LOAD");
            if (pass == 0 && is_synthetic_load)
                || (pass == 1 && !is_synthetic_load)
                || section.is_bss
                || section.data.is_none()
                || address < section.address
            {
                continue;
            }
            if let Some(word) = read_section_word(section, address - section.address) {
                return Some(word);
            }
        }
    }
    None
}

fn writes_gpr(raw: u32, reg: u32) -> bool {
    if reg == GPR_ZERO {
        return false;
    }

    let op = opcode(raw);
    if op == OPCODE_SPECIAL || op == OPCODE_MMI {
        return rd(raw) == reg;
    }
    if op == OPCODE_JAL {
        return reg == GPR_RA_REG;
    }

    let writes_rt = matches!(
        op,
        OPCODE_ADDI
            | OPCODE_ADDIU
            | OPCODE_SLTI
            | OPCODE_SLTIU
            | OPCODE_ANDI
            | OPCODE_ORI
            | OPCODE_XORI
            | OPCODE_LUI
            | OPCODE_DADDI
            | OPCODE_DADDIU
            | OPCODE_LDL
            | OPCODE_LDR
            | OPCODE_LQ
            | OPCODE_LB
            | OPCODE_LH
            | OPCODE_LWL
            | OPCODE_LW
            | OPCODE_LBU
            | OPCODE_LHU
            | OPCODE_LWR
            | OPCODE_LWU
            | OPCODE_LL
            | OPCODE_LLD
            | OPCODE_LD
    );
    writes_rt && rt(raw) == reg
}

fn is_control_transfer(raw: u32) -> bool {
    let op = opcode(raw);
    if matches!(
        op,
        OPCODE_REGIMM
            | OPCODE_J
            | OPCODE_JAL
            | OPCODE_BEQ
            | OPCODE_BNE
            | OPCODE_BLEZ
            | OPCODE_BGTZ
            | OPCODE_BEQL
            | OPCODE_BNEL
            | OPCODE_BLEZL
            | OPCODE_BGTZL
    ) {
        return true;
    }
    op == OPCODE_SPECIAL && matches!(function(raw), SPECIAL_JR | SPECIAL_JALR)
}

fn is_call_instruction(raw: u32) -> bool {
    let op = opcode(raw);
    op == OPCODE_JAL || (op == OPCODE_SPECIAL && function(raw) == SPECIAL_JALR)
}

/// `TryGetGprCopy`: `(destination, source)` when the instruction is a register move.
fn gpr_copy(raw: u32) -> Option<(u32, u32)> {
    let op = opcode(raw);
    let (rs, rt, rd) = (rs(raw), rt(raw), rd(raw));

    if op == OPCODE_SPECIAL {
        let f = function(raw);
        let zero_extended_move = matches!(f, SPECIAL_ADDU | SPECIAL_DADDU | SPECIAL_OR);
        if zero_extended_move && rd != GPR_ZERO {
            if rs == GPR_ZERO && rt != GPR_ZERO {
                return Some((rd, rt));
            }
            if rt == GPR_ZERO && rs != GPR_ZERO {
                return Some((rd, rs));
            }
        }

        if f == SPECIAL_SLL && sa(raw) == 0 && rd != GPR_ZERO && rt != GPR_ZERO {
            return Some((rd, rt));
        }
    }

    if matches!(op, OPCODE_ADDIU | OPCODE_DADDIU | OPCODE_ORI)
        && immediate(raw) == 0
        && rs != GPR_ZERO
        && rt != GPR_ZERO
    {
        return Some((rt, rs));
    }

    None
}

fn propagate_tracked_gprs(raw: u32, tracked: u32) -> u32 {
    let mut tracked = tracked;
    let copy = gpr_copy(raw).filter(|(_, source)| tracked & (1 << source) != 0);

    for reg in 1..32u32 {
        if writes_gpr(raw, reg) {
            tracked &= !(1 << reg);
        }
    }

    if let Some((destination, _)) = copy {
        tracked |= 1 << destination;
    }
    tracked
}

fn is_conditional_branch(raw: u32) -> bool {
    match opcode(raw) {
        OPCODE_BEQ | OPCODE_BNE | OPCODE_BLEZ | OPCODE_BGTZ | OPCODE_BEQL | OPCODE_BNEL | OPCODE_BLEZL
        | OPCODE_BGTZL => true,
        OPCODE_REGIMM => matches!(
            rt(raw),
            REGIMM_BLTZ
                | REGIMM_BGEZ
                | REGIMM_BLTZL
                | REGIMM_BGEZL
                | REGIMM_BLTZAL
                | REGIMM_BGEZAL
                | REGIMM_BLTZALL
                | REGIMM_BGEZALL
        ),
        _ => false,
    }
}

fn is_likely_branch(raw: u32) -> bool {
    match opcode(raw) {
        OPCODE_BEQL | OPCODE_BNEL | OPCODE_BLEZL | OPCODE_BGTZL => true,
        OPCODE_REGIMM => matches!(rt(raw), REGIMM_BLTZL | REGIMM_BGEZL | REGIMM_BLTZALL | REGIMM_BGEZALL),
        _ => false,
    }
}

fn pc_relative_branch_target_offset(instruction_offset: u32, raw: u32) -> u32 {
    instruction_offset.wrapping_add(INSTRUCTION_SIZE).wrapping_add((simmediate(raw).wrapping_mul(4)) as u32)
}

fn direct_jump_target_offset(section: &Section<'_>, instruction_offset: u32, raw: u32) -> Option<u32> {
    if opcode(raw) != OPCODE_J && opcode(raw) != OPCODE_JAL {
        return None;
    }

    let instruction_address = section.address.wrapping_add(instruction_offset);
    let target_address =
        (instruction_address.wrapping_add(INSTRUCTION_SIZE) & JUMP_REGION_MASK) | ((raw & 0x03FF_FFFF) << 2);
    if target_address < section.address || target_address >= section.address.wrapping_add(section.size) {
        return None;
    }
    Some(target_address - section.address)
}

fn has_reachable_return_by_control_flow(section: &Section<'_>, start_offset: u32) -> bool {
    // This is a corruption/cycle guard.
    const SAFETY_BUDGET: usize = 4096;

    let mut pending: Vec<u32> = vec![start_offset];
    let mut visited: HashSet<u32> = HashSet::with_capacity(256);

    let is_readable_offset =
        |offset: u32| offset % INSTRUCTION_SIZE == 0 && offset <= section.size && section.size - offset >= 4;

    while !pending.is_empty() && visited.len() < SAFETY_BUDGET {
        let Some(mut offset) = pending.pop() else {
            break;
        };

        while is_readable_offset(offset) && visited.len() < SAFETY_BUDGET && visited.insert(offset) {
            let Some(raw) = read_section_word(section, offset) else {
                break;
            };

            let op = opcode(raw);
            if op == OPCODE_SPECIAL && function(raw) == SPECIAL_JR {
                if rs(raw) == GPR_RA_REG {
                    return true;
                }
                break;
            }

            if is_conditional_branch(raw) {
                let target_offset = pc_relative_branch_target_offset(offset, raw);
                let is_unconditional_branch = op == OPCODE_BEQ && rs(raw) == GPR_ZERO && rt(raw) == GPR_ZERO;

                if is_unconditional_branch {
                    if !is_readable_offset(target_offset) {
                        break;
                    }
                    offset = target_offset;
                    continue;
                }

                if is_readable_offset(target_offset) {
                    pending.push(target_offset);
                }

                // Both ordinary and likely branches resume after their delay slot on the
                // fallthrough path. The delay instruction cannot legally contain another
                // control transfer.
                offset = offset.wrapping_add(2 * INSTRUCTION_SIZE);
                continue;
            }

            if op == OPCODE_J {
                let Some(target_offset) = direct_jump_target_offset(section, offset, raw) else {
                    break;
                };
                offset = target_offset;
                continue;
            }

            if op == OPCODE_JAL || (op == OPCODE_SPECIAL && function(raw) == SPECIAL_JALR) {
                // Calls return after their delay slot; the callee is a separate control-flow
                // region and is intentionally not traversed here.
                offset = offset.wrapping_add(2 * INSTRUCTION_SIZE);
                continue;
            }

            offset = offset.wrapping_add(INSTRUCTION_SIZE);
        }
    }

    false
}

/// `LooksLikeCallableEntry`.
fn looks_like_callable_entry(
    sections: &[Section<'_>],
    address: u32,
    allow_leaf_thunk: bool,
    allow_long_leaf: bool,
    restrict_tail_thunk: bool,
) -> bool {
    if address % INSTRUCTION_SIZE != 0 {
        return false;
    }

    let Some(section) = find_code_section(sections, address) else {
        return false;
    };
    if section.data.is_none() {
        return false;
    }

    let start_offset = address - section.address;
    const STANDARD_PROBE_WORDS: u32 = 8;
    // Retail initializer tables sometimes point at functions that materialize a sizeable block of
    // constants before allocating their stack frame. The broader window is exclusive to that
    // LUI-preamble pattern.
    const DELAYED_PROLOGUE_PROBE_WORDS: u32 = 12;
    let probe_words = DELAYED_PROLOGUE_PROBE_WORDS.max(STANDARD_PROBE_WORDS);
    let mut has_only_constant_preamble = true;

    for index in 0..probe_words {
        let Some(raw) = read_section_word(section, start_offset.wrapping_add(index * INSTRUCTION_SIZE)) else {
            break;
        };

        let op = opcode(raw);
        let (rs, rt) = (rs(raw), rt(raw));
        let imm = immediate(raw);

        // Non-leaf functions normally allocate their stack frame immediately. Static initializers
        // may first issue a block of LUI instructions, as long as no unrelated operation appears.
        if (op == OPCODE_ADDIU || op == OPCODE_DADDIU)
            && rs == GPR_SP_REG
            && rt == GPR_SP_REG
            && (imm & IMMEDIATE_SIGN_BIT) != 0
            && index < DELAYED_PROLOGUE_PROBE_WORDS
            && (index < 4 || has_only_constant_preamble)
        {
            return true;
        }

        // Some prologues set up GP before saving RA, so also recognize SW/SD/SQ $ra,offset($sp).
        if index < STANDARD_PROBE_WORDS
            && matches!(op, OPCODE_SW | OPCODE_SD | OPCODE_SQ)
            && rs == GPR_SP_REG
            && rt == GPR_RA_REG
        {
            return true;
        }

        // Leaf callbacks and vtable thunks often have no stack frame at all: besides `jr $ra`,
        // constructor/destructor wrappers tail-call their implementation with a direct `j`.
        let is_leaf_return =
            index < STANDARD_PROBE_WORDS && op == OPCODE_SPECIAL && function(raw) == SPECIAL_JR && rs == GPR_RA_REG;
        let tail_thunk_probe_words = if restrict_tail_thunk { 4 } else { STANDARD_PROBE_WORDS };
        let is_short_tail_thunk = index < tail_thunk_probe_words && op == OPCODE_J;
        if allow_leaf_thunk && (is_leaf_return || is_short_tail_thunk) {
            return true;
        }

        if op != OPCODE_LUI {
            has_only_constant_preamble = false;
        }
    }

    // A callback supported by strong address-taken evidence does not need a prologue: follow its
    // reachable basic blocks instead.
    allow_leaf_thunk && allow_long_leaf && has_reachable_return_by_control_flow(section, start_offset)
}

#[derive(Clone, Copy)]
struct TrackedGprState {
    offset: u32,
    tracked_gprs: u32,
}

/// The `enqueue` lambda: only offsets that fit a whole instruction in the region.
fn enqueue(worklist: &mut Vec<TrackedGprState>, offset: u32, registers: u32, region_begin: u32, region_end: u32) {
    if registers != 0 && offset >= region_begin && offset.wrapping_add(INSTRUCTION_SIZE) <= region_end {
        worklist.push(TrackedGprState { offset, tracked_gprs: registers });
    }
}

/// `IsConsumedAsCodePointerInControlFlow`.
fn is_consumed_as_code_pointer_in_control_flow(
    section: &Section<'_>,
    first_instruction_offset: u32,
    tracked_gprs: u32,
    region_begin: u32,
    region_end: u32,
) -> bool {
    let mut worklist: Vec<TrackedGprState> = Vec::new();
    let mut visited: HashSet<u64> = HashSet::new();
    enqueue(&mut worklist, first_instruction_offset, tracked_gprs, region_begin, region_end);

    while let Some(state) = worklist.pop() {
        let visit_key = (u64::from(state.offset) << 32) | u64::from(state.tracked_gprs);
        if !visited.insert(visit_key) {
            continue;
        }

        let Some(raw) = read_section_word(section, state.offset) else {
            continue;
        };

        let op = opcode(raw);
        if matches!(op, OPCODE_SW | OPCODE_SD | OPCODE_SQ) && (state.tracked_gprs & (1 << rt(raw))) != 0 {
            // Some retail C++ runtimes construct vtables and interface descriptors in writable
            // memory: a materialized code address stored into an object is address-taken evidence.
            return true;
        }

        if is_call_instruction(raw) {
            let is_tracked_indirect_target = op == OPCODE_SPECIAL && (state.tracked_gprs & (1 << rs(raw))) != 0;
            if is_tracked_indirect_target {
                return true;
            }

            let mut after_delay = state.tracked_gprs;
            if let Some(delay_raw) = read_section_word(section, state.offset.wrapping_add(INSTRUCTION_SIZE)) {
                after_delay = propagate_tracked_gprs(delay_raw, after_delay);
            }
            if (after_delay & ARGUMENT_MASK) != 0 {
                return true;
            }

            // A normal call may clobber caller-saved registers, but values deliberately parked in
            // s0-s7/fp remain live afterwards.
            enqueue(
                &mut worklist,
                state.offset.wrapping_add(2 * INSTRUCTION_SIZE),
                after_delay & CALLEE_PRESERVED_GPR_MASK,
                region_begin,
                region_end,
            );
            continue;
        }

        if is_conditional_branch(raw) {
            let mut delay_tracked = state.tracked_gprs;
            if let Some(delay_raw) = read_section_word(section, state.offset.wrapping_add(INSTRUCTION_SIZE)) {
                delay_tracked = propagate_tracked_gprs(delay_raw, delay_tracked);
            }

            enqueue(
                &mut worklist,
                pc_relative_branch_target_offset(state.offset, raw),
                delay_tracked,
                region_begin,
                region_end,
            );
            enqueue(
                &mut worklist,
                state.offset.wrapping_add(2 * INSTRUCTION_SIZE),
                if is_likely_branch(raw) { state.tracked_gprs } else { delay_tracked },
                region_begin,
                region_end,
            );
            continue;
        }

        if op == OPCODE_J {
            let mut delay_tracked = state.tracked_gprs;
            if let Some(delay_raw) = read_section_word(section, state.offset.wrapping_add(INSTRUCTION_SIZE)) {
                delay_tracked = propagate_tracked_gprs(delay_raw, delay_tracked);
            }

            match direct_jump_target_offset(section, state.offset, raw) {
                Some(target_offset) => enqueue(&mut worklist, target_offset, delay_tracked, region_begin, region_end),
                None if (delay_tracked & ARGUMENT_MASK) != 0 => {
                    // A direct jump outside the current coarse function is a tail call, and
                    // observes the same argument registers.
                    return true;
                }
                None => {}
            }
            continue;
        }

        if op == OPCODE_SPECIAL && function(raw) == SPECIAL_JR {
            // A materialized code address used as a JR target is a tail callback even though it
            // does not write $ra.
            if (state.tracked_gprs & (1 << rs(raw))) != 0 {
                return true;
            }
            continue;
        }

        if is_control_transfer(raw) {
            continue;
        }

        let next_tracked = propagate_tracked_gprs(raw, state.tracked_gprs);
        enqueue(&mut worklist, state.offset.wrapping_add(INSTRUCTION_SIZE), next_tracked, region_begin, region_end);
    }

    false
}

/// `TryCompleteCodeAddress`: `(target, value register)` of an `addiu`/`ori` on the upper half.
fn try_complete_code_address(raw: u32, upper_value: u32, upper_aliases: u32) -> Option<(u32, u32)> {
    let op = opcode(raw);
    if !matches!(op, OPCODE_ADDIU | OPCODE_DADDIU | OPCODE_ORI) || (upper_aliases & (1 << rs(raw))) == 0 {
        return None;
    }

    let imm = immediate(raw);
    let target = if op == OPCODE_ORI { upper_value | imm } else { upper_value.wrapping_add(simmediate(raw) as u32) };
    Some((target, rt(raw)))
}

/// `TryLoadCodeAddressFromInitializedData`.
fn try_load_code_address_from_initialized_data(
    sections: &[Section<'_>],
    raw: u32,
    upper_value: u32,
    upper_aliases: u32,
) -> Option<(u32, u32)> {
    let op = opcode(raw);
    let base_reg = rs(raw);
    if (op != OPCODE_LW && op != OPCODE_LWU) || (upper_aliases & (1 << base_reg)) == 0 {
        return None;
    }

    let slot_address = upper_value.wrapping_add(simmediate(raw) as u32);
    let target = read_word_by_address(sections, slot_address)?;
    if target % INSTRUCTION_SIZE != 0 {
        return None;
    }

    let target_section = find_code_section(sections, target)?;
    let target_offset = target - target_section.address;
    if target_offset >= INSTRUCTION_SIZE {
        if let Some(predecessor) = read_section_word(target_section, target_offset - INSTRUCTION_SIZE) {
            if is_control_transfer(predecessor) {
                // A loaded code-looking value that lands immediately after a control transfer names
                // its delay slot. Promoting it would cut the real owner before the delay instruction.
                return None;
            }
        }
    }

    let value_reg = rt(raw);
    (value_reg != GPR_ZERO).then_some((target, value_reg))
}

#[allow(clippy::too_many_arguments)]
fn is_materialized_address_consumed(
    sections: &[Section<'_>],
    section: &Section<'_>,
    target: u32,
    value_reg: u32,
    first_use_offset: u32,
    region_begin: u32,
    region_end: u32,
    consumed_by_current_call: bool,
    starts: &mut BTreeSet<u32>,
) -> bool {
    let materialized_as_code_pointer = consumed_by_current_call
        || is_consumed_as_code_pointer_in_control_flow(
            section,
            first_use_offset,
            1 << value_reg,
            region_begin,
            region_end,
        );

    if !looks_like_callable_entry(
        sections,
        target,
        materialized_as_code_pointer,
        materialized_as_code_pointer,
        materialized_as_code_pointer,
    ) {
        return false;
    }

    starts.insert(target);
    true
}

/// `ScanMaterializedCodeAddresses`.
fn scan_materialized_code_addresses(sections: &[Section<'_>], starts: &mut BTreeSet<u32>) {
    for section in sections {
        if !section.is_code || section.data.is_none() || section.size < 2 * INSTRUCTION_SIZE {
            continue;
        }

        // JAL targets known before this pass form conservative function boundaries. CFG traversal
        // may cross basic blocks but never leaks into the next coarse function.
        let function_boundaries: Vec<u32> = starts
            .iter()
            .filter(|&&start| start >= section.address && start < section.address.wrapping_add(section.size))
            .map(|&start| start - section.address)
            .collect();

        let mut offset = 0u32;
        while offset.wrapping_add(INSTRUCTION_SIZE) <= section.size {
            let current_offset = offset;
            offset = offset.wrapping_add(INSTRUCTION_SIZE);

            let Some(upper_raw) = read_section_word(section, current_offset) else {
                continue;
            };
            if opcode(upper_raw) != OPCODE_LUI {
                continue;
            }

            let upper_reg = rt(upper_raw);
            if upper_reg == GPR_ZERO {
                continue;
            }

            let next_boundary = function_boundaries.partition_point(|&boundary| boundary <= current_offset);
            let region_begin = if next_boundary == 0 { 0 } else { function_boundaries[next_boundary - 1] };
            let region_end = function_boundaries.get(next_boundary).copied().unwrap_or(section.size);
            let upper_value = immediate(upper_raw) << 16;
            let initial_aliases = 1u32 << upper_reg;

            let mut worklist: Vec<TrackedGprState> = Vec::new();
            let mut visited: HashSet<u64> = HashSet::new();

            let mut starts_after_predecessor_transfer = false;
            if current_offset >= INSTRUCTION_SIZE {
                if let Some(predecessor_raw) = read_section_word(section, current_offset - INSTRUCTION_SIZE) {
                    if is_conditional_branch(predecessor_raw) {
                        enqueue(
                            &mut worklist,
                            pc_relative_branch_target_offset(current_offset - INSTRUCTION_SIZE, predecessor_raw),
                            initial_aliases,
                            region_begin,
                            region_end,
                        );
                        if !is_likely_branch(predecessor_raw) {
                            enqueue(
                                &mut worklist,
                                current_offset + INSTRUCTION_SIZE,
                                initial_aliases,
                                region_begin,
                                region_end,
                            );
                        }
                        starts_after_predecessor_transfer = true;
                    } else if opcode(predecessor_raw) == OPCODE_J {
                        if let Some(target_offset) =
                            direct_jump_target_offset(section, current_offset - INSTRUCTION_SIZE, predecessor_raw)
                        {
                            enqueue(&mut worklist, target_offset, initial_aliases, region_begin, region_end);
                        }
                        starts_after_predecessor_transfer = true;
                    } else if is_call_instruction(predecessor_raw)
                        || (opcode(predecessor_raw) == OPCODE_SPECIAL && function(predecessor_raw) == SPECIAL_JR)
                    {
                        // The LUI ran in a call/jump delay slot. An incomplete address cannot
                        // survive through an unknown callee or indirect target.
                        starts_after_predecessor_transfer = true;
                    }
                }
            }
            if !starts_after_predecessor_transfer {
                enqueue(&mut worklist, current_offset + INSTRUCTION_SIZE, initial_aliases, region_begin, region_end);
            }

            while let Some(state) = worklist.pop() {
                let visit_key = (u64::from(state.offset) << 32) | u64::from(state.tracked_gprs);
                if !visited.insert(visit_key) {
                    continue;
                }

                let Some(raw) = read_section_word(section, state.offset) else {
                    continue;
                };

                if let Some((target, value_reg)) =
                    try_load_code_address_from_initialized_data(sections, raw, upper_value, state.tracked_gprs)
                {
                    // A singleton callback slot is strong evidence only when the loaded value
                    // actually flows into a call/jump (or a callback store).
                    is_materialized_address_consumed(
                        sections,
                        section,
                        target,
                        value_reg,
                        state.offset + INSTRUCTION_SIZE,
                        region_begin,
                        region_end,
                        false,
                        starts,
                    );

                    // Keep following the data-slot base as well: one function can load several
                    // callbacks from the same descriptor.
                    let next_aliases = propagate_tracked_gprs(raw, state.tracked_gprs);
                    enqueue(&mut worklist, state.offset + INSTRUCTION_SIZE, next_aliases, region_begin, region_end);
                    continue;
                }

                if let Some((target, value_reg)) = try_complete_code_address(raw, upper_value, state.tracked_gprs) {
                    is_materialized_address_consumed(
                        sections,
                        section,
                        target,
                        value_reg,
                        state.offset + INSTRUCTION_SIZE,
                        region_begin,
                        region_end,
                        false,
                        starts,
                    );
                    continue;
                }

                if is_control_transfer(raw) {
                    let mut delay_aliases = state.tracked_gprs;
                    let delay_raw = read_section_word(section, state.offset + INSTRUCTION_SIZE);
                    if let Some((target, value_reg)) =
                        delay_raw.and_then(|delay| try_complete_code_address(delay, upper_value, delay_aliases))
                    {
                        let current_is_call = is_call_instruction(raw);
                        let first_use_offset = state.offset + 2 * INSTRUCTION_SIZE;
                        is_materialized_address_consumed(
                            sections,
                            section,
                            target,
                            value_reg,
                            first_use_offset,
                            region_begin,
                            region_end,
                            current_is_call && (GPR_A0..=GPR_A3).contains(&value_reg),
                            starts,
                        );
                        continue;
                    }
                    if let Some(delay) = delay_raw {
                        delay_aliases = propagate_tracked_gprs(delay, delay_aliases);
                    }

                    if is_conditional_branch(raw) {
                        enqueue(
                            &mut worklist,
                            pc_relative_branch_target_offset(state.offset, raw),
                            delay_aliases,
                            region_begin,
                            region_end,
                        );
                        enqueue(
                            &mut worklist,
                            state.offset + 2 * INSTRUCTION_SIZE,
                            if is_likely_branch(raw) { state.tracked_gprs } else { delay_aliases },
                            region_begin,
                            region_end,
                        );
                    } else if opcode(raw) == OPCODE_J {
                        if let Some(target_offset) = direct_jump_target_offset(section, state.offset, raw) {
                            enqueue(&mut worklist, target_offset, delay_aliases, region_begin, region_end);
                        }
                    } else if is_call_instruction(raw) {
                        // An upper half deliberately kept in s0-s7/fp/gp survives ordinary calls.
                        enqueue(
                            &mut worklist,
                            state.offset + 2 * INSTRUCTION_SIZE,
                            delay_aliases & CALLEE_PRESERVED_GPR_MASK,
                            region_begin,
                            region_end,
                        );
                    }
                    continue;
                }

                let next_aliases = propagate_tracked_gprs(raw, state.tracked_gprs);
                enqueue(&mut worklist, state.offset + INSTRUCTION_SIZE, next_aliases, region_begin, region_end);
            }
        }
    }
}

fn is_dedicated_function_pointer_section(name: &str) -> bool {
    matches!(name, ".ctors" | ".dtors" | ".init_array" | ".fini_array")
}

struct PointerCandidate {
    source_offset: u32,
    target: u32,
    has_conservative_shape: bool,
}

/// `ScanDataFunctionPointerTables`.
fn scan_data_function_pointer_tables(sections: &[Section<'_>], starts: &mut BTreeSet<u32>) {
    const CLUSTER_DISTANCE_BYTES: u32 = 32;

    for section in sections {
        if !section.is_data
            || section.is_code
            || section.is_bss
            || section.data.is_none()
            || section.size < INSTRUCTION_SIZE
        {
            continue;
        }

        let mut candidates: Vec<PointerCandidate> = Vec::new();
        let mut offset = 0u32;
        while offset.wrapping_add(INSTRUCTION_SIZE) <= section.size {
            if let Some(target) = read_section_word(section, offset) {
                if target % INSTRUCTION_SIZE == 0 && find_code_section(sections, target).is_some() {
                    candidates.push(PointerCandidate {
                        source_offset: offset,
                        target,
                        has_conservative_shape: looks_like_callable_entry(sections, target, true, false, false),
                    });
                }
            }
            offset += INSTRUCTION_SIZE;
        }

        let dedicated_pointer_section = is_dedicated_function_pointer_section(&section.name);

        let has_conservative_neighbor_within = |candidate_index: usize, max_distance: u32| -> bool {
            let mut neighbor = candidate_index;
            while neighbor > 0 {
                neighbor -= 1;
                if candidates[candidate_index].source_offset - candidates[neighbor].source_offset > max_distance {
                    break;
                }
                if candidates[neighbor].has_conservative_shape {
                    return true;
                }
            }

            for neighbor in candidate_index + 1..candidates.len() {
                if candidates[neighbor].source_offset - candidates[candidate_index].source_offset > max_distance {
                    break;
                }
                if candidates[neighbor].has_conservative_shape {
                    return true;
                }
            }

            false
        };

        // Several retail engines describe a class as: name pointer, ordinary method, 0, 0, long
        // leaf method. The ordinary method gives a conservative executable anchor, while the two
        // reserved words and the data pointer distinguish it from an accidental code address.
        let has_class_descriptor_shape = |candidate_index: usize| -> bool {
            let method_offset = candidates[candidate_index].source_offset;
            const DESCRIPTOR_PREFIX_BYTES: u32 = 16;
            if method_offset < DESCRIPTOR_PREFIX_BYTES {
                return false;
            }

            let (Some(name_address), Some(conservative_method), Some(reserved0), Some(reserved1)) = (
                read_section_word(section, method_offset - 16),
                read_section_word(section, method_offset - 12),
                read_section_word(section, method_offset - 8),
                read_section_word(section, method_offset - 4),
            ) else {
                return false;
            };
            if reserved0 != 0 || reserved1 != 0 {
                return false;
            }

            find_section(sections, name_address)
                .is_some_and(|name_section| name_section.is_data && !name_section.is_code)
                && looks_like_callable_entry(sections, conservative_method, true, false, false)
        };

        let has_alternating_pointer_id_shape = |candidate_index: usize| -> bool {
            let is_small_id_at = |offset: u32| -> bool {
                offset.wrapping_add(4) <= section.size
                    && read_section_word(section, offset).is_some_and(|value| value <= 0xFFFF)
            };

            let candidate = &candidates[candidate_index];
            if !is_small_id_at(candidate.source_offset + 4) {
                return false;
            }

            if candidate_index > 0 {
                let previous = &candidates[candidate_index - 1];
                if previous.has_conservative_shape
                    && previous.source_offset + 8 == candidate.source_offset
                    && is_small_id_at(previous.source_offset + 4)
                {
                    return true;
                }
            }

            if candidate_index + 1 < candidates.len() {
                let next = &candidates[candidate_index + 1];
                if next.has_conservative_shape
                    && candidate.source_offset + 8 == next.source_offset
                    && is_small_id_at(next.source_offset + 4)
                {
                    return true;
                }
            }
            false
        };

        for (index, candidate) in candidates.iter().enumerate() {
            if candidate.has_conservative_shape {
                let clustered =
                    dedicated_pointer_section || has_conservative_neighbor_within(index, CLUSTER_DISTANCE_BYTES);
                if clustered {
                    starts.insert(candidate.target);
                }
                continue;
            }

            let has_conservative_anchor = dedicated_pointer_section
                || has_class_descriptor_shape(index)
                || has_alternating_pointer_id_shape(index);
            if has_conservative_anchor && looks_like_callable_entry(sections, candidate.target, true, true, false) {
                starts.insert(candidate.target);
            }
        }
    }
}

/// `ScanAdjacentLeafThunkRuns`: trivial accessors packed back-to-back after a known start.
fn scan_adjacent_leaf_thunk_runs(sections: &[Section<'_>], starts: &mut BTreeSet<u32>) {
    const LEAF_THUNK_BYTES: u32 = 2 * INSTRUCTION_SIZE;

    let is_return = |raw: u32| opcode(raw) == OPCODE_SPECIAL && function(raw) == SPECIAL_JR && rs(raw) == GPR_RA_REG;

    for section in sections {
        if !section.is_code || section.data.is_none() || section.size < 2 * LEAF_THUNK_BYTES {
            continue;
        }

        let mut offset = LEAF_THUNK_BYTES;
        while offset + LEAF_THUNK_BYTES <= section.size {
            let previous_address = section.address.wrapping_add(offset).wrapping_sub(LEAF_THUNK_BYTES);
            if starts.contains(&previous_address) {
                if let (Some(previous_raw), Some(candidate_raw)) =
                    (read_section_word(section, offset - LEAF_THUNK_BYTES), read_section_word(section, offset))
                {
                    if is_return(previous_raw) && is_return(candidate_raw) {
                        starts.insert(section.address.wrapping_add(offset));
                    }
                }
            }
            offset += INSTRUCTION_SIZE;
        }
    }
}

/// `ScanFunctionStartsFallback`: every function start the heuristics find, with no end.
pub fn scan_function_starts(elf: &ElfFile<'_>) -> Vec<Function> {
    let sections = elf.sections();
    let mut starts: BTreeSet<u32> = BTreeSet::new();

    let entry = elf.entry();
    if find_code_section(sections, entry).is_some() {
        starts.insert(entry);
    }

    for section in sections {
        if !section.is_code || section.data.is_none() || section.size < INSTRUCTION_SIZE {
            continue;
        }

        let mut offset = 0u32;
        while offset.wrapping_add(INSTRUCTION_SIZE) <= section.size {
            let pc = section.address.wrapping_add(offset);
            let Some(raw) = read_section_word(section, offset) else {
                offset += INSTRUCTION_SIZE;
                continue;
            };
            offset += INSTRUCTION_SIZE;

            if opcode(raw) != OPCODE_JAL {
                continue;
            }

            let target = (pc.wrapping_add(INSTRUCTION_SIZE) & JUMP_REGION_MASK) | ((raw & 0x03FF_FFFF) << 2);
            if find_code_section(sections, target).is_some() {
                starts.insert(target);
            }
        }
    }

    scan_materialized_code_addresses(sections, &mut starts);
    scan_data_function_pointer_tables(sections, &mut starts);
    scan_adjacent_leaf_thunk_runs(sections, &mut starts);

    starts
        .into_iter()
        .filter(|&start| find_code_section(sections, start).is_some())
        .map(|start| Function { name: auto_function_name(start), start, end: 0, is_stub: false, is_skipped: false })
        .collect()
}
