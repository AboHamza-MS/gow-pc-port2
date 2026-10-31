//! `REGIMM` opcodes.
//!
//! Port of `PS2Recomp/ps2xRecomp/src/lib/regimm_translator.cpp`. The branches
//! (`bltzal` and friends) only get a comment here: the control flow pass emits
//! the branch itself, and the `target` in the comment uses the C++ 64-bit
//! intermediates so a negative offset cannot wrap.

use ps2_r5900::opcodes::*;
use ps2_r5900::Instruction;

use crate::translate::instruction::FamilyTranslator;

/// Translate one `REGIMM` instruction.
pub fn translate(hooks: &mut impl FamilyTranslator, inst: &Instruction) -> String {
    match inst.rt {
        REGIMM_BLTZ | REGIMM_BGEZ | REGIMM_BLTZL | REGIMM_BGEZL | REGIMM_BLTZAL | REGIMM_BGEZAL | REGIMM_BLTZALL
        | REGIMM_BGEZALL => {
            let offset_bytes = (inst.simmediate as i32).wrapping_shl(2);
            let target = (i64::from(inst.address) + 4 + i64::from(offset_bytes)) as u32;
            format!("// REGIMM branch instruction to 0x{target:X} - Handled by branch logic")
        }
        REGIMM_MTSAB => format!("ctx->sa = ((GPR_U32(ctx, {}) ^ (uint32_t){}) & 0xF) << 3;", inst.rs, inst.simmediate),
        REGIMM_MTSAH => format!("ctx->sa = ((GPR_U32(ctx, {}) ^ (uint32_t){}) & 0x7) << 4;", inst.rs, inst.simmediate),
        REGIMM_TGEI => format!(
            "if (GPR_S64(ctx, {}) >= (int64_t)(int32_t){}) {{ runtime->handleTrap(rdram, ctx); }}",
            inst.rs, inst.simmediate
        ),
        REGIMM_TGEIU => format!(
            "if (GPR_U64(ctx, {}) >= (uint64_t)(int64_t)(int32_t){}) {{ runtime->handleTrap(rdram, ctx); }}",
            inst.rs, inst.simmediate
        ),
        REGIMM_TLTI => format!(
            "if (GPR_S64(ctx, {}) < (int64_t)(int32_t){}) {{ runtime->handleTrap(rdram, ctx); }}",
            inst.rs, inst.simmediate
        ),
        REGIMM_TLTIU => format!(
            "if (GPR_U64(ctx, {}) < (uint64_t)(int64_t)(int32_t){}) {{ runtime->handleTrap(rdram, ctx); }}",
            inst.rs, inst.simmediate
        ),
        REGIMM_TEQI => format!(
            "if (GPR_S64(ctx, {}) == (int64_t)(int32_t){}) {{ runtime->handleTrap(rdram, ctx); }}",
            inst.rs, inst.simmediate
        ),
        REGIMM_TNEI => format!(
            "if (GPR_S64(ctx, {}) != (int64_t)(int32_t){}) {{ runtime->handleTrap(rdram, ctx); }}",
            inst.rs, inst.simmediate
        ),
        _ => hooks.emit_unhandled(inst, &format!("Unhandled REGIMM instruction: 0x{:X}", inst.rt)),
    }
}
