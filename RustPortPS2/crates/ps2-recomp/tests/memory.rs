//! The GOW register interception and the memory templates.
//!
//! The address classifiers are the GOW port's own additions: an access the
//! analysis resolved to an EE timer, a DMA channel register or a GS display
//! register becomes a native call, not a hardware-register access.

use ps2_r5900::Instruction;
use ps2_recomp::translate::memory::{
    address_literal, gen_fast_write, is_ee_dma_register, is_ee_timer_register, is_gs_display_register,
    is_gs_status_register, memory_value_type, ps2_is_special_address, translate_memory_read, translate_memory_write,
    MemoryAccessHint,
};

#[test]
fn the_ee_timers_are_the_count_mode_comp_registers_of_t0_to_t3() {
    // T0 COUNT.
    assert_eq!(is_ee_timer_register(0x1000_0000), Some(0x1000_0000));
    // T3 COUNT is outside the `offset == 0x30` rule (only T0/T1 have HOLD).
    assert_eq!(is_ee_timer_register(0x1000_1800), Some(0x1000_1800));
    assert_eq!(is_ee_timer_register(0x1000_0830), Some(0x1000_0830), "T1 HOLD");
    assert_eq!(is_ee_timer_register(0x1000_1030), None, "T2 has no HOLD");
    // Unmapped offsets inside the timer window.
    assert_eq!(is_ee_timer_register(0x1000_0040), None);
    // KSEG2 and the uncached RAM mirror never alias the timers.
    assert_eq!(is_ee_timer_register(0xC000_0000), None);
    assert_eq!(is_ee_timer_register(0x2000_0000), None);
    // A different I/O block.
    assert_eq!(is_ee_timer_register(0x1000_2000), None);
}

#[test]
fn the_ee_dma_window_and_enablers_are_recognised() {
    assert_eq!(is_ee_dma_register(0x1000_8000), Some(0x1000_8000), "D_STAT");
    assert_eq!(is_ee_dma_register(0x1000_9000), Some(0x1000_9000));
    assert_eq!(is_ee_dma_register(0x1000_F520), Some(0x1000_F520), "D_ENABLER");
    assert_eq!(is_ee_dma_register(0x1000_F590), Some(0x1000_F590), "D_ENABLEW");
    assert_eq!(is_ee_dma_register(0x1000_F000), None, "past the window");
    assert_eq!(is_ee_dma_register(0xC000_0000), None);
}

#[test]
fn the_gs_display_registers_and_the_csr_are_split() {
    for register in [0x1200_0000, 0x1200_0020, 0x1200_0070, 0x1200_0080, 0x1200_0090, 0x1200_00A0, 0x1200_00E0] {
        assert_eq!(is_gs_display_register(register), Some(register), "{register:#x}");
        assert!(!is_gs_status_register(register));
    }
    assert_eq!(is_gs_display_register(0x1200_1000), None, "CSR is a status register");
    assert!(is_gs_status_register(0x1200_1000));
    assert!(!is_gs_status_register(0xC000_1000), "KSEG2 is never the CSR");
}

#[test]
fn special_addresses_cover_the_devices_and_kseg2() {
    assert!(ps2_is_special_address(0x1FC0_0000), "BIOS");
    assert!(ps2_is_special_address(0x7000_0000), "scratchpad");
    assert!(ps2_is_special_address(0x1000_8000), "I/O");
    assert!(ps2_is_special_address(0x1200_1000), "GS");
    assert!(ps2_is_special_address(0x1100_4000), "VU memory");
    assert!(ps2_is_special_address(0xC000_0000), "KSEG2");
    assert!(!ps2_is_special_address(0x8010_0000), "KSEG0 mirrors plain RAM");
    assert!(!ps2_is_special_address(0x0010_0000), "plain RAM");
    assert!(!ps2_is_special_address(0x01FF_FFFF), "just below the BIOS");
}

#[test]
fn literals_and_types_follow_the_cpp_spelling() {
    assert_eq!(address_literal(0x1000_0000), "0x10000000u");
    assert_eq!(address_literal(0x1FC0_0000), "0x1FC00000u");
    assert_eq!(memory_value_type(8), "uint8_t");
    assert_eq!(memory_value_type(64), "uint64_t");
    assert_eq!(memory_value_type(128), "");
}

#[test]
fn a_resolved_ram_read_uses_the_fast_path() {
    let inst = Instruction::new(0x0010_0000, 0, false);
    let read = translate_memory_read(&inst, MemoryAccessHint::resolved(0x0010_0000), 32, "addr");
    assert_eq!(read, "FAST_READ32(0x100000u)");

    // A special address goes through the runtime.
    let read = translate_memory_read(&inst, MemoryAccessHint::resolved(0x1FC0_0000), 32, "addr");
    assert_eq!(read, "runtime->Load32(rdram, ctx, 0x1FC00000u)");

    // A resolved timer register becomes the native clock.
    let read = translate_memory_read(&inst, MemoryAccessHint::resolved(0x1000_0000), 32, "addr");
    assert_eq!(read, "runtime->memory().eeTimerLoad(0x10000000u)");
}

#[test]
fn an_unresolved_read_uses_the_address_expression() {
    let inst = Instruction::new(0x0010_0000, 0, false);
    let read = translate_memory_read(&inst, MemoryAccessHint::default(), 32, "expr");
    assert_eq!(read, "READ32(expr)");

    let mut mmio = Instruction::new(0x0010_0000, 0, false);
    mmio.is_mmio = true;
    mmio.mmio_address = 0x1000_0000;
    let read = translate_memory_read(&mmio, MemoryAccessHint::default(), 32, "expr");
    assert!(read.starts_with("[&]() -> uint32_t { const uint32_t _a = expr;"), "{read}");
}

#[test]
fn a_resolved_write_is_traced_and_fast() {
    let inst = Instruction::new(0x0010_0000, 0, false);
    let write = translate_memory_write(&inst, MemoryAccessHint::resolved(0x0010_0000), 32, "addr", "value");
    assert!(write.starts_with("do { uint32_t _value = static_cast<uint32_t>(value);"), "{write}");
    assert!(write.contains("ps2TraceGuestWrite(rdram, 0x100000u, 4u, _value, 0u, \"WRITE32\", ctx);"), "{write}");
    assert!(write.ends_with("FAST_WRITE32(0x100000u, _value); } while (0)"), "{write}");

    // The 128-bit form uses the two 64-bit halves.
    let write = gen_fast_write(128, 0x0010_0000, "vec");
    assert!(write.contains("PS2_EXTRACT_EPI64_0"), "{write}");
    assert!(write.contains("\"WRITE128\""), "{write}");
}

#[test]
fn a_gs_display_store_goes_to_the_display_service() {
    let inst = Instruction::new(0x0010_0000, 0, false);
    let write = translate_memory_write(&inst, MemoryAccessHint::resolved(0x1200_0000), 64, "addr", "value");
    assert_eq!(write, "runtime->memory().gsDisplayStore(0x12000000u, value)");
}
