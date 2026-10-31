//! The generic opcode translator.
//!
//! Port of `InstructionTranslator::translate` (and its `effectiveMemoryHintFor`
//! no-op) from `PS2Recomp/ps2xRecomp/src/lib/instruction_translator.cpp`. The
//! families (`SPECIAL`, `REGIMM`, `COP0`, `COP1`, `COP2`, `MMI`) are delegated
//! to a [`FamilyTranslator`], mirroring the C++ `CodeGenerator` calls, and the
//! text of every generic opcode is reproduced verbatim: it is the generated C++.
//!
//! The immediates are printed as the unsigned 32-bit pattern, exactly like the
//! C++ `uint32_t simmediate` field, so `-0x18` prints as `4294967272` and the
//! generated casts stay identical.

use ps2_r5900::opcodes::*;
use ps2_r5900::Instruction;

use crate::translate::memory::{translate_memory_read, translate_memory_write, MemoryAccessHint};

/// The `CodeGenerator` calls the C++ translator makes for a family.
pub trait FamilyTranslator {
    fn translate_special(&mut self, inst: &Instruction) -> String;
    fn translate_regimm(&mut self, inst: &Instruction) -> String;
    fn translate_cop0(&mut self, inst: &Instruction) -> String;
    fn translate_fpu(&mut self, inst: &Instruction) -> String;
    fn translate_vu(&mut self, inst: &Instruction) -> String;
    fn translate_mmi(&mut self, inst: &Instruction) -> String;
    fn emit_unhandled(&mut self, inst: &Instruction, reason: &str) -> String;

    /// MMI sub-families (`mmi_translation_helpers.cpp`). Defaults report them as
    /// unimplemented so a generator can adopt them one at a time.
    fn translate_mmi0(&mut self, inst: &Instruction) -> String {
        self.emit_unhandled(inst, "MMI0 instruction")
    }
    fn translate_mmi1(&mut self, inst: &Instruction) -> String {
        self.emit_unhandled(inst, "MMI1 instruction")
    }
    fn translate_mmi2(&mut self, inst: &Instruction) -> String {
        self.emit_unhandled(inst, "MMI2 instruction")
    }
    fn translate_mmi3(&mut self, inst: &Instruction) -> String {
        self.emit_unhandled(inst, "MMI3 instruction")
    }
    fn translate_pmfhl(&mut self, inst: &Instruction) -> String {
        self.emit_unhandled(inst, "PMFHL instruction")
    }
    fn translate_pmthl(&mut self, inst: &Instruction) -> String {
        self.emit_unhandled(inst, "PMTHL instruction")
    }
    /// The whole `COP2_CO` VU0 macro block (`vu_translation_helpers.cpp`).
    fn translate_vu_macro(&mut self, inst: &Instruction) -> String {
        self.emit_unhandled(inst, "VU0 macro instruction")
    }
}

/// `buildAbsoluteJumpTarget`.
pub fn build_absolute_jump_target(address: u32, target: u32) -> u32 {
    (address.wrapping_add(4) & 0xF000_0000) | (target << 2)
}

