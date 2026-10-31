// GOW-Port: frame wait for God of War (SCUS-97399) without spinning.
//
// func_17A2A0 (the frame loop) waits for an interrupt to clear flag 0x29C7D0 like this:
//     while (*(u32 *)0x29C7D0 == 1)
//         for (v0 = 20000; v0; --v0) ;      // 0x17A328..0x17A35C
// In the recompiled code every turn of the inner loop goes through eeCheckpointDue: the EE thread occupied a
// whole core (EeScheduler::accountCycles was 31% of its samples) just to wait. On a laptop with the power
// saving profile that core throttles down and takes the sibling core (SMT) with the VU1 thread, which is the
// one limiting the frame.
//
// Here, when the EE re-enters that loop (on resuming after a checkpoint) and the flag is still 1, it
// sleeps in ~100 µs steps. The time slept is charged as EE cycles (294.912 MHz) via eeCheckpointDue, so
// VBlank, timers and the IOP still advance with real-time clock as before. As soon as there is a checkpoint
// (a pending interrupt, VBlank) it returns to the scheduler at 0x17A328 with $v0 = 1: the delay loop
// ends in one turn and reads the flag again (0x17A350). The scheduler delivers the interrupt sooner.
// The function table has one entry per resume point (0x17A328 and 0x17A32C inside the loop), so the
// wrapper is installed on all three. GOW_EE_WAIT=0 disables it.
#include "ps2_runtime.h"
#include "ps2_runtime_macros.h"

#include <algorithm>
#include <atomic>
#include <chrono>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <iostream>
#include <string>
#include <thread>

namespace
{
    constexpr uint32_t kFrameLoop = 0x17A2A0u;
    constexpr uint32_t kWaitStart = 0x17A328u;  // delay loop
    constexpr uint32_t kWaitEnd = 0x17A35Cu;    // delay slot of the branch back to it
    constexpr uint32_t kResume = 0x17A328u;     // registered resume point of the delay loop
    constexpr uint32_t kFlag = 0x29C7D0u;
    constexpr double kEeClockHz = 294912000.0;

    PS2Runtime::RecompiledFunction g_frameLoop = nullptr;
    std::atomic<uint64_t> g_sleptNs{0};
    bool g_diag = false;
}

// GOW-Port: GOW_WAIT_DIAG: timestamp (steady_clock ns) when the EE stopped waiting for the frame;
// ps2_memory.cpp measures against it when the VU1 thread's work starts and ends.
std::atomic<int64_t> g_gowFrameEndNs{0};

namespace
{

    // GOW_WAIT_DIAG=1: every 5 s, frames and average time per frame the EE spends waiting on the flag vs working.
    void diagnostics(bool waiting)
    {
        static bool wasWaiting = false;
        static auto mark = std::chrono::steady_clock::now(), window = mark;
        static double waitMs = 0.0, workMs = 0.0;
        static unsigned frames = 0;
        const auto now = std::chrono::steady_clock::now();
        const double ms = std::chrono::duration<double, std::milli>(now - mark).count();
        if (waiting == wasWaiting)
            return;
        (wasWaiting ? waitMs : workMs) += ms;
        if (!waiting)
            g_gowFrameEndNs.store(std::chrono::duration_cast<std::chrono::nanoseconds>(now.time_since_epoch()).count(),
                                   std::memory_order_relaxed);
        if (waiting)
            ++frames;
        wasWaiting = waiting;
        mark = now;
        if (now - window >= std::chrono::seconds(5) && frames)
        {
            std::fprintf(stderr, "[gow-wait] %u waits: EE waits %.2f ms and works %.2f ms per frame\n", frames,
                         waitMs / frames, workMs / frames);
            waitMs = workMs = 0.0;
            frames = 0;
            window = now;
        }
    }

    uint32_t flag(const uint8_t *rdram)
    {
        uint32_t v = 0u;
        std::memcpy(&v, rdram + kFlag, 4);
        return v;
    }

    void frameLoop(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        const uint32_t pc = ctx->pc;
        if (pc < kWaitStart || pc > kWaitEnd || ctx->in_delay_slot || flag(rdram) != 1u)
        {
            if (g_diag)
                diagnostics(false);
            g_frameLoop(rdram, ctx, runtime);
            return;
        }
        if (g_diag)
            diagnostics(true);
        // The EE is waiting on the flag: sleep instead of spinning, charging real time as cycles.
        auto last = std::chrono::steady_clock::now();
        const auto start = last;
        for (;;)
        {
            std::this_thread::sleep_for(std::chrono::microseconds(100));
            const auto now = std::chrono::steady_clock::now();
            const double seconds = std::chrono::duration<double>(now - last).count();
            last = now;
            const uint32_t cycles = static_cast<uint32_t>(std::clamp(seconds * kEeClockHz, 32.0, 4.0e6));
            const bool checkpoint = runtime->eeCheckpointDue(cycles);
            if (checkpoint || flag(rdram) != 1u || now - start > std::chrono::milliseconds(50))
                break;
        }
        g_sleptNs.fetch_add(static_cast<uint64_t>(std::chrono::duration_cast<std::chrono::nanoseconds>(
                                  std::chrono::steady_clock::now() - start).count()),
                              std::memory_order_relaxed);
        // Returns to the scheduler: delivers what is pending and resumes in the delay loop with $v0 = 1, which ends
        // in one turn and reads the flag again ($a2 is already 0x2A0000 from 0x17A300).
        SET_GPR_U32(ctx, 2, 1u);
        ctx->pc = kResume;
    }
}

uint64_t gowWaitSleptNs() { return g_sleptNs.load(std::memory_order_relaxed); }

void gowWaitInstall(PS2Runtime &runtime, const std::string &elfPath)
{
    if (std::filesystem::path(elfPath).filename() != "SCUS_973.99")
        return;
    const char *value = std::getenv("GOW_EE_WAIT");
    if (value && value[0] == '0')
        return;
    if (!runtime.hasFunction(kFrameLoop))
        return;
    const char *diag = std::getenv("GOW_WAIT_DIAG");
    g_diag = diag && diag[0] == '1';
    g_frameLoop = runtime.lookupFunction(kFrameLoop);
    for (const uint32_t pc : {kFrameLoop, 0x17A328u, 0x17A32Cu})
        if (runtime.hasFunction(pc) && runtime.lookupFunction(pc) == g_frameLoop)
            runtime.replaceFunction(pc, &frameLoop);
    std::cerr << "[gow-wait] the EE sleeps while waiting for the frame" << std::endl;
}
