// GOW-Port: no-emulation step 5: the game's GS privileged-register code (display setup, FINISH interrupt) is native.

#include "gow_game.h"
#include "gow_guest_functions.h"
#include "runtime/gs/gs_display.h"

#include <cstdio>
#include <cstdlib>
#include <cstring>

namespace gow_game
{
    namespace
    {
        // GOW-Port: no-emulation step 5, GS privileged registers (docs/dev/ARCHITECTURE.md). The game's three functions that
        // touched them as hardware are native: the display setup becomes a GSDisplayState handed to the presentation,
        // the GS FINISH interrupt handshake uses the native GS status (runtime/gs/gs_display.h).
        void gowStore32(uint8_t *rdram, uint32_t addr, uint32_t value) { std::memcpy(rdram + addr, &value, sizeof(value)); }
        uint64_t gowLoad64(const uint8_t *rdram, uint32_t addr)
        {
            uint64_t value;
            std::memcpy(&value, rdram + addr, sizeof(value));
            return value;
        }

        // 0x183360 (vid display setup, called once per frame by 0x1837B8): computes the CRTC setup from the game's video
        // globals. Same values as the original's register stores, minus the stack garbage it left in unused bits
        // (and in circuit 1 when circuit 1 is off). GOW_GSREGS_CHECK=1 also runs the original and compares.
        bool gowComputeDisplay(const uint8_t *rdram, GSDisplayState &s)
        {
            const uint32_t flags = readGuest32(rdram, 0x0032F1B4u);     // bit 1: interlaced
            const uint32_t fieldMode = readGuest32(rdram, 0x00334F5Cu); // FFMD; non-zero = one circuit, no merge
            const uint32_t fbp = readGuest32(rdram, 0x00331074u) & 0x1FFu;
            const uint32_t psm = readGuest32(rdram, 0x00331070u) & 0x1Fu;
            const uint32_t video = readGuest32(rdram, 0x00331078u);     // 0x50: PAL timing
            const uint32_t width = readGuest32(rdram, 0x002FDD50u);
            const uint32_t height = readGuest32(rdram, 0x002FDD54u);
            if (width == 0u)
                return false; // the original traps (divide by zero): let it run
            s = {};
            s.smode2 = ((flags >> 1) & 1u) | ((fieldMode & 1u) << 1);
            // EN2 + MMOD; without field mode also EN1 with ALP 0x80: both circuits merged 50/50, circuit 1 one line
            // lower (the menu "Soften" option).
            s.pmode = 0x22u | (fieldMode == 0u ? 0x8001u : 0u);
            s.dispfb2 = static_cast<uint64_t>(fbp | (((width >> 6) & 0x3Fu) << 9) | (psm << 15));
            uint32_t dx, magh, dw, dh;
            if (video == 0x50u)
            {
                magh = 0x60Eu / width;
                dx = 0x138u;
                dw = (width * magh - 3u) & 0xFFFu;
                dh = (height - 1u) & 0x7FFu;
            }
            else
            {
                magh = (width + 0x9FFu) / width;
                dx = 0x27Cu;
                dw = (width * magh - 1u) & 0xFFFu;
                dh = ((fieldMode != 0u ? height * 2u : height) - 1u) & 0x7FFu;
            }
            const uint64_t dy = 0x32u;
            s.display2 = dx | (dy << 12) | (static_cast<uint64_t>((magh - 1u) & 0xFu) << 23) |
                         (static_cast<uint64_t>(dw) << 32) | (static_cast<uint64_t>(dh) << 44);
            if (s.pmode & 1u)
            {
                s.dispfb1 = s.dispfb2 | (1ull << 43); // DBY 1
                s.display1 = (s.display2 & ~(0x7FFull << 44)) | (static_cast<uint64_t>((dh - 1u) & 0x7FFu) << 44);
            }
            s.bgcolor = 0u;
            return true;
        }
    }

