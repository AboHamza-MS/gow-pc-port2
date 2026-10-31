//! Constant propagation and GIF DMA kick recognition.
//!
//! Port of `gif_dma_kick_analyzer.cpp`. A guest routine that starts a GIF DMA
//! chain does four `sw` stores to `D_PCR`, `D_STAT`, `GIF TADR` and `GIF CHCR`
//! with addresses it builds from `lui`/`ori`; the recompiler replaces the four
//! stores with one runtime call. The constant tracking here is also what turns
//! `lui`+`sw` pairs into [`MemoryAccessHint`]s for the memory translators.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use ps2_r5900::opcodes::*;
use ps2_r5900::Instruction;

use crate::translate::memory::MemoryAccessHint;

/// `ConstantRegisterState`: which GPRs hold a known 32-bit constant.
#[derive(Debug, Clone)]
pub struct ConstantRegisterState {
    known: [bool; 32],
    values: [u32; 32],
}

impl Default for ConstantRegisterState {
    fn default() -> Self {
        let mut state = Self { known: [false; 32], values: [0; 32] };
        state.clear();
        state
    }
}

impl ConstantRegisterState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.known = [false; 32];
        self.values = [0; 32];
        self.known[0] = true;
    }

    pub fn read(&self, reg: u32) -> Option<u32> {
        let index = reg as usize;
        if index >= self.known.len() || !self.known[index] {
            return None;
        }
        Some(self.values[index])
    }

    pub fn write(&mut self, reg: u32, value: u32) {
        if reg == 0 || reg as usize >= self.known.len() {
            return;
        }
        self.known[reg as usize] = true;
        self.values[reg as usize] = value;
    }

    pub fn invalidate(&mut self, reg: u32) {
        if reg == 0 || reg as usize >= self.known.len() {
            return;
        }
        self.known[reg as usize] = false;
        self.values[reg as usize] = 0;
    }
}

/// `GifDmaKickPlan`.
#[derive(Debug, Clone)]
pub struct GifDmaKickPlan {
    pub valid: bool,
    pub store_indices: [usize; 4],
    pub values: [String; 4],
    pub capture_expressions: [String; 4],
    pub captures: [bool; 4],
    pub branch_index: usize,
    pub end_index: usize,
    pub completes_in_delay_slot: bool,
}

impl Default for GifDmaKickPlan {
    fn default() -> Self {
        Self {
            valid: false,
            store_indices: [0; 4],
            values: Default::default(),
            capture_expressions: Default::default(),
            captures: [false; 4],
            branch_index: usize::MAX,
            end_index: 0,
            completes_in_delay_slot: false,
        }
    }
}

impl GifDmaKickPlan {
    pub fn suppresses(&self, index: usize) -> bool {
        self.valid && self.store_indices.contains(&index)
    }

    pub fn completes_at(&self, index: usize) -> bool {
        self.valid && index == self.end_index
    }

    pub fn slot_for(&self, index: usize) -> usize {
        self.store_indices.iter().position(|&stored| stored == index).unwrap_or(self.store_indices.len())
    }
}

/// `isDirectMemoryAccess`.
pub fn is_direct_memory_access(inst: &Instruction) -> bool {
    matches!(
        inst.opcode,
        OPCODE_LB
            | OPCODE_LH
            | OPCODE_LW
            | OPCODE_LBU
            | OPCODE_LHU
            | OPCODE_LWU
            | OPCODE_LQ
            | OPCODE_LD
            | OPCODE_LWC1
            | OPCODE_LDC2
            | OPCODE_SB
            | OPCODE_SH
            | OPCODE_SW
            | OPCODE_SQ
            | OPCODE_SD
            | OPCODE_SWC1
            | OPCODE_SDC2
    )
}

/// `resolveMemoryAccessHint`.
pub fn resolve_memory_access_hint(inst: &Instruction, constants: &ConstantRegisterState) -> MemoryAccessHint {
    if !is_direct_memory_access(inst) {
        return MemoryAccessHint::default();
    }
    match constants.read(inst.rs) {
        Some(base) => MemoryAccessHint::resolved(base.wrapping_add(inst.simmediate)),
        None => MemoryAccessHint::default(),
    }
}