/// Translate one instruction to the C++ the runtime executes.
pub fn translate(hooks: &mut impl FamilyTranslator, inst: &Instruction, memory_hint: MemoryAccessHint) -> String {
    if inst.is_mmi {
        return hooks.translate_mmi(inst);
    }

    // `effectiveMemoryHintFor` is a no-op in the C++ too ("disable for now since
    // it causing issues with some games").
    let hint = memory_hint;
    let gen_read = |width: i32, addr: &str| translate_memory_read(inst, hint, width, addr);
    let gen_write = |width: i32, addr: &str, value: &str| translate_memory_write(inst, hint, width, addr, value);
    let indexed = |reg: u32| format!("ADD32(GPR_U32(ctx, {}), {})", reg, inst.simmediate);

    match inst.opcode {
        OPCODE_SPECIAL => hooks.translate_special(inst),
        OPCODE_REGIMM => hooks.translate_regimm(inst),
        OPCODE_COP0 => hooks.translate_cop0(inst),
        OPCODE_COP1 => hooks.translate_fpu(inst),
        OPCODE_COP2 => hooks.translate_vu(inst),

        OPCODE_ADDI => {
            if inst.rt == 0 {
                "// NOP (addi to $zero)".to_owned()
            } else {
                format!(
                    "{{ uint32_t tmp; bool ov; ADD32_OV(GPR_U32(ctx, {}), (int32_t){}, tmp, ov); \
                     if (ov) runtime->SignalException(ctx, EXCEPTION_INTEGER_OVERFLOW); \
                     else SET_GPR_S32(ctx, {}, (int32_t)tmp); }}",
                    inst.rs, inst.simmediate, inst.rt
                )
            }
        }
        OPCODE_ADDIU => {
            if inst.rt == 0 {
                "// NOP (addiu $zero, ...)".to_owned()
            } else {
                format!(
                    "SET_GPR_S32(ctx, {}, (int32_t)ADD32(GPR_U32(ctx, {}), {}));",
                    inst.rt, inst.rs, inst.simmediate
                )
            }
        }
        OPCODE_SLTI => format!(
            "SET_GPR_U64(ctx, {}, ((int64_t)GPR_S64(ctx, {}) < (int64_t)(int32_t){}) ? 1 : 0);",
            inst.rt, inst.rs, inst.simmediate
        ),
        OPCODE_SLTIU => format!(
            "SET_GPR_U64(ctx, {}, ((uint64_t)GPR_U64(ctx, {}) < (uint64_t)(int64_t)(int32_t){}) ? 1 : 0);",
            inst.rt, inst.rs, inst.simmediate
        ),
        OPCODE_ANDI => format!(
            "SET_GPR_U64(ctx, {}, GPR_U64(ctx, {}) & (uint64_t)(uint16_t){});",
            inst.rt, inst.rs, inst.immediate
        ),
        OPCODE_ORI => format!(
            "SET_GPR_U64(ctx, {}, GPR_U64(ctx, {}) | (uint64_t)(uint16_t){});",
            inst.rt, inst.rs, inst.immediate
        ),
        OPCODE_XORI => format!(
            "SET_GPR_U64(ctx, {}, GPR_U64(ctx, {}) ^ (uint64_t)(uint16_t){});",
            inst.rt, inst.rs, inst.immediate
        ),
        OPCODE_LUI => format!("SET_GPR_S32(ctx, {}, (int32_t)((uint32_t){} << 16));", inst.rt, inst.immediate),

        OPCODE_LB => format!("SET_GPR_S32(ctx, {}, (int8_t){});", inst.rt, gen_read(8, &indexed(inst.rs))),
        OPCODE_LH => format!("SET_GPR_S32(ctx, {}, (int16_t){});", inst.rt, gen_read(16, &indexed(inst.rs))),
        OPCODE_LW => format!("SET_GPR_S32(ctx, {}, (int32_t){});", inst.rt, gen_read(32, &indexed(inst.rs))),
        OPCODE_LBU => format!("SET_GPR_ZE32(ctx, {}, (uint8_t){});", inst.rt, gen_read(8, &indexed(inst.rs))),
        OPCODE_LHU => format!("SET_GPR_ZE32(ctx, {}, (uint16_t){});", inst.rt, gen_read(16, &indexed(inst.rs))),
        OPCODE_LWU => format!("SET_GPR_ZE32(ctx, {}, {});", inst.rt, gen_read(32, &indexed(inst.rs))),
        OPCODE_SB => {
            format!("{};", gen_write(8, &indexed(inst.rs), &format!("(uint8_t)GPR_U32(ctx, {})", inst.rt)))
        }
        OPCODE_SH => {
            format!("{};", gen_write(16, &indexed(inst.rs), &format!("(uint16_t)GPR_U32(ctx, {})", inst.rt)))
        }
        OPCODE_SW => format!("{};", gen_write(32, &indexed(inst.rs), &format!("GPR_U32(ctx, {})", inst.rt))),
        OPCODE_LQ => format!("SET_GPR_VEC(ctx, {}, {});", inst.rt, gen_read(128, &indexed(inst.rs))),
        OPCODE_SQ => {
            format!("{};", gen_write(128, &indexed(inst.rs), &format!("GPR_VEC(ctx, {})", inst.rt)))
        }
        OPCODE_LD => format!("SET_GPR_U64(ctx, {}, {});", inst.rt, gen_read(64, &indexed(inst.rs))),
        OPCODE_SD => format!("{};", gen_write(64, &indexed(inst.rs), &format!("GPR_U64(ctx, {})", inst.rt))),

        OPCODE_LWC1 => format!(
            "{{ uint32_t bits = {}; float f; std::memcpy(&f, &bits, sizeof(f)); ctx->f[{}] = f; }}",
            gen_read(32, &indexed(inst.rs)),
            inst.rt
        ),
        OPCODE_SWC1 => format!(
            "{{ float f = ctx->f[{}]; uint32_t bits; std::memcpy(&bits, &f, sizeof(bits)); {}; }}",
            inst.rt,
            gen_write(32, &indexed(inst.rs), "bits")
        ),
        OPCODE_LDC2 => format!("ctx->vu0_vf[{}] = _mm_castsi128_ps({});", inst.rt, gen_read(128, &indexed(inst.rs))),
        OPCODE_SDC2 => {
            format!("{};", gen_write(128, &indexed(inst.rs), &format!("_mm_castps_si128(ctx->vu0_vf[{}])", inst.rt)))
        }

        // The adjacent C++ literals keep their leading spaces; `concat!` reproduces them.
        OPCODE_DADDI => format!(
            concat!(
                "{{ int64_t src = (int64_t)GPR_S64(ctx, {}); ",
                "int64_t imm = (int64_t)(int32_t){}; ",
                "int64_t res = src + imm; ",
                "if (((src ^ imm) >= 0) && ((src ^ res) < 0)) ",
                "    runtime->SignalException(ctx, EXCEPTION_INTEGER_OVERFLOW); ",
                "else SET_GPR_S64(ctx, {}, res); }}"
            ),
            inst.rs, inst.simmediate, inst.rt
        ),
        OPCODE_DADDIU => format!(
            "SET_GPR_S64(ctx, {}, (int64_t)GPR_S64(ctx, {}) + (int64_t)(int32_t){});",
            inst.rt, inst.rs, inst.simmediate
        ),

        OPCODE_J => {
            format!("// J 0x{:X} - Handled by branch logic", build_absolute_jump_target(inst.address, inst.target))
        }
        OPCODE_JAL => {
            format!("// JAL 0x{:X} - Handled by branch logic", build_absolute_jump_target(inst.address, inst.target))
        }
        OPCODE_BEQ | OPCODE_BNE | OPCODE_BLEZ | OPCODE_BGTZ | OPCODE_BEQL | OPCODE_BNEL | OPCODE_BLEZL
        | OPCODE_BGTZL => {
            format!("// Likely branch instruction at 0x{:X} - Handled by branch logic", inst.address)
        }

        OPCODE_LDL => format!(
            "{{ uint32_t addr = ADD32(GPR_U32(ctx, {}), {}); \
             uint32_t aligned_addr = addr & ~7u; \
             uint32_t offset = addr & 7u; \
             uint64_t mem = {}; \
             uint32_t shift = (7u - offset) << 3; \
             uint64_t keepMask = (shift == 0) ? 0ull : ((1ull << shift) - 1ull); \
             SET_GPR_U64(ctx, {}, (GPR_U64(ctx, {}) & keepMask) | (mem << shift)); }}",
            inst.rs,
            inst.simmediate,
            gen_read(64, "aligned_addr"),
            inst.rt,
            inst.rt
        ),
        OPCODE_LDR => format!(
            "{{ uint32_t addr = ADD32(GPR_U32(ctx, {}), {}); \
             uint32_t aligned_addr = addr & ~7u; \
             uint32_t offset = addr & 7u; \
             uint64_t mem = {}; \
             uint32_t shift = offset << 3; \
             uint64_t keepMask = (offset == 0) ? 0ull : (0xFFFFFFFFFFFFFFFFull << ((8u - offset) << 3)); \
             SET_GPR_U64(ctx, {}, (GPR_U64(ctx, {}) & keepMask) | (mem >> shift)); }}",
            inst.rs,
            inst.simmediate,
            gen_read(64, "aligned_addr"),
            inst.rt,
            inst.rt
        ),
        OPCODE_LWL => format!(
            "{{ uint32_t addr = ADD32(GPR_U32(ctx, {}), {}); \
             uint32_t aligned_addr = addr & ~3u; \
             uint32_t offset = addr & 3u; \
             uint32_t mem = {}; \
             uint32_t shift = (3u - offset) << 3; \
             uint32_t keepMask = (shift == 0) ? 0u : ((1u << shift) - 1u); \
             uint32_t merged = (GPR_U32(ctx, {}) & keepMask) | (mem << shift); \
             SET_GPR_S32(ctx, {}, (int32_t)merged); }}",
            inst.rs,
            inst.simmediate,
            gen_read(32, "aligned_addr"),
            inst.rt,
            inst.rt
        ),
        OPCODE_LWR => format!(
            "{{ uint32_t addr = ADD32(GPR_U32(ctx, {}), {}); \
             uint32_t aligned_addr = addr & ~3u; \
             uint32_t offset = addr & 3u; \
             uint32_t mem = {}; \
             uint32_t shift = offset << 3; \
             uint32_t keepMask = (offset == 0) ? 0u : (0xFFFFFFFFu << ((4u - offset) << 3)); \
             uint32_t merged32 = (GPR_U32(ctx, {}) & keepMask) | (mem >> shift); \
             uint64_t merged64 = (GPR_U64(ctx, {}) & 0xFFFFFFFF00000000ull) | (uint64_t)merged32; \
             if (offset == 0) merged64 = (uint64_t)(int64_t)(int32_t)merged32; \
             SET_GPR_U64(ctx, {}, merged64); }}",
            inst.rs,
            inst.simmediate,
            gen_read(32, "aligned_addr"),
            inst.rt,
            inst.rt,
            inst.rt
        ),
        OPCODE_SWL => format!(
            "{{ uint32_t addr = ADD32(GPR_U32(ctx, {}), {}); \
             uint32_t aligned_addr = addr & ~3u; \
             uint32_t offset = addr & 3u; \
             uint32_t shift = (3u - offset) << 3; \
             uint32_t mask = 0xFFFFFFFFu >> shift; \
             uint32_t old_data = {}; \
             uint32_t val = GPR_U32(ctx, {}); \
             uint32_t new_data = (old_data & ~mask) | ((val >> shift) & mask); \
             {}; }}",
            inst.rs,
            inst.simmediate,
            gen_read(32, "aligned_addr"),
            inst.rt,
            gen_write(32, "aligned_addr", "new_data")
        ),
        OPCODE_SWR => format!(
            "{{ uint32_t addr = ADD32(GPR_U32(ctx, {}), {}); \
             uint32_t aligned_addr = addr & ~3u; \
             uint32_t offset = addr & 3u; \
             uint32_t shift = offset << 3; \
             uint32_t mask = 0xFFFFFFFFu << shift; \
             uint32_t old_data = {}; \
             uint32_t val = GPR_U32(ctx, {}); \
             uint32_t new_data = (old_data & ~mask) | ((val << shift) & mask); \
             {}; }}",
            inst.rs,
            inst.simmediate,
            gen_read(32, "aligned_addr"),
            inst.rt,
            gen_write(32, "aligned_addr", "new_data")
        ),
        OPCODE_SDL => format!(
            "{{ uint32_t addr = ADD32(GPR_U32(ctx, {}), {}); \
             uint32_t aligned_addr = addr & ~7u; \
             uint32_t offset = addr & 7u; \
             uint32_t shift = (7u - offset) << 3; \
             uint64_t mask = 0xFFFFFFFFFFFFFFFFull >> shift; \
             uint64_t old_data = {}; \
             uint64_t val = GPR_U64(ctx, {}); \
             uint64_t new_data = (old_data & ~mask) | ((val >> shift) & mask); \
             {}; }}",
            inst.rs,
            inst.simmediate,
            gen_read(64, "aligned_addr"),
            inst.rt,
            gen_write(64, "aligned_addr", "new_data")
        ),
        OPCODE_SDR => format!(
            "{{ uint32_t addr = ADD32(GPR_U32(ctx, {}), {}); \
             uint32_t aligned_addr = addr & ~7u; \
             uint32_t offset = addr & 7u; \
             uint32_t shift = offset << 3; \
             uint64_t mask = 0xFFFFFFFFFFFFFFFFull << shift; \
             uint64_t old_data = {}; \
             uint64_t val = GPR_U64(ctx, {}); \
             uint64_t new_data = (old_data & ~mask) | ((val << shift) & mask); \
             {}; }}",
            inst.rs,
            inst.simmediate,
            gen_read(64, "aligned_addr"),
            inst.rt,
            gen_write(64, "aligned_addr", "new_data")
        ),

        OPCODE_CACHE => "// CACHE instruction (ignored)".to_owned(),
        OPCODE_PREF => "// PREF instruction (ignored)".to_owned(),

        OPCODE_LL => format!(
            "{{ uint32_t addr = ADD32(GPR_U32(ctx, {}), {}); \
             SET_GPR_S32(ctx, {}, (int32_t)READ32(addr)); \
             ctx->llbit = 1; ctx->lladdr = addr; }}",
            inst.rs, inst.simmediate, inst.rt
        ),
        OPCODE_SC => format!(
            "{{ uint32_t addr = ADD32(GPR_U32(ctx, {}), {}); \
             if (ctx->llbit && ctx->lladdr == addr) {{ WRITE32(addr, GPR_U32(ctx, {})); \
             SET_GPR_S32(ctx, {}, 1); }} \
             else {{ SET_GPR_S32(ctx, {}, 0); }} \
             ctx->llbit = 0; ctx->lladdr = 0; }}",
            inst.rs, inst.simmediate, inst.rt, inst.rt, inst.rt
        ),

        _ => hooks.emit_unhandled(inst, &format!("Unhandled opcode: 0x{:X}", inst.opcode)),
    }
}
