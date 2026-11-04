#pragma once
// GOW-Port: GOW_FRAME_DIAG=1 prints, every 5 s of wall time, how long the game's frames took: p50 / p95 / p99 / max and how
// many frames went over one and two 60 Hz periods. A frame is one entry to GOW_PERF_FRAME_PC (vid::Flip, the same entries
// that GOW_PERF_DIAG counts as guest_flip_hz). One steady-clock read per frame and fixed storage: no per-call timing scopes,
// so it can stay on during an FPS comparison. These are guest-frame intervals, not presentation latency.
// Idea from gow-native-pc runtime patch 29c (session frame metrics). Kept out of the precompiled header.
#include <cstdint>

namespace gow_frame_metrics
{
    // Reads GOW_FRAME_DIAG and GOW_PERF_FRAME_PC once. Callers keep the result in a function-local static.
    bool enabled();

    // Guest code entered `pc`. Only entries to GOW_PERF_FRAME_PC count; it runs on the thread that runs guest code.
    void onGuestEntry(uint32_t pc);

    // The interval histogram, exposed so a test can feed it fixed intervals without a clock.
    struct Window
    {
        static constexpr uint32_t kBinUs = 100;       // 0.1 ms per bin
        static constexpr uint32_t kBins = 1000;       // up to 100 ms; longer frames go to the last bin
        uint32_t bins[kBins] = {};
        uint64_t frames = 0;
        uint64_t maxUs = 0;
        uint64_t over1 = 0; // frames longer than one 60 Hz period (16.67 ms)
        uint64_t over2 = 0; // frames longer than two periods (33.33 ms)

        void add(uint64_t intervalUs);
        // Interval below which `fraction` (0..1) of the frames fall, in ms, at the bin's upper edge.
        double percentileMs(double fraction) const;
        void reset();
    };
}
