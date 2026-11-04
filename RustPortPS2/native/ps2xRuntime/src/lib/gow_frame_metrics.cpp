// GOW-Port: guest-frame interval distribution (GOW_FRAME_DIAG), see gow_frame_metrics.h.
#include "gow_frame_metrics.h"

#include <algorithm>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>

namespace gow_frame_metrics
{
    namespace
    {
        constexpr uint64_t kPeriodUs = 16667;      // one 60 Hz period
        constexpr uint64_t kReportUs = 5000000;    // one report every 5 s of wall time

        uint32_t framePc()
        {
            static const uint32_t pc = [] {
                const char *value = std::getenv("GOW_PERF_FRAME_PC");
                return value ? static_cast<uint32_t>(std::strtoul(value, nullptr, 0)) : 0u;
            }();
            return pc;
        }

        uint64_t nowUs()
        {
            return static_cast<uint64_t>(std::chrono::duration_cast<std::chrono::microseconds>(
                std::chrono::steady_clock::now().time_since_epoch()).count());
        }

        // Guest code runs one function at a time on the EE executor, so these need no lock.
        Window g_window;
        uint64_t g_lastFrameUs = 0;
        uint64_t g_windowStartUs = 0;
        uint64_t g_startUs = 0;
    }

    void Window::add(uint64_t intervalUs)
    {
        ++frames;
        maxUs = std::max(maxUs, intervalUs);
        if (intervalUs > kPeriodUs)
            ++over1;
        if (intervalUs > 2u * kPeriodUs)
            ++over2;
        const uint64_t bin = std::min<uint64_t>(intervalUs / kBinUs, kBins - 1u);
        ++bins[bin];
    }

    double Window::percentileMs(double fraction) const
    {
        if (frames == 0u)
            return 0.0;
        // The smallest bin whose cumulative count reaches ceil(fraction * frames).
        const double wanted = std::max(1.0, fraction * static_cast<double>(frames));
        uint64_t seen = 0;
        for (uint32_t bin = 0; bin < kBins; ++bin)
        {
            seen += bins[bin];
            if (static_cast<double>(seen) >= wanted - 1e-9)
                return static_cast<double>(bin + 1u) * kBinUs / 1000.0;
        }
        return static_cast<double>(kBins) * kBinUs / 1000.0;
    }

    void Window::reset()
    {
        std::memset(bins, 0, sizeof(bins));
        frames = 0;
        maxUs = 0;
        over1 = 0;
        over2 = 0;
    }

    // GOW-Port: GOW_GAP_DIAG=<ms>: every time two consecutive guest function entries are more than <ms> apart, the pc
    // entered before the gap (where the time went: guest code, a stub or a syscall) and the one after it. Finds what a
    // periodic hitch is doing without a profiler.
    double gapThresholdMs()
    {
        static const double ms = [] {
            const char *value = std::getenv("GOW_GAP_DIAG");
            return value ? std::atof(value) : 0.0;
        }();
        return ms;
    }

    bool enabled()
    {
        static const bool on = [] {
            const char *value = std::getenv("GOW_FRAME_DIAG");
            return (value && std::strcmp(value, "1") == 0 && framePc() != 0u) || gapThresholdMs() > 0.0;
        }();
        return on;
    }

    void onGuestEntry(uint32_t pc)
    {
        if (gapThresholdMs() > 0.0)
        {
            static uint64_t lastEntryUs = 0, startUs = 0;
            static uint32_t lastPc = 0;
            const uint64_t at = nowUs();
            if (startUs == 0u)
                startUs = at;
            if (lastEntryUs != 0u && static_cast<double>(at - lastEntryUs) / 1000.0 > gapThresholdMs())
                std::fprintf(stderr, "[gow-gap] %.1f ms after entering %08x, next %08x, t=%.3f\n",
                             static_cast<double>(at - lastEntryUs) / 1000.0, lastPc, pc, static_cast<double>(at - startUs) * 1e-6);
            lastEntryUs = at;
            lastPc = pc;
            if (framePc() == 0u)
                return;
        }
        if (pc != framePc())
            return;
        const uint64_t now = nowUs();
        if (g_lastFrameUs == 0u)
        {
            // The first frame only starts the clock: there is no interval before it.
            g_lastFrameUs = g_windowStartUs = g_startUs = now;
            return;
        }
        g_window.add(now - g_lastFrameUs);
        g_lastFrameUs = now;
        if (now - g_windowStartUs < kReportUs)
            return;
        const double seconds = static_cast<double>(now - g_windowStartUs) * 1e-6;
        std::fprintf(stderr,
                     "[gow-frames] t=%.2f window=%.2f frames=%llu fps=%.2f p50_ms=%.2f p95_ms=%.2f p99_ms=%.2f max_ms=%.2f "
                     "over16_7ms=%llu over33_3ms=%llu\n",
                     static_cast<double>(now - g_startUs) * 1e-6, seconds, static_cast<unsigned long long>(g_window.frames),
                     static_cast<double>(g_window.frames) / seconds, g_window.percentileMs(0.50), g_window.percentileMs(0.95),
                     g_window.percentileMs(0.99), static_cast<double>(g_window.maxUs) / 1000.0,
                     static_cast<unsigned long long>(g_window.over1), static_cast<unsigned long long>(g_window.over2));
        g_window.reset();
        g_windowStartUs = now;
    }
}
