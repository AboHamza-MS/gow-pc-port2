//! Emission of one recompiled function.
//!
//! Port of `FunctionEmitter::emit` (`function_emitter.cpp`): the file header, the
//! resume switch, one labelled statement per instruction (with its disassembly
//! comment), the branch/delay-slot groups and the GIF DMA kick replacement.

use std::fmt::Write as _;

use ps2_r5900::opcodes::*;
use ps2_r5900::Instruction;

use crate::flow::emitter::ControlFlowEmitter;
use crate::generator::{CodeGenerator, GenerationError};
use crate::gif_dma::{
    emit_gif_dma_capture, gif_dma_delay_slot_override, gif_dma_kick_call, resolve_memory_access_hint,
    try_build_gif_dma_kick_plan, update_constant_registers, ConstantRegisterState, GifDmaKickPlan,
};
use crate::types::Function;

fn make_synthetic_delay_slot(address: u32) -> Instruction {
    Instruction { address, raw: 0, opcode: OPCODE_SPECIAL, function: SPECIAL_SLL, ..Instruction::default() }
}

/// `CodeGenerator::generateFunction`.
pub fn emit_function(
    cg: &mut CodeGenerator,
    function: &Function,
    instructions: &[Instruction],
    use_headers: bool,
) -> Result<String, GenerationError> {
    let mut ss = String::new();
    cg.set_current_function_name(&function.name);

    if use_headers {
        ss.push_str("#include <stdexcept>\n");
        ss.push_str("#include \"ps2_runtime_macros.h\"\n");
        ss.push_str("#include \"ps2_runtime.h\"\n");
        ss.push_str("#include <ps2_recompiled_functions.h>\n");
        ss.push_str("#include <ps2_recompiled_stubs.h>\n\n");
        ss.push_str("#include \"ps2_syscalls.h\"\n");
        ss.push_str("#include \"ps2_stubs.h\"\n\n");
        ss.push_str("#ifdef PS2_FUNCTION_LOG_TRACKER\n");
        ss.push_str("#include \"ps2_log.h\"\n");
        ss.push_str("#endif\n\n");
    }

    let mut analysis = cg.collect_internal_branch_targets(function, instructions, None);
    let mut resume_targets: Vec<u32> = analysis.resume_entry_points.iter().copied().collect();
    if let Some(extra) = cg.resume_entry_targets_by_owner().get(&function.start) {
        resume_targets.extend_from_slice(extra);
    }
    resume_targets.sort_unstable();
    resume_targets.dedup();
    for &target in &resume_targets {
        analysis.entry_points.insert(target);
    }

    let mut constant_registers = ConstantRegisterState::new();
    let mut gif_dma_kick_plan = GifDmaKickPlan::default();
    let _ = writeln!(ss, "// Function: {}", function.name);
    let _ = writeln!(ss, "// Address: 0x{:x} - 0x{:x}", function.start, function.end);

    let mut sanitized_name = cg.get_function_name(function.start);
    if sanitized_name.is_empty() {
        sanitized_name = format!("Errorfunc_{:x}", function.start);
    }

    let _ = writeln!(ss, "void {sanitized_name}(uint8_t* rdram, R5900Context* ctx, PS2Runtime *runtime) {{");
    ss.push_str("#ifdef PS2_FUNCTION_LOG_TRACKER\n");
    let _ = writeln!(ss, "    PS_LOG_ENTRY(\"{sanitized_name}\");");
    ss.push_str("#endif\n");
    ss.push('\n');
    if !resume_targets.is_empty() {
        ss.push_str("    switch (ctx->pc) {\n");
        for target in &resume_targets {
            let _ = writeln!(ss, "        case 0x{target:x}u: goto label_{target:x};");
        }
        ss.push_str("        default: break;\n");
        ss.push_str("    }\n\n");
    }
    let _ = writeln!(ss, "    ctx->pc = 0x{:x}u;", function.start);
    ss.push('\n');

    let mut last_instruction_was_control_flow = false;
    let emit_comments = cg.emit_instruction_comments();

    let mut i = 0usize;
    while i < instructions.len() {
        let inst = &instructions[i];
        last_instruction_was_control_flow = inst.has_delay_slot;

        if analysis.entry_points.contains(&inst.address) {
            constant_registers.clear();
            let _ = writeln!(ss, "label_{:x}:", inst.address);
        }

        if emit_comments {
            let _ = write!(ss, "    // 0x{:x}: 0x{:x}", inst.address, inst.raw);
            let disassembly = inst.disassembly();
            if !disassembly.is_empty() {
                let _ = write!(ss, "  {disassembly}");
            }
            ss.push('\n');
        }

        if inst.has_delay_slot {
            let has_decoded_delay_slot =
                i + 1 < instructions.len() && instructions[i + 1].address == inst.address.wrapping_add(4);

            let synthetic_delay_slot;
            let delay_slot: &Instruction = if has_decoded_delay_slot {
                &instructions[i + 1]
            } else {
                synthetic_delay_slot = make_synthetic_delay_slot(inst.address.wrapping_add(4));
                &synthetic_delay_slot
            };

            if has_decoded_delay_slot && analysis.entry_points.contains(&delay_slot.address) {
                let _ = writeln!(ss, "label_{:x}:", delay_slot.address);
            }

            if gif_dma_kick_plan.valid
                && gif_dma_kick_plan.completes_in_delay_slot
                && gif_dma_kick_plan.branch_index == i
                && has_decoded_delay_slot
            {
                let override_code = gif_dma_delay_slot_override(delay_slot, &gif_dma_kick_plan, emit_comments);
                ss.push_str(&ControlFlowEmitter::new(cg, inst, delay_slot, function, &analysis, override_code).emit());
                gif_dma_kick_plan = GifDmaKickPlan::default();
            } else {
                ss.push_str(&ControlFlowEmitter::new(cg, inst, delay_slot, function, &analysis, String::new()).emit());
            }

            if has_decoded_delay_slot {
                i += 1; // Skip delay slot instruction (handled inside branch logic)
            }
            constant_registers.clear();
        } else {
            if !gif_dma_kick_plan.valid {
                gif_dma_kick_plan =
                    try_build_gif_dma_kick_plan(instructions, i, &constant_registers, &analysis.entry_points);
            }

            if gif_dma_kick_plan.suppresses(i) {
                let slot = gif_dma_kick_plan.slot_for(i);
                emit_gif_dma_capture(&mut ss, &gif_dma_kick_plan, slot, "    ");

                if gif_dma_kick_plan.completes_at(i) {
                    let _ = writeln!(ss, "    ctx->pc = 0x{:x}u;", inst.address);
                    let _ = writeln!(ss, "    {}", gif_dma_kick_call(&gif_dma_kick_plan));
                    gif_dma_kick_plan = GifDmaKickPlan::default();
                }

                update_constant_registers(inst, &mut constant_registers);
                i += 1;
                continue;
            }

            let _ = writeln!(ss, "    ctx->pc = 0x{:x}u;", inst.address);
            let memory_hint = resolve_memory_access_hint(inst, &constant_registers);
            let _ = write!(ss, "    {}", cg.translate_instruction_with_hint(inst, memory_hint));
            if inst.is_mmio {
                let _ = write!(ss, " // MMIO: 0x{:x}", inst.mmio_address);
            }
            ss.push('\n');

            update_constant_registers(inst, &mut constant_registers);
        }

        i += 1;
    }

    // Fallthrough with no terminating branch: publish the next PC so the EE dispatcher does not re-enter this function.
    if !instructions.is_empty() && !last_instruction_was_control_flow {
        let _ = writeln!(ss, "    ctx->pc = 0x{:x}u;", function.end);
    }

    ss.push_str("}\n");
    Ok(ss)
}
