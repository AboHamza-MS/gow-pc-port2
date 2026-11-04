#ifndef PS2_EE_CHECKPOINT_CLOCK_H
#define PS2_EE_CHECKPOINT_CLOCK_H

#include <atomic>
#include <cstdint>

// GOW-Port: the EE guest clock and the state of the generated code's checkpoint fast path (idea from gow-native-pc
// runtime patch 04, a backport of Waffle565's EE checkpoint work). The generated code calls PS2Runtime::eeCheckpointDue
// at every loop back edge and every dispatched call, tens of millions of times a second; it used to be a call out of line
// into EeScheduler::checkpointDue, which advanced the EE timers and the IOP each time. EeScheduler drives this clock
// (full checkpoints, events, timers); PS2Runtime owns it so eeCheckpointDue reaches advanceQuiet inline, without a call.
// Everything is EE-thread state except the two flags, which other threads set to force a full checkpoint.
struct alignas(64) EeCheckpointClock
{
    uint64_t eeCycle = 0; // EE cycles since reset
    // Cycles that can pass before anything is due: the distance to the next EE timer step, IOP batch, scheduled event or
    // time-slice end, computed by the last full checkpoint. 0: the next checkpoint is a full one.
    uint64_t quietCycles = 0;
    // Cycles already in eeCycle but not yet handed to the EE timers and the IOP (EeScheduler::syncDeferredCycles).
    uint64_t deferredCycles = 0;
    std::atomic<bool> checkpointPending{false}; // an event, interrupt or reschedule is waiting
    std::atomic<bool> limitsChanged{true};      // a limit of quietCycles moved: recompute it

    // While the cycles stay below quietCycles and no other thread asked for a checkpoint, the full checkpoint would only
    // add cycles and return false: advance the clock here and defer the timer and IOP cycles. Returns false when the full
    // checkpoint has to run; the full checkpoints then happen at the same guest cycles, with the same results.
    [[nodiscard]] bool advanceQuiet(uint32_t cycles) noexcept
    {
        const uint64_t elapsed = cycles != 0u ? cycles : 1u;
        if (elapsed >= quietCycles || checkpointPending.load(std::memory_order_relaxed) ||
            limitsChanged.load(std::memory_order_relaxed))
            return false;
        quietCycles -= elapsed;
        eeCycle += elapsed;
        deferredCycles += elapsed;
        return true;
    }
};

#endif // PS2_EE_CHECKPOINT_CLOCK_H