    void gowDisplaySetup(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        static const bool check = [] { const char *v = std::getenv("GOW_GSREGS_CHECK"); return v && v[0] == '1'; }();
        GSDisplayState state;
        if (!gowComputeDisplay(rdram, state))
        {
            sub_00183360_0x183360(rdram, ctx, runtime);
            return;
        }
        if (check)
        {
            const uint32_t ra = GPR_U32(ctx, 31);
            sub_00183360_0x183360(rdram, ctx, runtime); // writes through the register path
            const GSDisplayState hw = ps2_gs_display::display(runtime->memory());
            static int reports = 0;
            const uint64_t fb = 0x3FFFFF000FFFFFull, dm = 0x007FFFFF1FFFFFFFull; // DISPFB/DISPLAY defined fields
            const bool c1 = (state.pmode & 1u) != 0u;
            const bool same = (hw.pmode & 0xFFFFu) == state.pmode && (hw.smode2 & 3u) == state.smode2 &&
                              (hw.dispfb2 & fb) == state.dispfb2 && (hw.display2 & dm) == state.display2 &&
                              (hw.bgcolor & 0xFFFFFFu) == state.bgcolor &&
                              (!c1 || ((hw.dispfb1 & fb) == state.dispfb1 && (hw.display1 & dm) == state.display1));
            if (!same || reports < 3)
            {
                if (reports++ < 40)
                    std::fprintf(stderr, "[gsregs-check] %s pmode %llx/%llx smode2 %llx/%llx fb1 %llx/%llx d1 %llx/%llx fb2 %llx/%llx d2 %llx/%llx\n",
                                 same ? "same" : "DIFF", (unsigned long long)hw.pmode, (unsigned long long)state.pmode,
                                 (unsigned long long)hw.smode2, (unsigned long long)state.smode2, (unsigned long long)hw.dispfb1,
                                 (unsigned long long)state.dispfb1, (unsigned long long)hw.display1, (unsigned long long)state.display1,
                                 (unsigned long long)hw.dispfb2, (unsigned long long)state.dispfb2, (unsigned long long)hw.display2,
                                 (unsigned long long)state.display2);
            }
            SET_GPR_U32(ctx, 31, ra);
        }
        ps2_gs_display::setDisplay(runtime->memory(), state);
        SET_GPR_U64(ctx, 2, 0u); // the original returned stack garbage; its only caller (0x1837B8 <- 0x17A128) ignores it
        ctx->pc = GPR_U32(ctx, 31);
    }

    // 0x182DD8: GS interrupt (INTC 0) handler, installed by 0x182E88 with FINISH unmasked. On FINISH it sets the
    // game's "GS done" flag (0x29C7D0 = 2, read by vid::WaitForDMAComplete), optionally samples timer 0 for the GPU
    // time statistic, acknowledges FINISH and returns 0 with interrupts enabled.
    void gowGsFinishHandler(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        PS2Memory &memory = runtime->memory();
        if (ps2_gs_display::finishPending(memory))
        {
            gowStore32(rdram, 0x0029C7D0u, 2u);
            if (readGuest32(rdram, 0x0032F1B4u) & 4u)
            {
                const uint32_t now = runtime->memory().eeTimerLoad(0x10000000u); // native EE clock, timer 0
                if (now < readGuest32(rdram, 0x002C7908u))
                {
                    const uint64_t wraps = gowLoad64(rdram, 0x00303628u) + 0x10000u;
                    std::memcpy(rdram + 0x00303628u, &wraps, sizeof(wraps));
                }
                const uint32_t total = static_cast<uint32_t>(gowLoad64(rdram, 0x00303628u) + now);
                gowStore32(rdram, 0x002C7908u, now);
                gowStore32(rdram, 0x0029C66Cu, total - readGuest32(rdram, 0x00334F54u));
            }
            ps2_gs_display::ackFinish(memory);
        }
        ctx->cop0_status |= 0x10000u; // ei
        SET_GPR_U64(ctx, 2, 0u);
        ctx->pc = GPR_U32(ctx, 31);
    }

    // 0x182E88: installs that handler. Same kernel calls as the original (AddIntcHandler(0, 0x182DD8, 0),
    // EnableIntc(0), GsPutIMR(GsGetIMR() & ~FINISHMSK)); the CSR store clearing a stale FINISH is the native ack.
    void gowGsFinishInstall(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        const uint32_t ra = GPR_U32(ctx, 31);
        auto call = [&](uint32_t address)
        {
            SET_GPR_U32(ctx, 31, ra);
            ctx->pc = address;
            if (PS2Runtime::RecompiledFunction fn = runtime->lookupFunction(address))
                fn(rdram, ctx, runtime);
            if (ctx->pc != ra)
                std::fprintf(stderr, "[gsregs] call %08x did not return (pc=%08x)\n", address, ctx->pc);
        };
        SET_GPR_U64(ctx, 4, 0u);
        SET_GPR_U64(ctx, 5, 0x00182DD8u);
        SET_GPR_U64(ctx, 6, 0u);
        call(0x00293900u); // AddIntcHandler
        gowStore32(rdram, 0x0029C7C0u, GPR_U32(ctx, 2));
        ps2_gs_display::ackFinish(runtime->memory());
        SET_GPR_U64(ctx, 4, 0u);
        call(0x00294690u); // EnableIntc
        call(0x00279E30u); // GsGetIMR
        SET_GPR_U64(ctx, 4, GPR_U64(ctx, 2) & ~0x200ull);
        call(0x00279DF0u); // GsPutIMR
        SET_GPR_U32(ctx, 31, ra);
        ctx->pc = ra;
    }
}
