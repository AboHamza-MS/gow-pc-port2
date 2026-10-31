//! The `COP0` templates.

use ps2_r5900::opcodes::*;
use ps2_r5900::Instruction;
use ps2_recomp::translate::cop0::translate;
use ps2_recomp::translate::instruction::{translate as translate_generic, FamilyTranslator};
use ps2_recomp::translate::memory::MemoryAccessHint;

struct Hooks;

impl FamilyTranslator for Hooks {
    fn translate_special(&mut self, _: &Instruction) -> String {
        String::new()
    }
    fn translate_regimm(&mut self, _: &Instruction) -> String {
        String::new()
    }
    fn translate_cop0(&mut self, inst: &Instruction) -> String {
        translate(self, inst)
    }
    fn translate_fpu(&mut self, _: &Instruction) -> String {
        String::new()
    }
    fn translate_vu(&mut self, _: &Instruction) -> String {
        String::new()
    }
    fn translate_mmi(&mut self, _: &Instruction) -> String {
        String::new()
    }
    fn emit_unhandled(&mut self, _: &Instruction, reason: &str) -> String {
        format!("<unhandled: {reason}>")
    }
}

fn cop0(format: u32, rt: u32, rd: u32, function: u32) -> String {
    let raw = (OPCODE_COP0 << 26) | (format << 21) | (rt << 16) | (rd << 11) | function;
    translate_generic(&mut Hooks, &ps2_r5900::decode(0x0010_0000, raw, false), MemoryAccessHint::default())
}

#[test]
fn mfc0_reads_a_named_register() {
    assert_eq!(cop0(COP0_MF, 2, COP0_REG_STATUS, 0), "SET_GPR_S32(ctx, 2, (int32_t)ctx->cop0_status);");
    assert_eq!(cop0(COP0_MF, 2, COP0_REG_EPC, 0), "SET_GPR_S32(ctx, 2, (int32_t)ctx->cop0_epc);");
}

#[test]
fn mtc0_masks_the_value_and_handles_the_read_only_registers() {
    assert_eq!(cop0(COP0_MT, 4, COP0_REG_INDEX, 0), "ctx->cop0_index = GPR_U32(ctx, 4) & 0x3F;");
    assert_eq!(cop0(COP0_MT, 4, COP0_REG_WIRED, 0), "ctx->cop0_wired = GPR_U32(ctx, 4) & 0x3F; ctx->cop0_random = 47;");
    assert_eq!(cop0(COP0_MT, 4, COP0_REG_PRID, 0), "// MTC0 to PRID register ignored (read-only)");
}

#[test]
fn an_unknown_register_still_writes_zero() {
    // rd 0x1F is past ERROREPC (0x1E), the last named register.
    assert_eq!(cop0(COP0_MF, 2, 0x1F, 0), "SET_GPR_S32(ctx, 2, 0);  // Unimplemented COP0 register 31");
}

#[test]
fn eret_reproduces_the_multiline_exception_return() {
    let text = cop0(COP0_CO, 0, 0, COP0_CO_ERET);
    assert!(text.starts_with("if (ctx->cop0_status & 0x4) { \n    ctx->pc = ctx->cop0_errorepc; \n"), "{text}");
    assert!(text.ends_with("} \nruntime->clearLLBit(ctx); \nreturn;"), "{text}");
}

#[test]
fn tlb_operations_call_the_runtime_handlers() {
    assert_eq!(cop0(COP0_CO, 0, 0, COP0_CO_TLBR), "runtime->handleTLBR(rdram, ctx);");
    assert_eq!(cop0(COP0_CO, 0, 0, COP0_CO_TLBP), "runtime->handleTLBP(rdram, ctx);");
}

#[test]
fn interrupt_enable_and_disable_keep_the_comment() {
    assert_eq!(cop0(COP0_CO, 0, 0, COP0_CO_EI), "ctx->cop0_status |= 0x10000; // Enable interrupts");
    assert_eq!(cop0(COP0_CO, 0, 0, COP0_CO_DI), "ctx->cop0_status &= ~0x10000; // Disable interrupts");
}

#[test]
fn an_unknown_format_is_reported() {
    assert_eq!(cop0(0x3, 0, 0, 0), "<unhandled: Unhandled COP0 instruction format: 0x3>");
}
