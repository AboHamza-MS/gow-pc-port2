//! Semantic decode of one R5900 instruction.
//!
//! Port of `R5900Decoder::decodeInstruction` and the family functions it calls
//! (`decodeSpecial`, `decodeMMI`, `decodeMMI0..3`, `decodePMFHL`, `decodeCOP2`)
//! from `PS2Recomp/ps2xRecomp/src/lib/r5900_decoder.cpp`.
//!
//! The C++ class also defines `decodeRType`, `decodeIType`, `decodeJType`,
//! `decodeRegimm`, `decodeCOP0` and `decodeCOP1`; no call site reaches them, so
//! they are not ported. The flags they would set already come from the
//! Rabbitizer descriptor (a `bgezal` is a function call in Rabbitizer too).
//!
//! Two pieces of state are intentionally left at zero because the C++ never
//! writes them: `Instruction::vu_function` and the MMI1 `subFunction` local.

use rabbitizer::{InstrCategory, InstrId, Instruction as RabbitizerInstruction};

use crate::opcodes::*;
use crate::Instruction;

/// Decode `raw` located at `address`, exactly like `R5900Decoder::decodeInstruction`.
pub fn decode(address: u32, raw: u32, include_disassembly: bool) -> Instruction {
    let mut inst = Instruction::new(address, raw, false);
    let rabbit = RabbitizerInstruction::new(raw, address, InstrCategory::R5900);

    inst.is_branch = rabbit.is_branch();
    inst.is_jump = rabbit.is_jump();
    inst.is_call = rabbit.is_function_call();
    inst.is_return = rabbit.is_return() || rabbit.unique_id == InstrId::cpu_eret;
    inst.has_delay_slot = rabbit.has_delay_slot();
    inst.is_load = rabbit.does_load();
    inst.is_store = rabbit.does_store();

    inst.is_mmi = inst.opcode == OPCODE_MMI;
    inst.is_vu = matches!(inst.opcode, OPCODE_COP2 | OPCODE_LWC2 | OPCODE_LDC2 | OPCODE_SWC2 | OPCODE_SDC2);

    let mut modifies_gpr = rabbit.does_link();
    modifies_gpr |= rabbit.modifies_rs() && inst.rs != 0;
    modifies_gpr |= rabbit.modifies_rt() && inst.rt != 0;
    modifies_gpr |= rabbit.modifies_rd() && inst.rd != 0;
    inst.modification.modifies_gpr = modifies_gpr;

    inst.modification.modifies_fpr = rabbit.modifies_fs() || rabbit.modifies_ft() || rabbit.modifies_fd();
    inst.modification.modifies_memory = rabbit.does_store();
    inst.modification.modifies_control =
        rabbit.is_branch() || rabbit.is_jump() || rabbit.is_trap() || rabbit.modifies_hi() || rabbit.modifies_lo();

    if rabbit.unique_id == InstrId::cpu_eret {
        inst.is_return = true;
        inst.has_delay_slot = false;
        inst.modification.modifies_control = true;
    }

    // Load-linked and store-conditional touch control state (the reservation).
    if inst.opcode == OPCODE_LL || inst.opcode == OPCODE_LLD {
        inst.is_load = true;
        inst.modification.modifies_control = true;
        if inst.rt != 0 {
            inst.modification.modifies_gpr = true;
        }
    }
    if inst.opcode == OPCODE_SC || inst.opcode == OPCODE_SCD {
        inst.is_store = true;
        inst.modification.modifies_control = true;
        // The success flag is written to rt.
        if inst.rt != 0 {
            inst.modification.modifies_gpr = true;
        }
    }

    if inst.opcode == OPCODE_COP2 {
        decode_cop2(&mut inst);
    } else if inst.opcode == OPCODE_LWC2 || inst.opcode == OPCODE_LDC2 {
        inst.is_vu = true;
        inst.modification.modifies_vfr = true;
    } else if inst.opcode == OPCODE_SWC2 || inst.opcode == OPCODE_SDC2 {
        inst.is_vu = true;
        inst.modification.modifies_memory = true;
    }

    if inst.opcode == OPCODE_LQ || inst.opcode == OPCODE_SQ {
        inst.is_multimedia = true;
    }

    if inst.opcode == OPCODE_SPECIAL {
        decode_special(&mut inst);
    } else if inst.opcode == OPCODE_MMI {
        decode_mmi(&mut inst);
    }

    if inst.is_mmi || inst.is_vu {
        inst.is_multimedia = true;
        // Only VU operations are truly vector.
        inst.vector.is_vector = inst.is_vu;
    }

    if include_disassembly {
        inst.disassembly = rabbit.disassemble(None, 0);
    }
    inst
}

