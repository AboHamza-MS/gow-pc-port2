//! Control flow helpers shared with the emitters.
//!
//! Port of `PS2Recomp/ps2xRecomp/include/ps2recomp/control_flow_utils.h`
//! (`isGuestNop`; `buildAbsoluteJumpTarget` lives next to the translator).

use ps2_r5900::opcodes::*;
use ps2_r5900::Instruction;

/// `isGuestNop`: a `sll $zero, $zero, 0`, or the `addiu $zero, $zero, 0` some
/// decoded streams use for the same thing.
pub fn is_guest_nop(inst: &Instruction) -> bool {
    let sll_zero =
        inst.opcode == OPCODE_SPECIAL && inst.function == SPECIAL_SLL && inst.rd == 0 && inst.rt == 0 && inst.sa == 0;

    let addiu_zero = inst.opcode == OPCODE_ADDIU && inst.rs == 0 && inst.rt == 0 && (inst.immediate as u16 as i16) == 0;

    sll_zero || addiu_zero
}
