//! R5900 (Emotion Engine) instruction fields, flags and disassembly.
//!
//! Port of the decoding half of `PS2Recomp/ps2xRecomp/src/lib/r5900_decoder.cpp`.
//! The disassembly text comes from the same Rabbitizer release the C++ build
//! links (the vendored copy), because it is copied verbatim into the
//! generated C++ comments and must match byte for byte.
//!
//! Status: instruction fields, defaults and the Rabbitizer-backed disassembly
//! are here; the semantic decode (branch/call/MMI/COP2 flags and the
//! `vectorInfo`/`modificationInfo` bookkeeping) lands with the translators.

pub mod decode;
pub mod opcodes;

use rabbitizer::{InstrCategory, Instruction as RabbitizerInstruction};

pub use decode::decode;
pub use opcodes::*;

/// Vector-side bookkeeping of one instruction, mirroring `Instruction::vectorInfo`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VectorInfo {
    pub is_vector: bool,
    pub uses_q_reg: bool,
    pub uses_p_reg: bool,
    pub modifies_mac: bool,
    /// `xyzw` mask; the C++ side starts at 0xF (all fields).
    pub vector_field: u8,
    /// Field selector of the source (bits 10-11), set by the VU0 macro decode.
    pub fsf: u8,
    /// Field selector of the target (bits 8-9), set by the VU0 macro decode.
    pub ftf: u8,
}

impl Default for VectorInfo {
    fn default() -> Self {
        Self {
            is_vector: false,
            uses_q_reg: false,
            uses_p_reg: false,
            modifies_mac: false,
            vector_field: 0xF,
            fsf: 0,
            ftf: 0,
        }
    }
}

/// Side-effect bookkeeping of one instruction, mirroring `Instruction::modificationInfo`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ModificationInfo {
    pub modifies_gpr: bool,
    pub modifies_fpr: bool,
    pub modifies_vfr: bool,
    pub modifies_vir: bool,
    pub modifies_vic: bool,
    pub modifies_memory: bool,
    pub modifies_control: bool,
}

/// The raw bit fields every R5900 instruction carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Fields {
    pub opcode: u32,
    pub rs: u32,
    pub rt: u32,
    pub rd: u32,
    pub sa: u32,
    pub function: u32,
    pub immediate: u32,
    /// Sign-extended immediate, kept in the 32-bit pattern like the C++ struct.
    pub simmediate: u32,
    pub target: u32,
}

impl Fields {
    /// Extract the fields exactly like the `OPCODE`, `RS`, `RT`... macros.
    pub fn new(raw: u32) -> Self {
        Self {
            opcode: (raw >> 26) & 0x3F,
            rs: (raw >> 21) & 0x1F,
            rt: (raw >> 16) & 0x1F,
            rd: (raw >> 11) & 0x1F,
            sa: (raw >> 6) & 0x1F,
            function: raw & 0x3F,
            immediate: raw & 0xFFFF,
            // `(int32_t)(int16_t)(raw & 0xFFFF)` kept in the 32-bit pattern.
            simmediate: (raw as u16 as i16 as i32) as u32,
            target: raw & 0x03FF_FFFF,
        }
    }

    /// The sign-extended immediate as a signed number.
    pub fn simmediate_i32(&self) -> i32 {
        self.simmediate as i32
    }
}

/// One decoded instruction, mirroring `ps2recomp::Instruction`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Instruction {
    pub address: u32,
    pub raw: u32,
    pub opcode: u32,
    pub rs: u32,
    pub rt: u32,
    pub rd: u32,
    pub sa: u32,
    pub function: u32,
    pub immediate: u32,
    pub simmediate: u32,
    pub target: u32,

    pub is_mmi: bool,
    pub is_vu: bool,
    pub is_branch: bool,
    pub is_jump: bool,
    pub is_call: bool,
    pub is_return: bool,
    pub has_delay_slot: bool,
    pub is_multimedia: bool,
    pub is_store: bool,
    pub is_load: bool,

    pub mmi_type: u8,
    pub mmi_function: u8,
    pub pmfhl_variation: u8,
    pub vu_function: u8,

    pub is_mmio: bool,
    pub mmio_address: u32,

    pub vector: VectorInfo,
    pub modification: ModificationInfo,

    /// Filled only when the caller asks for it; empty means "not disassembled".
    pub disassembly: String,
}

impl Instruction {
    /// Fields plus the C++ default state, without the semantic decode.
    pub fn new(address: u32, raw: u32, include_disassembly: bool) -> Self {
        let fields = Fields::new(raw);
        Self {
            address,
            raw,
            opcode: fields.opcode,
            rs: fields.rs,
            rt: fields.rt,
            rd: fields.rd,
            sa: fields.sa,
            function: fields.function,
            immediate: fields.immediate,
            simmediate: fields.simmediate,
            target: fields.target,
            disassembly: if include_disassembly { disassemble(address, raw) } else { String::new() },
            ..Self::default()
        }
    }

    /// The cached text, or a fresh disassembly of `address`/`raw`.
    pub fn disassembly(&self) -> String {
        if !self.disassembly.is_empty() {
            return self.disassembly.clone();
        }
        disassemble(self.address, self.raw)
    }
}

/// `R5900Decoder::disassembleInstruction`: Rabbitizer's text for one word.
pub fn disassemble(address: u32, raw: u32) -> String {
    RabbitizerInstruction::new(raw, address, InstrCategory::R5900).disassemble(None, 0)
}
