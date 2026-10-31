// GOW-Port: replacements of the game's calls to the PS2 system: IPU init, rand, CDVD, IOP module loading and the
// EE timer system. configureGowCdImage selects the disc source.

#include "gow_game.h"
#include "runtime/ee_scheduler.h"

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <system_error>

namespace gow_game
{
    namespace
    {
        // DVD layer 0 sectors (where layer 1 begins). Measured from the user's ISO.
        constexpr uint32_t kLayer1StartLbn = 2080544u;
    }

    // SCUS-97399 sceIpuInit uses SetD4_CHCR at 0x279588 and tables at
    // 0x2a1610/0x2a1660. The generic stub's hard-coded 0x126428 is a
    // FilteredCopyTile continuation in this ELF, so initialize MMIO directly.
    void gowIpuInit(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        auto &memory = runtime->memory();
        memory.write32(0x1000B400u, 1u);
        memory.write32(0x10002010u, 0x40000000u);
        memory.write32(0x10002000u, 0u);
        for (uint32_t offset : {0u, 16u, 32u, 48u, 64u, 64u, 64u, 64u})
            memory.write128(0x10007010u, runtime->Load128(rdram, ctx, 0x002A1610u + offset));
        memory.write32(0x10002000u, 0x50000000u);
        memory.write32(0x10002000u, 0x58000000u);
        for (uint32_t offset : {0u, 16u})
            memory.write128(0x10007010u, runtime->Load128(rdram, ctx, 0x002A1660u + offset));
        memory.write32(0x10002000u, 0x60000000u);
        memory.write32(0x10002000u, 0x90000000u);
        memory.write32(0x10002010u, 0x40000000u);
        memory.write32(0x10002000u, 0u);
        std::fprintf(stderr, "[gow-ipu] initialized using SCUS-97399 tables\n");
        returnToCaller(ctx, 0u);
    }

    // GOW-Port: rand@0x28C0C8 is stubbed in the recompiler config, and the generic stub returned the host's
    // 15-bit rand(). The game's newlib rand (checked against the ELF code and PCSX2's interpreter with
    // PS2X_EE_DIFF) keeps its seed at _impure_ptr(0x2A4294)+0x58 and returns 31 bits:
    // seed = seed * 0x41C64E6D + 0x3039; return seed & 0x7FFFFFFF.
    void gowRand(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        (void)runtime;
        const uint32_t reent = readGuest32(rdram, 0x002A4294u);
        const uint32_t seed = readGuest32(rdram, reent + 0x58u) * 0x41C64E6Du + 0x3039u;
        std::memcpy(rdram + ((reent + 0x58u) & 0x01FFFFFFu), &seed, sizeof(seed));
        setReturnU32(ctx, seed & 0x7FFFFFFFu);
        ctx->pc = GPR_U32(ctx, 31);
    }

    // int sceCdReadDvdDualInfo(int *on_dual, unsigned int *layer1_start)
    void gowCdReadDvdDualInfo(uint8_t *rdram, R5900Context *ctx, PS2Runtime *)
    {
        writeGuest32(rdram, GPR_U32(ctx, 4), 1u);
        writeGuest32(rdram, GPR_U32(ctx, 5), kLayer1StartLbn);
        SET_GPR_U32(ctx, 2, 1u);
        ctx->pc = GPR_U32(ctx, 31);
    }

    // sceSifLoadStartModuleBuffer(iopAddr, argLen, args, int *result) used by the game to load a
    // minimal IOP module embedded in the ELF ("ck01"). The runtime still can't run modules loaded
    // from IOP memory, so we answer like a retail console would: the module loads (valid id)
    // and ends with NO_RESIDENT_END (1), which is the value the game normally continues with.
    void gowLoadStartModuleBuffer(uint8_t *rdram, R5900Context *ctx, PS2Runtime *)
    {
        writeGuest32(rdram, GPR_U32(ctx, 7), 1u);
        SET_GPR_U32(ctx, 2, 0x40001000u);
        ctx->pc = GPR_U32(ctx, 31);
    }

