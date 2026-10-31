//! Memory-access translation: the text `translateMemoryRead`/`translateMemoryWrite` emit.
//!
//! Port of the memory half of `PS2Recomp/ps2xRecomp/src/lib/instruction_translator.cpp`
//! plus `runtime/ps2_address.h`. A constant resolved by the analysis becomes a
//! `FAST_READ32`/fast-write block; a special address (BIOS, scratchpad, I/O, GS,
//! VU memory, KSEG2/3) becomes a runtime call; and the GOW port's EE timer, DMA
//! and GS display registers become direct calls of the runtime's native services
//! instead of hardware-register accesses.

use ps2_r5900::Instruction;

/// Address the analysis resolved for an access, or none when it is computed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MemoryAccessHint {
    pub has_address: bool,
    pub address: u32,
}

impl MemoryAccessHint {
    pub fn resolved(address: u32) -> Self {
        Self { has_address: true, address }
    }
}

// `runtime/ps2_address.h` and `runtime/ps2_memory.h`.
const KSEG0_BASE: u32 = 0x8000_0000;
const KSEG2_BASE: u32 = 0xC000_0000;
const BIOS_BASE: u32 = 0x1FC0_0000;
const BIOS_SIZE: u32 = 4 * 1024 * 1024;
const SCRATCHPAD_BASE: u32 = 0x7000_0000;
const SCRATCHPAD_SIZE: u32 = 16 * 1024;
const IO_BASE: u32 = 0x1000_0000;
const IO_SIZE: u32 = 0x1_0000;
const GS_BASE: u32 = 0x1200_0000;
const GS_PRIV_REG_SIZE: u32 = 0x2000;
const VU0_DATA_BASE: u32 = 0x1100_4000;
const VU1_CODE_BASE: u32 = 0x1100_8000;
const VU1_CODE_SIZE: u32 = 16 * 1024;

// Wrapping subtraction on purpose: it is the C++ `Ps2AddressInRange`, and it also
// rejects addresses below `base` without a second comparison.
#[allow(clippy::manual_range_contains)]
fn in_range(value: u32, base: u32, size: u32) -> bool {
    value.wrapping_sub(base) < size
}

fn physical_address(address: u32) -> u32 {
    if address >= KSEG0_BASE {
        address & 0x1FFF_FFFF
    } else {
        address
    }
}

fn is_physical_special(physical: u32) -> bool {
    in_range(physical, BIOS_BASE, BIOS_SIZE)
        || in_range(physical, SCRATCHPAD_BASE, SCRATCHPAD_SIZE)
        || in_range(physical, IO_BASE, IO_SIZE)
        || in_range(physical, GS_BASE, GS_PRIV_REG_SIZE)
        || (VU0_DATA_BASE..VU1_CODE_BASE + VU1_CODE_SIZE).contains(&physical)
}

/// `Ps2IsSpecialAddress`: an address that needs the runtime's device path.
pub fn ps2_is_special_address(address: u32) -> bool {
    if address >= KSEG2_BASE {
        return true;
    }
    is_physical_special(physical_address(address))
}

/// `Ps2Memory::isEeTimerRegister`, as used by the translator.
pub fn is_ee_timer_register(address: u32) -> Option<u32> {
    if address >= KSEG2_BASE || (0x2000_0000..KSEG0_BASE).contains(&address) {
        return None;
    }
    let physical = address & 0x1FFF_FFFF;
    let base = physical & !0x7FF;
    let offset = physical & 0x7FF;
    if base != 0x1000_0000 && base != 0x1000_0800 && base != 0x1000_1000 && base != 0x1000_1800 {
        return None;
    }
    if offset == 0x00 || offset == 0x10 || offset == 0x20 || (offset == 0x30 && base < 0x1000_1000) {
        return Some(physical);
    }
    None
}

/// `Ps2Memory::isEeDmaRegister`.
pub fn is_ee_dma_register(address: u32) -> Option<u32> {
    if address >= KSEG2_BASE || (0x2000_0000..KSEG0_BASE).contains(&address) {
        return None;
    }
    let physical = address & 0x1FFF_FFFF;
    if (0x1000_8000..0x1000_F000).contains(&physical) || physical == 0x1000_F520 || physical == 0x1000_F590 {
        return Some(physical);
    }
    None
}

/// The GS display registers the runtime intercepts (PMODE, SMODE2, DISPFB1/2, DISPLAY1/2, BGCOLOR).
pub fn is_gs_display_register(address: u32) -> Option<u32> {
    if address >= KSEG2_BASE || (0x2000_0000..KSEG0_BASE).contains(&address) {
        return None;
    }
    let physical = address & 0x1FFF_FFFF;
    matches!(physical, 0x1200_0000 | 0x1200_0020 | 0x1200_0070 | 0x1200_0080 | 0x1200_0090 | 0x1200_00A0 | 0x1200_00E0)
        .then_some(physical)
}

/// `Ps2Memory::isGsStatusRegister`: CSR, the GS status register.
pub fn is_gs_status_register(address: u32) -> bool {
    address < KSEG2_BASE && !(0x2000_0000..KSEG0_BASE).contains(&address) && (address & 0x1FFF_FFFF) == 0x1200_1000
}

/// `addressLiteral`: the C++ `0x{:X}u` form.
pub fn address_literal(address: u32) -> String {
    format!("0x{address:X}u")
}

/// The C++ type used for an access of `width` bits, or `""` when unsupported.
pub fn memory_value_type(width: i32) -> &'static str {
    match width {
        8 => "uint8_t",
        16 => "uint16_t",
        32 => "uint32_t",
        64 => "uint64_t",
        _ => "",
    }
}

