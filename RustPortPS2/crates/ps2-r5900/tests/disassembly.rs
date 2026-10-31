//! Field extraction and disassembly, pinned against Rabbitizer 1.14.3.
//!
//! The disassembly text is not decoration: the recompiler copies it into the
//! generated C++ comments, so any change here changes the generated tree.

use ps2_r5900::{disassemble, Fields, Instruction};

#[test]
fn extracts_the_bit_fields_like_the_cpp_macros() {
    let fields = Fields::new(0x27BD_FFE8);
    assert_eq!(fields.opcode, 9); // ADDIU
    assert_eq!(fields.rs, 29); // $sp
    assert_eq!(fields.rt, 29);
    assert_eq!(fields.immediate, 0xFFE8);
    assert_eq!(fields.simmediate_i32(), -24);
    assert_eq!(fields.target, 0x03BD_FFE8); // the low 26 bits, as `TARGET`

    let fields = Fields::new(0x0C00_0004); // jal
    assert_eq!(fields.opcode, 3);
    assert_eq!(fields.target, 4);

    let fields = Fields::new(0x03E0_0008); // jr $ra
    assert_eq!((fields.opcode, fields.function, fields.rs), (0, 8, 31));

    let fields = Fields::new(0x7C88_0000); // sq $t0, 0($a0)
    assert_eq!((fields.opcode, fields.rs, fields.rt, fields.sa), (0x1F, 4, 8, 0));
}

#[test]
fn disassembles_exactly_like_rabbitizer_1_14_3() {
    // Pinned output; the same strings end up in `// 0x...: 0x...  <text>`.
    let cases: &[(u32, &str)] = &[
        (0x27BD_FFE8, "addiu       $sp, $sp, -0x18"),
        (0x03E0_0008, "jr          $ra"),
        (0x0000_0000, "nop"),
        (0x0C00_0004, "jal         func_000010"),
        (0x2402_0001, "addiu       $v0, $zero, 0x1"),
        (0x0000_000C, "syscall     0"),
        (0x0085_1020, "add         $v0, $a0, $a1"),
        (0x8C88_0000, "lw          $t0, 0x0($a0)"),
        (0xAC88_0000, "sw          $t0, 0x0($a0)"),
        (0x7888_0000, "lq          $t0, 0x0($a0)"),
        (0x7C88_0000, "sq          $t0, 0x0($a0)"),
        (0x0085_0018, "mult        $zero, $a0, $a1"),
        (0x4A02_09C0, "vaddx       $vf7, $vf1, $vf2x"),
        (0x4842_0000, "cfc2.ni     $v0, $vi0"),
    ];
    for (raw, expected) in cases {
        assert_eq!(&disassemble(0x0010_0000, *raw), expected, "{raw:08X}");
    }
}

#[test]
fn the_disassembly_follows_the_instruction_address() {
    // `jal 0x0010_0004` at 0x0010_0000 lands on 0x0010_0010; the text names the
    // target relative to the instruction's own address.
    assert_eq!(disassemble(0x0010_0000, 0x0C00_0004), "jal         func_000010");
    assert!(disassemble(0x0010_0020, 0x0C00_0004).starts_with("jal"));
}

#[test]
fn the_instruction_caches_the_disassembly_only_when_asked() {
    let cached = Instruction::new(0x0010_0000, 0x27BD_FFE8, true);
    assert_eq!(cached.disassembly, "addiu       $sp, $sp, -0x18");

    let lazy = Instruction::new(0x0010_0000, 0x27BD_FFE8, false);
    assert!(lazy.disassembly.is_empty(), "nothing is disassembled unless requested");
    assert_eq!(lazy.disassembly(), cached.disassembly);
}

#[test]
fn defaults_match_the_cpp_initialisation() {
    let instruction = Instruction::new(0, 0, false);
    assert_eq!(instruction.vector.vector_field, 0xF);
    assert!(!instruction.vector.is_vector);
    assert!(!instruction.modification.modifies_gpr);
    assert!(!instruction.is_mmi);
    assert!(!instruction.is_vu);
    assert_eq!(instruction.mmi_type, 0);
    assert_eq!(instruction.mmio_address, 0);
}
