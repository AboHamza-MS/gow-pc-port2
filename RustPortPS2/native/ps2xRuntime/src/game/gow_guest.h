// GOW-Port: helpers the God of War overrides share to touch the guest (EE) memory and to hand control back to the
// caller of an overridden function.
#pragma once

#include "ps2_runtime.h"
#include "ps2_runtime_macros.h"
#include <cstdint>
#include <cstring>

namespace gow_game
{
    inline void writeGuest32(uint8_t *rdram, uint32_t addr, uint32_t value)
    {
        if (addr == 0u) return;
        std::memcpy(rdram + (addr & 0x01FFFFFFu), &value, sizeof(value));
    }

    inline uint32_t readGuest32(const uint8_t *rdram, uint32_t addr)
    {
        uint32_t v;
        std::memcpy(&v, rdram + (addr & 0x01FFFFFFu), sizeof(v));
        return v;
    }

    // Pointer into the guest RAM for `size` bytes at `address`, or nullptr when the range is not inside the 32 MB.
    inline uint8_t *guestBuffer(uint8_t *rdram, uint32_t address, size_t size)
    {
        const uint32_t physical = address & 0x1fffffffu;
        return address && physical < 0x02000000u && size <= 0x02000000u - physical ? rdram + physical : nullptr;
    }

    // Finishes an overridden function: v0 = result, back to the caller.
    inline void returnToCaller(R5900Context *ctx, uint32_t result)
    {
        SET_GPR_S32(ctx, 2, static_cast<int32_t>(result));
        ctx->pc = GPR_U32(ctx, 31);
    }
}