    // GOW-Port: no-emulation step 6 (docs/dev/ARCHITECTURE.md). The game reads its data from the files extracted next to the ELF
    // (tools/iso_extract.py --layout: .disc_layout + .disc_meta), each at its original LSN, so smpd's raw sector reads
    // of GODOFWAR.TOC / PART*.PAK are host file reads (ps2xIOP DiscSource). GOW_ISO=<iso> still selects the original
    // image explicitly (for comparisons); no .iso is looked for otherwise.
    void configureGowCdImage()
    {
        namespace fs = std::filesystem;
        PS2Runtime::IoPaths paths = PS2Runtime::getIoPaths();
        if (!paths.cdImage.empty())
            return;
        std::error_code ec;
        fs::path image;
#ifdef _WIN32
        // Windows environment variables are UTF-16; getenv's ANSI bytes are not UTF-8.
        if (const wchar_t *env = _wgetenv(L"GOW_ISO"); env != nullptr && env[0] != L'\0')
            image = fs::path(env);
#else
        if (const char *env = std::getenv("GOW_ISO"); env != nullptr && env[0] != '\0')
            image = fs::u8path(env);
#endif
        if (image.empty())
        {
            const fs::path root = paths.cdRoot.empty() ? paths.elfDirectory : paths.cdRoot;
            if (!fs::is_regular_file(root / ".disc_layout", ec))
                std::fprintf(stderr, "[gow-cd] no .disc_layout in %s: extract the disc with tools/iso_extract.py --layout\n",
                             reinterpret_cast<const char *>(root.u8string().c_str()));
            return;
        }
        if (!fs::is_regular_file(image, ec))
        {
            std::fprintf(stderr, "[gow-cd] GOW_ISO is not a file; using the extracted files\n");
            return;
        }
        paths.cdImage = image;
        PS2Runtime::setIoPaths(paths);
        const auto imageUtf8 = image.u8string();
        std::fprintf(stderr, "[gow-cd] disc image (GOW_ISO): %s\n", reinterpret_cast<const char *>(imageUtf8.c_str()));
    }

    // ---- GOW-Port: no-emulation, EE timer 2 (docs/dev/ARCHITECTURE.md "EE timers") ----
    // libkernel's timer system (InitTimer + StartTimerSystemTime from _InitSys, SetTimerAlarm and the timer-2
    // interrupt handler cbTimerHandler) serves only DelayThread in this game. DelayThread is a native scheduler
    // delay and the timer system starts without timer 2, so no game code programs or reads timer 2.

    // 0x294A30 DelayThread(usec): the original waits on a semaphore signalled by a timer alarm. Same result
    // (0, or KE_CAN_NOT_WAIT 0x80008008 with interrupts disabled), the thread sleeps usec of guest time.
    void gowDelayThread(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        ctx->pc = GPR_U32(ctx, 31);
        if ((ctx->cop0_status & 0x10000u) == 0u)
        {
            SET_GPR_S32(ctx, 2, static_cast<int32_t>(0x80008008u));
            return;
        }
        const uint64_t usec = GPR_U32(ctx, 4);
        runtime->eeScheduler().delayCurrent(usec * EeScheduler::kEeClockHz / 1000000u); // resumes with v0 = 0
    }

    // 0x2998C0 InitTimer(prescale) and 0x299A20 StartTimerSystemTime(), called once each by _InitSys (0x2994A0):
    // success without programming timer 2 or installing its interrupt handler.
    void gowTimerSystemNoop(uint8_t *, R5900Context *ctx, PS2Runtime *)
    {
        SET_GPR_U64(ctx, 2, 0u);
        ctx->pc = GPR_U32(ctx, 31);
    }
    // ---- end EE timer 2 ----
}
