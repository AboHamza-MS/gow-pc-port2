//! Tests of `ps2-dev rate-readers` on synthetic generated code (the recompiler's comment format, no game files).

use ps2_devtools::rate::{rate_constant, report, scan_source, DEFAULT_DELTA, DEFAULT_RATE};

/// Two functions in the recompiler's layout: a delta reader that also builds 60.0, and a per-frame function that
/// holds 1/60 and writes the floor in fields from a call's delay slot.
const SOURCE: &str = r#"
// Function: sub_00200000
// Address: 0x200000 - 0x200040
void sub_00200000_0x200000(uint8_t* rdram, R5900Context* ctx, PS2Runtime *runtime) {
    // 0x200000: 0x3c02002a  lui         $v0, 0x2A
    SET_GPR_S32(ctx, 2, 0);
    // 0x200004: 0xc440c64c  lwc1        $f0, -0x39B4($v0)
    // 0x200008: 0x3c014270  lui         $at, 0x4270
    // 0x20000c: 0x44810800  mtc1        $at, $f1
    // 0x200010: 0x8c43c654  lw          $v1, -0x39AC($v0)
    // 0x200014: 0x0  nop
}

// Function: sub_00200040
// Address: 0x200040 - 0x200080
void sub_00200040_0x200040(uint8_t* rdram, R5900Context* ctx, PS2Runtime *runtime) {
    // 0x200040: 0x3c013c88  lui         $at, 0x3C88
    // 0x200044: 0x34218889  ori         $at, $at, 0x8889
    // 0x200048: 0x44810000  mtc1        $at, $f0
    // 0x20004c: 0x3c04002a  lui         $a0, 0x2A
    // 0x200050: 0xc05f5de  jal         func_17D778
    // 0x200054: 0xac80c654  sw          $zero, -0x39AC($a0) (Delay Slot)
    // 0x200058: 0x8c82c640  lw          $v0, -0x39C0($a0)
    // 0x20005c: 0x2404003c  addiu       $a0, $zero, 0x3C
    // 0x200060: 0x44840000  mtc1        $a0, $f2
}
"#;

#[test]
fn constants_match_with_rounding() {
    assert_eq!(rate_constant(0x4270_0000), Some("60.0"));
    assert_eq!(rate_constant(0x3C88_8889), Some("1/60"));
    assert_eq!(rate_constant(0x3C88_8888), Some("1/60"));
    assert_eq!(rate_constant(0x41F0_0000), Some("30.0"));
    assert_eq!(rate_constant(0x3F80_0000), None); // 1.0
    assert_eq!(rate_constant(0), None);
}

#[test]
fn scan_resolves_addresses_and_constants() {
    let scans = scan_source(SOURCE);
    assert_eq!(scans.len(), 2);
    let reader = &scans[0];
    assert_eq!((reader.name.as_str(), reader.address), ("sub_00200000", 0x200000));
    let reads: Vec<u32> = reader.accesses.iter().map(|a| a.address).collect();
    assert_eq!(reads, [0x29C64C, 0x29C654]);
    assert!(reader.accesses.iter().all(|a| !a.write));
    assert_eq!(reader.constants.len(), 1);
    assert_eq!(reader.constants[0].name, "60.0");

    let frame = &scans[1];
    // the store in the call's delay slot still sees $a0; after it the call clobbers $a0, so the next load is unknown
    assert_eq!(frame.accesses.len(), 1);
    assert_eq!((frame.accesses[0].address, frame.accesses[0].write, frame.accesses[0].pc), (0x29C654, true, 0x200054));
    // 0x3C moved as an integer bit pattern is not a float rate constant
    let names: Vec<&str> = frame.constants.iter().map(|c| c.name).collect();
    assert_eq!(names, ["1/60"]);
}

#[test]
fn report_lists_readers_writers_and_suspects() {
    let scans = scan_source(SOURCE);
    let text = report(&scans, &DEFAULT_RATE, &DEFAULT_DELTA, &[0x29C640]);
    assert!(text.contains("rate 0x0029C654 read by 1 function(s)\n  sub_00200000 at 0x200010\n"), "{text}");
    assert!(text.contains("rate 0x0029C654 written by 1 function(s)\n  sub_00200040 at 0x200054\n"), "{text}");
    assert!(text.contains("delta 0x0029C64C read by 1 function(s)\n  sub_00200000 at 0x200004\n"), "{text}");
    assert!(text.contains("watch 0x0029C640 read by 0 function(s)\n"), "{text}");
    assert!(text.contains("delta readers with a rate constant: 1\n  sub_00200000 [60.0]\n"), "{text}");
    assert!(
        text.contains("rate constants without a delta read (per-frame candidates): 1\n  sub_00200040 [1/60]\n"),
        "{text}"
    );
}
