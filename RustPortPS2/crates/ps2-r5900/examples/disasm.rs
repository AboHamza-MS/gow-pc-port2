//! Prints the Rabbitizer disassembly of a few instructions.
//!
//! Used to pin the tests and to eyeball the syntax after a Rabbitizer update:
//!
//! ```sh
//! cargo run -p ps2-r5900 --example disasm
//! ```

const WORDS: &[(u32, &str)] = &[
    (0x27BD_FFE8, "addiu sp, sp, -0x18"),
    (0x03E0_0008, "jr ra"),
    (0x0000_0000, "nop"),
    (0x0C00_0004, "jal 0x00100010"),
    (0x2402_0001, "addiu v0, zero, 1"),
    (0x0000_000C, "syscall"),
    (0x0085_1020, "add v0, a0, a1"),
    (0x8C88_0000, "lw t0, 0(a0)"),
    (0xAC88_0000, "sw t0, 0(a0)"),
    (0x7888_0000, "lq t0, 0(a0)"),
    (0x7C88_0000, "sq t0, 0(a0)"),
    (0x0085_0018, "mult a0, a1"),
    (0x4A02_09C0, "COP2 macro (VU0)"),
    (0x4842_0000, "qmfc2"),
];

fn main() {
    for (word, hint) in WORDS {
        let text = ps2_r5900::disassemble(0x0010_0000, *word);
        println!("{word:08X}  {text:?}   ({hint})");
    }
}