/// `decodeSpecial`.
fn decode_special(inst: &mut Instruction) {
    if inst.rd != 0 {
        inst.modification.modifies_gpr = true;
    }

    if inst.function == SPECIAL_JR {
        inst.is_jump = true;
        inst.has_delay_slot = true;
        // Does not modify a GPR by itself.
        inst.modification.modifies_gpr = false;
        inst.modification.modifies_control = true;
        if inst.rs == 31 {
            // `jr $ra` is a return.
            inst.is_return = true;
        }
    } else if inst.function == SPECIAL_JALR {
        inst.is_jump = true;
        inst.is_call = true;
        inst.has_delay_slot = true;
        // `jalr $zero, $rs` behaves like `jr $rs`.
        inst.modification.modifies_gpr = inst.rd != 0;
        inst.modification.modifies_control = true;
    } else if matches!(inst.function, SPECIAL_SYSCALL | SPECIAL_BREAK) {
        inst.modification.modifies_gpr = false;
        inst.modification.modifies_control = true;
    } else if matches!(inst.function, SPECIAL_MULT | SPECIAL_MULTU) {
        // The R5900 MULT/MULTU also write rd when rd != 0.
        inst.modification.modifies_gpr = inst.rd != 0;
        inst.modification.modifies_control = true;
    } else if matches!(
        inst.function,
        SPECIAL_MFHI
            | SPECIAL_MFLO
            | SPECIAL_ADD
            | SPECIAL_ADDU
            | SPECIAL_SUB
            | SPECIAL_SUBU
            | SPECIAL_AND
            | SPECIAL_OR
            | SPECIAL_XOR
            | SPECIAL_NOR
            | SPECIAL_SLL
            | SPECIAL_SRL
            | SPECIAL_SRA
            | SPECIAL_SLLV
            | SPECIAL_SRLV
            | SPECIAL_SRAV
            | SPECIAL_DADD
            | SPECIAL_DADDU
            | SPECIAL_DSUB
            | SPECIAL_DSUBU
            | SPECIAL_DSLL
            | SPECIAL_DSRL
            | SPECIAL_DSRA
            | SPECIAL_DSLL32
            | SPECIAL_DSRL32
            | SPECIAL_DSRA32
            | SPECIAL_DSLLV
            | SPECIAL_DSRLV
            | SPECIAL_DSRAV
            | SPECIAL_MOVZ
            | SPECIAL_MOVN
            | SPECIAL_SLT
            | SPECIAL_SLTU
            | SPECIAL_MFSA
    ) {
        // Writing `$zero` changes nothing, but `rd != 0` still counts.
        if inst.rd == 0 {
            inst.modification.modifies_gpr = false;
        }
    } else if matches!(
        inst.function,
        SPECIAL_MTHI
            | SPECIAL_MTLO
            | SPECIAL_DIV
            | SPECIAL_DIVU
            | SPECIAL_TGE
            | SPECIAL_TGEU
            | SPECIAL_TLT
            | SPECIAL_TLTU
            | SPECIAL_TEQ
            | SPECIAL_TNE
            | SPECIAL_MTSA
            | SPECIAL_SYNC
    ) {
        inst.modification.modifies_gpr = false;
        inst.modification.modifies_control = true;
    }
}

/// `decodeMMI`.
fn decode_mmi(inst: &mut Instruction) {
    inst.is_mmi = true;
    inst.is_multimedia = true;
    // Most MMI operations write rd, except when rd is `$zero`.
    inst.modification.modifies_gpr = inst.rd != 0;

    let mmi_function = inst.function;

    if mmi_function == MMI_MMI0 {
        decode_mmi0(inst);
        return;
    }
    if mmi_function == MMI_MMI1 {
        decode_mmi1(inst);
        return;
    }
    if mmi_function == MMI_MMI2 {
        decode_mmi2(inst);
        return;
    }
    if mmi_function == MMI_MMI3 {
        decode_mmi3(inst);
        return;
    }

    if matches!(mmi_function, MMI_MFHI1 | MMI_MFLO1) {
        if inst.rd == 0 {
            inst.modification.modifies_gpr = false;
        }
        inst.modification.modifies_control = false;
    } else if matches!(mmi_function, MMI_MTHI1 | MMI_MTLO1) {
        inst.modification.modifies_gpr = false;
        inst.modification.modifies_control = true;
    } else if matches!(
        mmi_function,
        MMI_MADD | MMI_MADDU | MMI_MSUB | MMI_MSUBU | MMI_MADD1 | MMI_MADDU1 | MMI_MULT1 | MMI_MULTU1
    ) {
        // The R5900 writes rd too (checked against the EE manual).
        inst.modification.modifies_gpr = inst.rd != 0;
        inst.modification.modifies_control = true;
    } else if matches!(mmi_function, MMI_DIV1 | MMI_DIVU1 | MMI_PMTHL | MMI3_PMTHI | MMI3_PMTLO) {
        // Writes HI/LO (or HI1/LO1).
        inst.modification.modifies_gpr = false;
        inst.modification.modifies_control = true;
    } else if mmi_function == MMI_PMFHL {
        decode_pmfhl(inst);
    }
    // MMI_PLZCW and the packed shifts keep the defaults.
}

