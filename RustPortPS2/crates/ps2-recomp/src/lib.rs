//! The recompiler's code generation.
//!
//! The translator family (`PS2Recomp/ps2xRecomp/src/lib/`), the control-flow analysis and the emitters. The generated
//! text must match the C++ recompiler byte for byte, so every formatting decision here is part of the contract.

pub mod calls;
mod calls_generated;
pub mod codegen_helpers;
pub mod emit;
pub mod flow;
pub mod generator;
pub mod gif_dma;
pub mod recompiler;
pub mod reporter;
pub mod scan;
pub mod translate;
pub mod types;
