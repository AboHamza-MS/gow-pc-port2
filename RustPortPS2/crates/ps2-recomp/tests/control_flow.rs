//! `isGuestNop`.

use ps2_r5900::decode;
use ps2_recomp::translate::control_flow::is_guest_nop;

#[test]
fn a_zero_shift_is_a_nop() {
    assert!(is_guest_nop(&decode(0x0010_0000, 0x0000_0000, false)));
}

#[test]
fn addiu_zero_zero_zero_is_also_a_nop() {
    assert!(is_guest_nop(&decode(0x0010_0000, 0x2400_0000, false)));
    // addiu $zero, $zero, 1 is not a nop.
    assert!(!is_guest_nop(&decode(0x0010_0000, 0x2400_0001, false)));
}

#[test]
fn a_real_shift_is_not_a_nop() {
    // sll $v0, $t0, 0
    let shift = (8 << 16) | (2 << 11);
    assert!(!is_guest_nop(&decode(0x0010_0000, shift, false)));
}
