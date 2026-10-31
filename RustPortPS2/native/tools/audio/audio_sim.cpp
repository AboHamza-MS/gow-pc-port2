// GOW-Port: bench of the SPU2 output path (ps2xIOP/src/emulator/core/sound_engine.cpp), no game needed. Simulates a producer
// on the emulated clock (bursts per game frame, hitches, slower or faster than the wall clock) and the device callback, feeds
// a 440 Hz sine and reports what the player would hear for the previous policy (copied here) and the current one: ms of
// silence, gaps, clicks (second difference of the signal) and pitch. Optional argument: latency target in frames (6000).
// Build (MSVC, from a developer prompt), in this folder:
//   cl /std:c++20 /O2 /EHsc /D_ALLOW_KEYWORD_MACROS /Dprivate=public /D_CRT_SECURE_NO_WARNINGS /I ..\..\ps2xIOP\src
//      audio_sim.cpp ..\..\ps2xIOP\src\emulator\core\sound_engine.cpp
// (private=public lets the bench push frames straight into the ring.)
#include <algorithm>
#include <array>
#include <atomic>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <functional>
#include <memory>
#include <string>
#include <vector>


#include "emulator/core/sound_engine.h"


using ps2x::iop::detail::SoundEngine;

namespace
{
    constexpr double kPi = 3.14159265358979323846;
    constexpr double kFreq = 440.0, kAmp = 10000.0;

    // the previous policy (copy of the old drainOutput): playback up to 5 % faster above the latency, silence when dry
    struct OldPolicy
    {
        std::vector<int16_t> out;
        double phase = 0.0;
        size_t drain(int16_t *stereo, size_t maxFrames, size_t maxLatencyFrames)
        {
            size_t available = out.size() / 2u;
            double rate = 1.0;
            if (available > maxFrames + 4u * maxLatencyFrames)
            {
                const size_t dropped = available - maxFrames - maxLatencyFrames;
                out.erase(out.begin(), out.begin() + static_cast<std::ptrdiff_t>(dropped * 2u));
                available -= dropped;
                phase = 0.0;
            }
            const double over = (static_cast<double>(available) - static_cast<double>(maxFrames + maxLatencyFrames)) /
                                static_cast<double>(maxLatencyFrames);
            rate = 1.0 + std::clamp(over * 0.1, 0.0, 0.05);
            size_t frames = 0;
            double position = phase;
            if (rate == 1.0 && position == 0.0)
            {
                frames = std::min(maxFrames, available);
                std::copy_n(out.begin(), frames * 2u, stereo);
                position = static_cast<double>(frames);
            }
            else
            {
                for (; frames < maxFrames; ++frames, position += rate)
                {
                    const size_t index = static_cast<size_t>(position);
                    if (index + 1u >= available)
                        break;
                    const double fraction = position - static_cast<double>(index);
                    for (size_t side = 0; side < 2u; ++side)
                    {
                        const double a = out[index * 2u + side], b = out[(index + 1u) * 2u + side];
                        stereo[frames * 2u + side] = static_cast<int16_t>(std::lround(a + (b - a) * fraction));
                    }
                }
            }
            const size_t consumed = std::min(available, static_cast<size_t>(position));
            phase = position - static_cast<double>(consumed);
            out.erase(out.begin(), out.begin() + static_cast<std::ptrdiff_t>(consumed * 2u));
            return frames;
        }
    };

    struct Scenario
    {
        const char *name;
        double speed;                       // emulated seconds per wall second
        std::function<double(double)> gap;  // wall time t (s) -> fraction of production kept (hitch = 0)
        double seconds;
    };

    struct Result
    {
        size_t silentFrames = 0, starts = 0, clicks = 0;
        double hz = 0.0;
        size_t played = 0;
    };