/// `updateConstantRegisters`.
pub fn update_constant_registers(inst: &Instruction, constants: &mut ConstantRegisterState) {
    match inst.opcode {
        OPCODE_LUI => {
            constants.write(inst.rt, inst.immediate << 16);
            return;
        }
        OPCODE_ORI => {
            match constants.read(inst.rs) {
                Some(lhs) => constants.write(inst.rt, lhs | (inst.immediate & 0xFFFF)),
                None => constants.invalidate(inst.rt),
            }
            return;
        }
        OPCODE_ADDIU => {
            match constants.read(inst.rs) {
                Some(lhs) => constants.write(inst.rt, lhs.wrapping_add(inst.simmediate)),
                None => constants.invalidate(inst.rt),
            }
            return;
        }
        OPCODE_ANDI => {
            match constants.read(inst.rs) {
                Some(lhs) => constants.write(inst.rt, lhs & (inst.immediate & 0xFFFF)),
                None => constants.invalidate(inst.rt),
            }
            return;
        }
        OPCODE_XORI => {
            match constants.read(inst.rs) {
                Some(lhs) => constants.write(inst.rt, lhs ^ (inst.immediate & 0xFFFF)),
                None => constants.invalidate(inst.rt),
            }
            return;
        }
        OPCODE_LB | OPCODE_LH | OPCODE_LWL | OPCODE_LW | OPCODE_LBU | OPCODE_LHU | OPCODE_LWR | OPCODE_LWU
        | OPCODE_LDL | OPCODE_LDR | OPCODE_LQ | OPCODE_LL | OPCODE_LD => {
            constants.invalidate(inst.rt);
            return;
        }
        OPCODE_SC => {
            constants.invalidate(inst.rt);
            return;
        }
        OPCODE_SPECIAL => match inst.function {
            SPECIAL_ADDU | SPECIAL_DADDU => {
                match (constants.read(inst.rs), constants.read(inst.rt)) {
                    (Some(lhs), Some(rhs)) => constants.write(inst.rd, lhs.wrapping_add(rhs)),
                    _ => constants.invalidate(inst.rd),
                }
                return;
            }
            SPECIAL_OR => {
                match (constants.read(inst.rs), constants.read(inst.rt)) {
                    (Some(lhs), Some(rhs)) => constants.write(inst.rd, lhs | rhs),
                    _ => constants.invalidate(inst.rd),
                }
                return;
            }
            _ => {}
        },
        _ => {}
    }

    if inst.modification.modifies_gpr || inst.modification.modifies_control {
        constants.clear();
    }
}

fn gif_dma_kick_temp_name(address: u32, slot: usize) -> String {
    format!("gifDmaKickValue_{address:x}_{slot}")
}

fn is_return_with_delay_slot(inst: &Instruction) -> bool {
    inst.opcode == OPCODE_SPECIAL && inst.function == SPECIAL_JR && inst.rs == 31 && inst.has_delay_slot
}

fn try_match_gif_dma_store(
    instruction_index: usize,
    inst: &Instruction,
    hint: MemoryAccessHint,
    constants: &ConstantRegisterState,
    target: u32,
    slot: usize,
    plan: &mut GifDmaKickPlan,
) -> bool {
    if inst.opcode != OPCODE_SW || !hint.has_address || hint.address != target {
        return false;
    }

    plan.store_indices[slot] = instruction_index;
    match constants.read(inst.rt) {
        Some(constant) => plan.values[slot] = format!("0x{constant:x}u"),
        None => {
            plan.values[slot] = gif_dma_kick_temp_name(inst.address, slot);
            plan.capture_expressions[slot] = format!("GPR_U32(ctx, {})", inst.rt);
            plan.captures[slot] = true;
        }
    }
    true
}

