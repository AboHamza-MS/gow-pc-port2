//! `COP0` (system control) opcodes.
//!
//! Port of `PS2Recomp/ps2xRecomp/src/lib/cop0_translator.cpp`. The format is
//! `rs`: move-from/move-to a named COP0 register, `bc0`, or the `co` group
//! (TLB operations, `eret`, interrupt enable/disable). The register masks are
//! the C++ ones, kept verbatim in the generated text.

use ps2_r5900::opcodes::*;
use ps2_r5900::Instruction;

use crate::translate::instruction::FamilyTranslator;

/// Translate one `COP0` instruction.
pub fn translate(hooks: &mut impl FamilyTranslator, inst: &Instruction) -> String {
    let format = inst.rs;
    let rt = inst.rt;
    let rd = inst.rd;

    match format {
        COP0_MF => match rd {
            COP0_REG_INDEX => format!("SET_GPR_S32(ctx, {rt}, (int32_t)ctx->cop0_index);"),
            COP0_REG_RANDOM => format!("SET_GPR_S32(ctx, {rt}, (int32_t)ctx->cop0_random);"),
            COP0_REG_ENTRYLO0 => format!("SET_GPR_S32(ctx, {rt}, (int32_t)ctx->cop0_entrylo0);"),
            COP0_REG_ENTRYLO1 => format!("SET_GPR_S32(ctx, {rt}, (int32_t)ctx->cop0_entrylo1);"),
            COP0_REG_CONTEXT => format!("SET_GPR_S32(ctx, {rt}, (int32_t)ctx->cop0_context);"),
            COP0_REG_PAGEMASK => format!("SET_GPR_S32(ctx, {rt}, (int32_t)ctx->cop0_pagemask);"),
            COP0_REG_WIRED => format!("SET_GPR_S32(ctx, {rt}, (int32_t)ctx->cop0_wired);"),
            COP0_REG_BADVADDR => format!("SET_GPR_S32(ctx, {rt}, (int32_t)ctx->cop0_badvaddr);"),
            COP0_REG_COUNT => format!("SET_GPR_S32(ctx, {rt}, (int32_t)ctx->cop0_count);"),
            COP0_REG_ENTRYHI => format!("SET_GPR_S32(ctx, {rt}, (int32_t)ctx->cop0_entryhi);"),
            COP0_REG_COMPARE => format!("SET_GPR_S32(ctx, {rt}, (int32_t)ctx->cop0_compare);"),
            COP0_REG_STATUS => format!("SET_GPR_S32(ctx, {rt}, (int32_t)ctx->cop0_status);"),
            COP0_REG_CAUSE => format!("SET_GPR_S32(ctx, {rt}, (int32_t)ctx->cop0_cause);"),
            COP0_REG_EPC => format!("SET_GPR_S32(ctx, {rt}, (int32_t)ctx->cop0_epc);"),
            COP0_REG_PRID => format!("SET_GPR_S32(ctx, {rt}, (int32_t)ctx->cop0_prid);"),
            COP0_REG_CONFIG => format!("SET_GPR_S32(ctx, {rt}, (int32_t)ctx->cop0_config);"),
            COP0_REG_BADPADDR => format!("SET_GPR_S32(ctx, {rt}, (int32_t)ctx->cop0_badpaddr);"),
            COP0_REG_DEBUG => format!("SET_GPR_S32(ctx, {rt}, (int32_t)ctx->cop0_debug);"),
            COP0_REG_PERF => format!("SET_GPR_S32(ctx, {rt}, (int32_t)ctx->cop0_perf);"),
            COP0_REG_TAGLO => format!("SET_GPR_S32(ctx, {rt}, (int32_t)ctx->cop0_taglo);"),
            COP0_REG_TAGHI => format!("SET_GPR_S32(ctx, {rt}, (int32_t)ctx->cop0_taghi);"),
            COP0_REG_ERROREPC => format!("SET_GPR_S32(ctx, {rt}, (int32_t)ctx->cop0_errorepc);"),
            _ => format!("SET_GPR_S32(ctx, {rt}, 0);  // Unimplemented COP0 register {rd}"),
        },
        COP0_MT => match rd {
            COP0_REG_INDEX => format!("ctx->cop0_index = GPR_U32(ctx, {rt}) & 0x3F;"),
            COP0_REG_RANDOM => "// MTC0 to RANDOM register ignored (read-only)".to_owned(),
            COP0_REG_ENTRYLO0 => format!("ctx->cop0_entrylo0 = GPR_U32(ctx, {rt}) & 0x3FFFFFFF;"),
            COP0_REG_ENTRYLO1 => format!("ctx->cop0_entrylo1 = GPR_U32(ctx, {rt}) & 0x3FFFFFFF;"),
            COP0_REG_CONTEXT => {
                format!("ctx->cop0_context = (ctx->cop0_context & 0xFF800000) | (GPR_U32(ctx, {rt}) & 0x7FFFFF);")
            }
            COP0_REG_PAGEMASK => format!("ctx->cop0_pagemask = GPR_U32(ctx, {rt}) & 0x01FFE000;"),
            COP0_REG_WIRED => format!("ctx->cop0_wired = GPR_U32(ctx, {rt}) & 0x3F; ctx->cop0_random = 47;"),
            COP0_REG_BADVADDR => "// MTC0 to BADVADDR register ignored (read-only)".to_owned(),
            COP0_REG_COUNT => format!("ctx->cop0_count = GPR_U32(ctx, {rt});"),
            COP0_REG_ENTRYHI => format!("ctx->cop0_entryhi = GPR_U32(ctx, {rt}) & 0xC00000FF;"),
            COP0_REG_COMPARE => format!("ctx->cop0_compare = GPR_U32(ctx, {rt}); ctx->cop0_cause &= ~0x8000;"),
            COP0_REG_STATUS => format!("ctx->cop0_status = GPR_U32(ctx, {rt}) & 0xFF57FFFF;"),
            COP0_REG_CAUSE => {
                format!("ctx->cop0_cause = (ctx->cop0_cause & ~0x00000300) | (GPR_U32(ctx, {rt}) & 0x00000300);")
            }
            COP0_REG_EPC => format!("ctx->cop0_epc = GPR_U32(ctx, {rt});"),
            COP0_REG_PRID => "// MTC0 to PRID register ignored (read-only)".to_owned(),
            COP0_REG_CONFIG => {
                format!("ctx->cop0_config = (ctx->cop0_config & ~0x7) | (GPR_U32(ctx, {rt}) & 0x7);")
            }
            COP0_REG_BADPADDR => "// MTC0 to BADPADDR register ignored (read-only)".to_owned(),
            COP0_REG_DEBUG => format!("ctx->cop0_debug = GPR_U32(ctx, {rt});"),
            COP0_REG_PERF => format!("ctx->cop0_perf = GPR_U32(ctx, {rt});"),
            COP0_REG_TAGLO => format!("ctx->cop0_taglo = GPR_U32(ctx, {rt});"),
            COP0_REG_TAGHI => format!("ctx->cop0_taghi = GPR_U32(ctx, {rt});"),
            COP0_REG_ERROREPC => format!("ctx->cop0_errorepc = GPR_U32(ctx, {rt});"),
            _ => format!("// Unimplemented MTC0 to COP0 {rd}"),
        },
        COP0_BC => format!("// BC0 (Condition: 0x{rt:X}) - Handled by branch logic"),
        COP0_CO => {
            let function = inst.function;
            match function {
                COP0_CO_TLBR => "runtime->handleTLBR(rdram, ctx);".to_owned(),
                COP0_CO_TLBWI => "runtime->handleTLBWI(rdram, ctx);".to_owned(),
                COP0_CO_TLBWR => "runtime->handleTLBWR(rdram, ctx);".to_owned(),
                COP0_CO_TLBP => "runtime->handleTLBP(rdram, ctx);".to_owned(),
                COP0_CO_ERET => concat!(
                    "if (ctx->cop0_status & 0x4) { \n",
                    "    ctx->pc = ctx->cop0_errorepc; \n",
                    "    ctx->cop0_status &= ~0x4; \n",
                    "} else { \n",
                    "    ctx->pc = ctx->cop0_epc; \n",
                    "    ctx->cop0_status &= ~0x2; \n",
                    "} \n",
                    "runtime->clearLLBit(ctx); \n",
                    "return;"
                )
                .to_owned(),
                COP0_CO_EI => "ctx->cop0_status |= 0x10000; // Enable interrupts".to_owned(),
                COP0_CO_DI => "ctx->cop0_status &= ~0x10000; // Disable interrupts".to_owned(),
                _ => hooks.emit_unhandled(inst, &format!("Unhandled COP0 CO-OP: 0x{function:X}")),
            }
        }
        _ => hooks.emit_unhandled(inst, &format!("Unhandled COP0 instruction format: 0x{format:X}")),
    }
}
