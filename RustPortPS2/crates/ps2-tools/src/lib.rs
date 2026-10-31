//! The installer's helper tools, in one binary (`ps2-tools`) so the installers need no Python.
//!
//! - [`elfcheck`]: `verify-elf`, identity and bounds of the local `SCUS_973.99`.
//! - [`iso`]: `iso-extract`, the ISO9660 extractor that also records the disc layout.
//! - [`vu1`]: `vu1-manifest` and `vu1-programs`, the VU1 micro-memories built from the ELF.
//! - [`iop_gen`]: `iop-gen`, the IRX modules compiled ahead of time to C++.
//! - [`vu1n_gen`]: `vu1n-gen`, the native VU1 block generator.
//! - [`hash`]: the FNV and BLAKE2b fingerprints those use.

pub mod elfcheck;
pub mod hash;
pub mod iop_gen;
pub mod iso;
pub mod vu1;
pub mod vu1n_gen;
