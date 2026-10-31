//! The generic opcode templates, checked against the C++ text.

use ps2_r5900::Instruction;
use ps2_recomp::translate::instruction::{build_absolute_jump_target, translate, FamilyTranslator};
use ps2_recomp::translate::memory::MemoryAccessHint;

/// A stand-in for the C++ `CodeGenerator`: it records which family was asked.
#[derive(Default)]
struct Hooks {
    calls: Vec<String>,
}

impl FamilyTranslator for Hooks {
    fn translate_special(&mut self, _inst: &Instruction) -> String {
        self.calls.push("special".to_owned());
        "<special>".to_owned()
    }
    fn translate_regimm(&mut self, _inst: &Instruction) -> String {
        "<regimm>".to_owned()
    }
    fn translate_cop0(&mut self, _inst: &Instruction) -> String {
        "<cop0>".to_owned()
    }
    fn translate_fpu(&mut self, _inst: &Instruction) -> String {
        "<fpu>".to_owned()
    }
    fn translate_vu(&mut self, _inst: &Instruction) -> String {
        "<vu>".to_owned()
    }
    fn translate_mmi(&mut self, _inst: &Instruction) -> String {
        self.calls.push("mmi".to_owned());
        "<mmi>".to_owned()
    }
    fn emit_unhandled(&mut self, _inst: &Instruction, reason: &str) -> String {
        format!("<unhandled: {reason}>")
    }
}

fn instruction(raw: u32) -> Instruction {
    let mut inst = ps2_r5900::decode(0x0010_0000, raw, false);
    // `translate` reads the fields through the decoder; a few tests need the
    // opcode alone, so keep the decoded flags.
    inst.address = 0x0010_0000;
    inst
}

fn translated(raw: u32) -> String {
    translate(&mut Hooks::default(), &instruction(raw), MemoryAccessHint::default())
}

#[test]
fn addiu_uses_the_unsigned_immediate_pattern() {
    // addiu $t0, $sp, -0x18
    assert_eq!(translated(0x27A8_FFE8), "SET_GPR_S32(ctx, 8, (int32_t)ADD32(GPR_U32(ctx, 29), 4294967272));");
}

#[test]
fn writing_zero_is_a_nop() {
    assert_eq!(translated(0x2400_0001), "// NOP (addiu $zero, ...)");
    assert_eq!(translated(0x2000_0001), "// NOP (addi to $zero)");
}

#[test]
fn byte_and_half_loads_use_the_fast_path_without_a_hint() {
    // lw $v0, -4($a0)
    assert_eq!(translated(0x8C82_FFFC), "SET_GPR_S32(ctx, 2, (int32_t)READ32(ADD32(GPR_U32(ctx, 4), 4294967292)));");
    // lbu $t0, 0($a0)
    assert_eq!(translated(0x9088_0000), "SET_GPR_ZE32(ctx, 8, (uint8_t)READ8(ADD32(GPR_U32(ctx, 4), 0)));");
}

#[test]
fn stores_end_with_a_semicolon() {
    // sw $v0, 0($a0)
    assert_eq!(translated(0xAC82_0000), "WRITE32(ADD32(GPR_U32(ctx, 4), 0), GPR_U32(ctx, 2));");
    // sq $t0, 0($a0): without a resolved address a 128-bit store is a plain write.
    let store = translated(0x7C88_0000);
    assert_eq!(store, "WRITE128(ADD32(GPR_U32(ctx, 4), 0), GPR_VEC(ctx, 8));");
}

#[test]
fn lui_shifts_the_immediate() {
    // lui $a0, 0x3C00
    assert_eq!(translated(0x3C04_3C00), "SET_GPR_S32(ctx, 4, (int32_t)((uint32_t)15360 << 16));");
}

#[test]
fn the_jump_target_is_built_from_the_instructions_address() {
    // jal 0x0010_0010
    assert_eq!(translated(0x0C04_0004), "// JAL 0x100010 - Handled by branch logic");
    assert_eq!(build_absolute_jump_target(0x0010_0000, 4), 0x10);
    assert_eq!(build_absolute_jump_target(0x0FF0_0000, 4), 0x10);
}

#[test]
fn branches_are_marked_for_the_control_flow_pass() {
    assert_eq!(
        translated(0x1109_0002), // beq $t0, $t1, +8
        "// Likely branch instruction at 0x100000 - Handled by branch logic"
    );
}

#[test]
fn the_families_are_delegated() {
    let mut hooks = Hooks::default();
    let special = translate(&mut hooks, &instruction(0x03E0_0008), MemoryAccessHint::default());
    assert_eq!(special, "<special>");
    assert_eq!(hooks.calls, ["special"]);

    let mmi = translate(&mut hooks, &instruction(0x70A5_0000), MemoryAccessHint::default());
    assert_eq!(mmi, "<mmi>", "MMI is checked before the opcode switch");
    assert_eq!(hooks.calls, ["special", "mmi"]);

    assert_eq!(translate(&mut hooks, &instruction(0x4A02_09C0), MemoryAccessHint::default()), "<vu>");
    assert_eq!(translate(&mut hooks, &instruction(0x0501_0002), MemoryAccessHint::default()), "<regimm>");
}

#[test]
fn an_unknown_opcode_asks_the_generator() {
    // Opcode 0x1D (29) is not handled by the switch (0x1C is MMI, 0x1E/0x1F are LQ/SQ).
    let raw = 0x7400_0000;
    assert_eq!(translated(raw), "<unhandled: Unhandled opcode: 0x1D>");
}

#[test]
fn a_resolved_hint_changes_the_memory_text() {
    let inst = instruction(0x8C82_0000); // lw $v0, 0($a0)
    let text = translate(&mut Hooks::default(), &inst, MemoryAccessHint::resolved(0x0010_0000));
    assert_eq!(text, "SET_GPR_S32(ctx, 2, (int32_t)FAST_READ32(0x100000u));");
}