/// `genFastWrite`: the traced fast-write block for a resolved address.
pub fn gen_fast_write(width: i32, address: u32, value: &str) -> String {
    let address = address_literal(address);
    if width == 128 {
        return format!(
            "do {{ __m128i _value = ({}); \
             const uint64_t _lo = static_cast<uint64_t>(PS2_EXTRACT_EPI64_0(_value)); \
             const uint64_t _hi = static_cast<uint64_t>(PS2_EXTRACT_EPI64_1(_value)); \
             ps2TraceGuestWrite(rdram, {}, 16u, _lo, _hi, \"WRITE128\", ctx); \
             FAST_WRITE128({}, _value); }} while (0)",
            value, address, address
        );
    }

    let value_type = memory_value_type(width);
    format!(
        "do {{ {} _value = static_cast<{}>({}); \
         ps2TraceGuestWrite(rdram, {}, {}u, _value, 0u, \"WRITE{}\", ctx); \
         FAST_WRITE{}({}, _value); }} while (0)",
        value_type,
        value_type,
        value,
        address,
        width / 8,
        width,
        width,
        address
    )
}

/// `translateMemoryRead`.
pub fn translate_memory_read(inst: &Instruction, hint: MemoryAccessHint, width: i32, addr: &str) -> String {
    if hint.has_address {
        let resolved = hint.address;
        if width == 32 {
            if let Some(timer) = is_ee_timer_register(resolved) {
                return format!("runtime->memory().eeTimerLoad({})", address_literal(timer));
            }
            if let Some(dma) = is_ee_dma_register(resolved) {
                return format!("runtime->memory().eeDmaLoad({})", address_literal(dma));
            }
        }
        if width == 64 && is_gs_status_register(resolved) {
            return "runtime->memory().gsStatusLoad()".to_owned();
        }
        if inst.is_mmio || ps2_is_special_address(resolved) {
            return format!("runtime->Load{}(rdram, ctx, {})", width, address_literal(resolved));
        }
        return format!("FAST_READ{}({})", width, address_literal(resolved));
    }

    if inst.is_mmio {
        if width == 32 && is_ee_timer_register(inst.mmio_address).is_some() {
            return format!(
                "[&]() -> uint32_t {{ const uint32_t _a = {addr}; return PS2Memory::isEeTimerRegister(_a) ? \
                 runtime->memory().eeTimerLoad(_a) : runtime->Load32(rdram, ctx, _a); }}()"
            );
        }
        if width == 32 && is_ee_dma_register(inst.mmio_address).is_some() {
            return format!(
                "[&]() -> uint32_t {{ const uint32_t _a = {addr}; return PS2Memory::isEeDmaRegister(_a) ? \
                 runtime->memory().eeDmaLoad(_a) : runtime->Load32(rdram, ctx, _a); }}()"
            );
        }
        if width == 64 && is_gs_status_register(inst.mmio_address) {
            return format!(
                "[&]() -> uint64_t {{ const uint32_t _a = {addr}; return PS2Memory::isGsStatusRegister(_a) ? \
                 runtime->memory().gsStatusLoad() : runtime->Load64(rdram, ctx, _a); }}()"
            );
        }
        return format!("runtime->Load{}(rdram, ctx, {})", width, addr);
    }
    format!("READ{}({})", width, addr)
}

/// `translateMemoryWrite`.
pub fn translate_memory_write(
    inst: &Instruction,
    hint: MemoryAccessHint,
    width: i32,
    addr: &str,
    value: &str,
) -> String {
    if hint.has_address {
        let resolved = hint.address;
        if width == 32 {
            if let Some(timer) = is_ee_timer_register(resolved) {
                return format!("(void)runtime->memory().eeTimerStore({}, {})", address_literal(timer), value);
            }
            if let Some(dma) = is_ee_dma_register(resolved) {
                return format!("(void)runtime->memory().eeDmaStore({}, {})", address_literal(dma), value);
            }
        }
        if width == 64 {
            if let Some(display) = is_gs_display_register(resolved) {
                return format!("runtime->memory().gsDisplayStore({}, {})", address_literal(display), value);
            }
        }
        if inst.is_mmio || ps2_is_special_address(resolved) {
            return format!("runtime->Store{}(rdram, ctx, {}, {})", width, address_literal(resolved), value);
        }
        return gen_fast_write(width, resolved, value);
    }

    if inst.is_mmio {
        if width == 32 && is_ee_timer_register(inst.mmio_address).is_some() {
            return format!(
                "do {{ const uint32_t _a = {addr}; const uint32_t _v = {value}; \
                 if (!PS2Memory::isEeTimerRegister(_a) || !runtime->memory().eeTimerStore(_a, _v)) \
                 runtime->Store32(rdram, ctx, _a, _v); }} while (0)"
            );
        }
        if width == 32 && is_ee_dma_register(inst.mmio_address).is_some() {
            return format!(
                "do {{ const uint32_t _a = {addr}; const uint32_t _v = {value}; \
                 if (!PS2Memory::isEeDmaRegister(_a) || !runtime->memory().eeDmaStore(_a, _v)) \
                 runtime->Store32(rdram, ctx, _a, _v); }} while (0)"
            );
        }
        if width == 64 && is_gs_display_register(inst.mmio_address).is_some() {
            return format!(
                "do {{ const uint32_t _a = {addr}; const uint64_t _v = {value}; \
                 if (PS2Memory::isGsDisplayRegister(_a)) runtime->memory().gsDisplayStore(_a, _v); \
                 else runtime->Store64(rdram, ctx, _a, _v); }} while (0)"
            );
        }
        return format!("runtime->Store{}(rdram, ctx, {}, {})", width, addr, value);
    }
    format!("WRITE{}({}, {})", width, addr, value)
}
