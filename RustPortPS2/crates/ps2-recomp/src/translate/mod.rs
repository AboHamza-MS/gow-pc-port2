//! Instruction translation to C++.
//!
//! Port of `PS2Recomp/ps2xRecomp/src/lib/instruction_translator.cpp` and the
//! family translators it delegates to.

pub mod control_flow;
pub mod cop0;
pub mod fpu;
pub mod instruction;
pub mod memory;
pub mod mmi;
pub mod mmi_helpers;
pub mod mmi_tables;
pub mod regimm;
pub mod special;
pub mod vu;
pub mod vu_helpers;