/// `decodeMMI0`: the sub-opcode lives in `sa`.
fn decode_mmi0(inst: &mut Instruction) {
    inst.mmi_type = 0;
    inst.mmi_function = inst.sa as u8;
    // Every MMI0 operation keeps the defaults set by `decode_mmi`.
}

/// `decodeMMI1`: the sub-opcode lives in `sa`.
fn decode_mmi1(inst: &mut Instruction) {
    inst.mmi_type = 1;
    inst.mmi_function = inst.sa as u8;
    if inst.sa == MMI1_QFSRV {
        // Quadword funnel shift right variable.
        inst.is_multimedia = true;
    }
}

/// `decodeMMI2`: the sub-opcode lives in `sa`.
fn decode_mmi2(inst: &mut Instruction) {
    inst.mmi_type = 2;
    inst.mmi_function = inst.sa as u8;

    if matches!(
        inst.sa,
        MMI2_PMADDW
            | MMI2_PMSUBW
            | MMI2_PMULTW
            | MMI2_PDIVW
            | MMI2_PDIVBW
            | MMI2_PMADDH
            | MMI2_PHMADH
            | MMI2_PMSUBH
            | MMI2_PHMSBH
            | MMI2_PMULTH
    ) {
        // HI/LO.
        inst.modification.modifies_control = true;
    }

    if matches!(
        inst.sa,
        MMI2_PMADDW
            | MMI2_PMSUBW
            | MMI2_PMADDH
            | MMI2_PHMADH
            | MMI2_PMSUBH
            | MMI2_PHMSBH
            | MMI2_PMULTH
            | MMI2_PMULTW
            | MMI2_PDIVW
            | MMI2_PDIVBW
    ) {
        inst.is_multimedia = true;
    }
}

/// `decodeMMI3`: the sub-opcode lives in `sa`.
fn decode_mmi3(inst: &mut Instruction) {
    inst.mmi_type = 3;
    inst.mmi_function = inst.sa as u8;

    if inst.sa == MMI3_PMADDUW {
        inst.is_multimedia = true;
        inst.modification.modifies_control = true;
    } else if matches!(inst.sa, MMI3_PMTHI | MMI3_PMTLO) {
        inst.mmi_function = inst.sa as u8;
        inst.modification.modifies_gpr = false;
        // HI or LO is modified.
        inst.modification.modifies_control = true;
    } else if matches!(inst.sa, MMI3_PMULTUW | MMI3_PDIVUW) {
        inst.is_multimedia = true;
        inst.modification.modifies_control = true;
    }
}

/// `decodePMFHL`: the variation is encoded in `sa`.
fn decode_pmfhl(inst: &mut Instruction) {
    inst.pmfhl_variation = match inst.sa {
        PMFHL_LW => PMFHL_LW as u8,
        PMFHL_UW => PMFHL_UW as u8,
        PMFHL_SLW => PMFHL_SLW as u8,
        PMFHL_LH => PMFHL_LH as u8,
        PMFHL_SH => PMFHL_SH as u8,
        _ => 0xFF,
    };
}

/// `decodeCOP2`: VU0 macro mode (format 0x10..0x1F) and the transfer/branch formats.
fn decode_cop2(inst: &mut Instruction) {
    let format = inst.rs;
    inst.is_vu = true;
    inst.is_multimedia = true;

    if matches!(format, COP2_QMFC2 | COP2_CFC2) {
        if inst.rt != 0 {
            inst.modification.modifies_gpr = true;
        }
    } else if format == COP2_QMTC2 {
        // Modifies VU state.
        inst.modification.modifies_vfr = true;
    } else if format == COP2_CTC2 {
        // Modifies VU control state.
        inst.modification.modifies_control = true;
    } else if format == COP2_BC {
        inst.is_branch = true;
        inst.has_delay_slot = true;
        inst.modification.modifies_control = true;
    } else if (COP2_CO..=COP2_CO + 15).contains(&format) {
        decode_cop2_macro(inst);
    }
}