/// `tryBuildGifDmaKickPlan`.
pub fn try_build_gif_dma_kick_plan(
    instructions: &[Instruction],
    start_index: usize,
    constants: &ConstantRegisterState,
    internal_targets: &BTreeSet<u32>,
) -> GifDmaKickPlan {
    const TARGETS: [u32; 4] = [
        0x1000_E020, // D_PCR
        0x1000_E010, // D_STAT
        0x1000_A030, // GIF TADR
        0x1000_A000, // GIF CHCR
    ];
    const MAX_SCAN_INSTRUCTIONS: usize = 32;

    let mut plan = GifDmaKickPlan::default();
    if start_index >= instructions.len() {
        return plan;
    }

    let mut scan_constants = constants.clone();
    let mut matched = 0usize;
    let scan_end = instructions.len().min(start_index + MAX_SCAN_INSTRUCTIONS);

    for j in start_index..scan_end {
        let inst = &instructions[j];
        if j != start_index && matched > 0 && internal_targets.contains(&inst.address) {
            return GifDmaKickPlan::default();
        }

        let hint = resolve_memory_access_hint(inst, &scan_constants);
        if is_direct_memory_access(inst) {
            if matched >= TARGETS.len()
                || !try_match_gif_dma_store(j, inst, hint, &scan_constants, TARGETS[matched], matched, &mut plan)
            {
                return GifDmaKickPlan::default();
            }

            matched += 1;
            update_constant_registers(inst, &mut scan_constants);
            if matched == TARGETS.len() {
                plan.valid = true;
                plan.end_index = j;
                return plan;
            }
            continue;
        }

        if is_return_with_delay_slot(inst) {
            let delay_index = j + 1;
            if matched != TARGETS.len() - 1
                || delay_index >= instructions.len()
                || instructions[delay_index].address != inst.address.wrapping_add(4)
                || internal_targets.contains(&instructions[delay_index].address)
            {
                return GifDmaKickPlan::default();
            }

            let delay_inst = &instructions[delay_index];
            let delay_hint = resolve_memory_access_hint(delay_inst, &scan_constants);
            if !try_match_gif_dma_store(
                delay_index,
                delay_inst,
                delay_hint,
                &scan_constants,
                TARGETS[matched],
                matched,
                &mut plan,
            ) {
                return GifDmaKickPlan::default();
            }

            plan.valid = true;
            plan.completes_in_delay_slot = true;
            plan.branch_index = j;
            plan.end_index = delay_index;
            return plan;
        }

        if inst.has_delay_slot || inst.is_branch || inst.is_jump || inst.modification.modifies_control {
            return GifDmaKickPlan::default();
        }

        update_constant_registers(inst, &mut scan_constants);
    }

    GifDmaKickPlan::default()
}

/// `gifDmaKickCall`.
pub fn gif_dma_kick_call(plan: &GifDmaKickPlan) -> String {
    format!(
        "runtime->kickGifDmaChainFromMMIO(rdram, ctx, {}, {}, {}, {});",
        plan.values[0], plan.values[1], plan.values[2], plan.values[3]
    )
}

/// `emitGifDmaCapture`: the `uint32_t` temporary that holds a stored register.
pub fn emit_gif_dma_capture(out: &mut String, plan: &GifDmaKickPlan, slot: usize, indent: &str) {
    if slot >= plan.captures.len() || !plan.captures[slot] {
        return;
    }
    let _ = writeln!(out, "{indent}uint32_t {} = {};", plan.values[slot], plan.capture_expressions[slot]);
}

/// `gifDmaDelaySlotOverride`.
pub fn gif_dma_delay_slot_override(delay_slot: &Instruction, plan: &GifDmaKickPlan, emit_comments: bool) -> String {
    let mut code = String::new();
    let slot = plan.slot_for(plan.end_index);
    if emit_comments {
        let _ = write!(code, "// 0x{:x}: 0x{:x}", delay_slot.address, delay_slot.raw);
        let disassembly = delay_slot.disassembly();
        if !disassembly.is_empty() {
            let _ = write!(code, "  {disassembly}");
        }
        code.push_str(" (Delay Slot)\n");
    }

    if slot < plan.captures.len() && plan.captures[slot] {
        let _ = writeln!(code, "uint32_t {} = {};", plan.values[slot], plan.capture_expressions[slot]);
    }
    code.push_str(&gif_dma_kick_call(plan));
    code
}