    template <typename Push, typename Drain>
    Result simulate(const Scenario &sc, Push push, Drain drain)
    {
        Result r;
        double produceClock = 0.0;     // emulated seconds of audio produced
        uint64_t producedFrames = 0;
        double nextCallback = 0.02;
        std::vector<int16_t> block(1024 * 2);
        std::vector<int16_t> heard;
        const double dt = 1.0 / 60.0 / 4.0; // the EE runs in slices of a quarter frame
        for (double t = 0.0; t < sc.seconds; t += dt)
        {
            const double kept = sc.gap(t);
            produceClock += dt * sc.speed * kept;
            const uint64_t target = static_cast<uint64_t>(produceClock * 48000.0);
            while (producedFrames < target)
            {
                const double phase = 2.0 * kPi * kFreq * static_cast<double>(producedFrames) / 48000.0;
                const int16_t v = static_cast<int16_t>(std::lround(kAmp * std::sin(phase)));
                push(v, v);
                ++producedFrames;
            }
            while (nextCallback <= t)
            {
                nextCallback += 1024.0 / 48000.0;
                const size_t got = drain(block.data(), 1024);
                std::fill(block.begin() + static_cast<std::ptrdiff_t>(got * 2u), block.end(), int16_t{0});
                heard.insert(heard.end(), block.begin(), block.end());
            }
        }
        // skip the first 0.5 s (buffer fill), then look at the left channel
        const size_t first = 24000u * 2u;
        size_t crossings = 0, zeroRun = 0;
        double span = 0.0;
        for (size_t i = first + 4u; i < heard.size(); i += 2u)
        {
            const double a = heard[i - 4u], b = heard[i - 2u], c = heard[i];
            const bool silent = heard[i] == 0 && heard[i - 2u] == 0 && heard[i - 4u] == 0;
            if (silent)
            {
                ++r.silentFrames;
                if (++zeroRun == 1u)
                    ++r.starts;
                continue;
            }
            zeroRun = 0;
            ++r.played;
            if (std::fabs(c - 2.0 * b + a) > 400.0) // a sine of this amplitude never exceeds ~35
                ++r.clicks;
            if ((b < 0.0) != (c < 0.0))
                ++crossings;
        }
        (void)span;
        // pitch from the zero crossings of the played frames
        r.hz = r.played ? static_cast<double>(crossings) / 2.0 / (static_cast<double>(r.played) / 48000.0) : 0.0;
        return r;
    }
}

int main(int argc, char **argv)
{
    const size_t target = argc > 1 ? static_cast<size_t>(std::atoi(argv[1])) : 6000u;
    const Scenario scenarios[] = {
        {"steady 1.000x", 1.0, [](double) { return 1.0; }, 30.0},
        {"fast 1.037x (title screen)", 1.037, [](double) { return 1.0; }, 30.0},
        {"slow 0.90x (heavy fight)", 0.90, [](double) { return 1.0; }, 30.0},
        {"1.0x, hitch of 75 ms every 3 s", 1.0, [](double t) { return std::fmod(t, 3.0) < 0.075 ? 0.0 : 1.0; }, 30.0},
        {"1.0x, hitch of 40 ms every 1 s", 1.0, [](double t) { return std::fmod(t, 1.0) < 0.040 ? 0.0 : 1.0; }, 30.0},
        {"0.97x with hitches 75 ms every 2 s", 0.97, [](double t) { return std::fmod(t, 2.0) < 0.075 ? 0.0 : 1.0; }, 30.0},
        {"slow 0.50x (extreme)", 0.50, [](double) { return 1.0; }, 30.0},
    };
    std::printf("%-38s | %-44s | %s\n", "scenario", "old: silent(ms) gaps clicks Hz", "new: silent(ms) gaps clicks Hz");
    for (const Scenario &sc : scenarios)
    {
        OldPolicy old;
        const Result a = simulate(
            sc, [&](int16_t l, int16_t r) { old.out.push_back(l); old.out.push_back(r); },
            [&](int16_t *p, size_t n) { return old.drain(p, n, 4800u); });
        auto engine = std::make_unique<SoundEngine>();
        engine->m_flush.store(false);
        const Result b = simulate(
            sc, [&](int16_t l, int16_t r) { engine->pushFrame(l, r); },
            [&](int16_t *p, size_t n) { return engine->drainOutput(p, n, target); });
        std::printf("%-38s | %9.0f %5zu %6zu %7.1f           | %9.0f %5zu %6zu %7.1f\n", sc.name,
                    static_cast<double>(a.silentFrames) / 48.0, a.starts, a.clicks, a.hz,
                    static_cast<double>(b.silentFrames) / 48.0, b.starts, b.clicks, b.hz);
    }
    return 0;
}