/// The VU0 macro block of `decodeCOP2`.
// The C++ constant keeps upstream's casing (`VCLIPw`) and it is used as a pattern.
#[allow(non_upper_case_globals)]
fn decode_cop2_macro(inst: &mut Instruction) {
    let special1_function = inst.function & 0x3F;
    let is_special2 = special1_function >= 0x3C;
    let special2_function = (((inst.raw >> 6) & 0x1F) << 2) | (inst.raw & 0x3);
    let vu_function = if is_special2 { special2_function } else { special1_function };

    inst.vector.vector_field = ((inst.raw >> 21) & 0xF) as u8;

    // The L/M selectors of the div/sqrt family live in bits 21-24.
    if matches!(vu_function, VU0_S2_VDIV | VU0_S2_VRSQRT | VU0_S2_VMTIR | VU0_S2_VRINIT | VU0_S2_VRXOR) {
        inst.vector.fsf = ((inst.raw >> 21) & 0x3) as u8;
    }
    if matches!(vu_function, VU0_S2_VDIV | VU0_S2_VSQRT | VU0_S2_VRSQRT) {
        inst.vector.ftf = ((inst.raw >> 23) & 0x3) as u8;
    }

    // Defaults: vector floats and the flags/special registers are touched.
    inst.modification.modifies_vfr = true;
    inst.modification.modifies_control = true;

    if is_special2 {
        match vu_function {
            VU0_S2_VDIV | VU0_S2_VSQRT | VU0_S2_VRSQRT => inst.vector.uses_q_reg = true,
            VU0_S2_VMTIR => {
                inst.modification.modifies_vfr = false;
                inst.modification.modifies_vic = true;
            }
            // Reads VI, writes VF.
            VU0_S2_VMFIR => inst.modification.modifies_vir = false,
            VU0_S2_VILWR => {
                inst.modification.modifies_vfr = false;
                inst.modification.modifies_vir = true;
                inst.is_load = true;
            }
            VU0_S2_VISWR => {
                inst.modification.modifies_vfr = false;
                inst.modification.modifies_vir = false;
                inst.is_store = true;
                inst.modification.modifies_memory = true;
            }
            // Modifies R.
            VU0_S2_VRINIT | VU0_S2_VRXOR => inst.modification.modifies_vfr = false,
            // Reads R, writes VF.
            VU0_S2_VRGET => inst.modification.modifies_control = false,
            VU0_S2_VRNEXT => {
                inst.modification.modifies_control = true;
                inst.modification.modifies_vfr = false;
            }
            // Only VF.
            VU0_S2_VABS | VU0_S2_VMOVE | VU0_S2_VMR32 => inst.modification.modifies_control = false,
            VU0_S2_VNOP => {
                inst.modification.modifies_vfr = false;
                inst.modification.modifies_control = false;
            }
            // Modifies the clip flags.
            VU0_S2_VCLIPw => inst.modification.modifies_vfr = false,
            _ => {}
        }
    } else if (VU0_S1_VIADD..=VU0_S1_VIOR).contains(&vu_function) || vu_function == VU0_S1_VIADDI {
        // Integer operations.
        inst.modification.modifies_vfr = false;
        inst.modification.modifies_vir = true;
    }
}

impl Instruction {
    /// `getBranchTarget`: PC + 4 + (immediate << 2), or 0 when not a branch.
    pub fn branch_target(&self) -> u32 {
        if !self.is_branch {
            return 0;
        }
        self.address.wrapping_add(4).wrapping_add(self.simmediate << 2)
    }

    /// `getJumpTarget`: the encoded target for `j`/`jal`, 0 otherwise.
    pub fn jump_target(&self) -> u32 {
        if !self.is_jump {
            return 0;
        }
        if self.opcode == OPCODE_SPECIAL && (self.function == SPECIAL_JR || self.function == SPECIAL_JALR) {
            // The target is in `rs`; not statically known.
            return 0;
        }
        if self.opcode == OPCODE_J || self.opcode == OPCODE_JAL {
            let pc_upper = self.address.wrapping_add(4) & 0xF000_0000;
            return pc_upper | (self.target << 2);
        }
        0
    }
}
