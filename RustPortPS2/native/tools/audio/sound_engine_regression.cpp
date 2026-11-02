// GOW-Port: regression cases of the SPU2 engine (ps2xIOP/src/emulator/core/sound_engine.cpp) through its public API, no game
// needed. Covers the exponential envelope (a keyed-off voice must fall to silence, a decay must reach the sustain level) and
// the master volume stage of the mix (a loud mix must keep its sign). Prints one PASS / FAIL line per case; the exit code is
// non-zero when any case fails.
// Build (MSVC, from a developer prompt), in this folder:
//   cl /std:c++20 /O2 /EHsc /D_CRT_SECURE_NO_WARNINGS /I ..\..\ps2xIOP\src sound_engine_regression.cpp
//      ..\..\ps2xIOP\src\emulator\core\sound_engine.cpp
// Note on "loud mix keeps its sign": before the 64-bit master volume stage the int32 product overflowed, which is undefined
// behavior. The case detects the wrap on the compilers that wrap (MSVC); it is a detector for that compiler, not a proof.
// The limiter flag of the engine is a function-local static, so GOW_AUDIO_LIMIT=0 is set before the first sample is made.
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <vector>

#include "emulator/core/sound_engine.h"

using ps2x::iop::detail::SoundEngine;

namespace
{
    // libsd ids (entry >> 8); entry = (id << 8) | (voice << 1) | core, master volumes also set bit 7
    enum : uint32_t
    {
        IdVolL = 0x00, IdVolR = 0x01, IdPitch = 0x02, IdAdsr1 = 0x03, IdAdsr2 = 0x04, IdEnvx = 0x05, IdMvolL = 0x09,
        IdMvolR = 0x0A, IdKon = 0x15, IdKoff = 0x16, IdVmixL = 0x18, IdVmixR = 0x1A, IdSsa = 0x20,
    };

    constexpr uint32_t voiceEntry(uint32_t id, uint32_t core, uint32_t voice) { return (id << 8) | (voice << 1) | core; }
    constexpr uint32_t masterEntry(uint32_t id, uint32_t core) { return (id << 8) | 0x80u | core; }

    int g_failures = 0;

    void report(const char *name, bool pass, const char *detail)
    {
        std::printf("%s %s%s%s\n", pass ? "PASS" : "FAIL", name, detail[0] ? ": " : "", detail);
        if (!pass)
            ++g_failures;
    }

    // A bench: the engine plus the cycle it was advanced to.
    struct Bench
    {
        SoundEngine engine;
        uint64_t cycle = 0;
        Bench()
        {
            int16_t discard[2];
            engine.drainOutput(discard, 0); // the first call drops what reset() flagged; do it before any sample exists
        }
        void run(uint64_t samples)
        {
            cycle += samples * SoundEngine::CyclesPerSample;
            engine.advanceTo(cycle);
        }
        uint16_t envx(uint32_t core, uint32_t voice) const { return engine.getParam(voiceEntry(IdEnvx, core, voice)); }
    };

    // voice 0 of core 0 on silent sample RAM (a zero ADPCM block repeats forever, so only the envelope matters)
    void setupSilentVoice(Bench &b, uint16_t adsr1, uint16_t adsr2)
    {
        SoundEngine &e = b.engine;
        e.setParam(voiceEntry(IdPitch, 0, 0), 0x1000);
        e.setParam(voiceEntry(IdAdsr1, 0, 0), adsr1);
        e.setParam(voiceEntry(IdAdsr2, 0, 0), adsr2);
        e.setSwitch(voiceEntry(IdVmixL, 0, 0), 1u);
        e.setSwitch(voiceEntry(IdVmixR, 0, 0), 1u);
        e.setParam(voiceEntry(IdVolL, 0, 0), 0x3FFF);
        e.setParam(voiceEntry(IdVolR, 0, 0), 0x3FFF);
        e.setAddr(voiceEntry(IdSsa, 0, 0), 0u);
    }

    void exponentialReleaseReachesSilence()
    {
        char detail[128];
        Bench b;
        setupSilentVoice(b, 0x0000, 0x20 | 11); // fastest attack, exponential release, shift 11
        b.engine.setSwitch(voiceEntry(IdKon, 0, 0), 1u);
        b.run(2000);
        const uint16_t peak = b.envx(0, 0);
        b.engine.setSwitch(voiceEntry(IdKoff, 0, 0), 1u);
        b.run(96000); // 2 s
        const uint16_t after = b.envx(0, 0);
        std::snprintf(detail, sizeof detail, "ENVX before KOFF 0x%04X (want 0x7FFF), 2 s after KOFF 0x%04X (want 0)", peak, after);
        report("exponential release reaches silence", peak == 0x7FFF && after == 0, detail);
    }

    void exponentialDecayEntersSustain()
    {
        char detail[128];
        Bench b;
        // decay rate 11, SL 0 (0x800); sustain holds (decrease, rate 127: one step per ~1 M samples)
        setupSilentVoice(b, (11u << 4u) | 0u, 0x4000u | (0x7Fu << 6u));
        b.engine.setSwitch(voiceEntry(IdKon, 0, 0), 1u);
        b.run(96000);
        const uint16_t env = b.envx(0, 0);
        std::snprintf(detail, sizeof detail, "ENVX after 2 s of decay 0x%04X (want 0x0700..0x0810)", env);
        report("exponential decay enters sustain", env >= 0x0700 && env <= 0x0810, detail);
    }

    void slowExponentialReleaseForEachShift()
    {
        for (uint32_t shift = 7; shift <= 15; ++shift)
        {
            char name[64], detail[128];
            Bench b;
            setupSilentVoice(b, 0x0000, static_cast<uint16_t>(0x20u | shift));
            b.engine.setSwitch(voiceEntry(IdKon, 0, 0), 1u);
            b.run(2000);
            const uint16_t peak = b.envx(0, 0);
            b.engine.setSwitch(voiceEntry(IdKoff, 0, 0), 1u);
            uint64_t samples = 96000ull;
            if (shift > 11)
                samples <<= (shift - 11);
            if (samples > 384000ull)
                samples = 384000ull; // 8 s of audio
            b.run(samples);
            const uint16_t after = b.envx(0, 0);
            std::snprintf(name, sizeof name, "exponential release reaches silence, shift %u", shift);
            std::snprintf(detail, sizeof detail, "ENVX before KOFF 0x%04X, after %llu samples 0x%04X (want 0)", peak,
                          static_cast<unsigned long long>(samples), after);
            report(name, peak == 0x7FFF && after == 0, detail);
        }
    }

    // 24 voices of core 1 at full scale through full master volume: the sum is about 690000 and the old int32 product with
    // the master volume (16383) wrapped to a negative number
    void loudMixKeepsItsSign()
    {
        char detail[160];
        Bench b;
        SoundEngine &e = b.engine;
        // one looping ADPCM block of constant samples: header 0x0700 (shift 0, filter 0, flags loop start | repeat | end),
        // then seven halfwords 0x7777
        uint8_t block[16];
        block[0] = 0x00;
        block[1] = 0x07;
        for (int i = 1; i < 8; ++i)
        {
            block[i * 2] = 0x77;
            block[i * 2 + 1] = 0x77;
        }
        e.writeSampleRam(0u, block, sizeof block);
        for (uint32_t voice = 0; voice < SoundEngine::VoicesPerCore; ++voice)
        {
            e.setParam(voiceEntry(IdPitch, 1, voice), 0x1000);
            // fast attack; the slowest decay with SL 15 hands over to sustain at once and the sustain (rate 0, increasing)
            // climbs back to full scale within a sample
            e.setParam(voiceEntry(IdAdsr1, 1, voice), 0x00FF);
            e.setParam(voiceEntry(IdAdsr2, 1, voice), 0x0000);
            e.setParam(voiceEntry(IdVolL, 1, voice), 0x3FFF);
            e.setParam(voiceEntry(IdVolR, 1, voice), 0x3FFF);
            e.setAddr(voiceEntry(IdSsa, 1, voice), 0u);
        }
        e.setSwitch(voiceEntry(IdVmixL, 1, 0), 0xFFFFFFu);
        e.setSwitch(voiceEntry(IdVmixR, 1, 0), 0xFFFFFFu);
        e.setParam(masterEntry(IdMvolL, 1), 0x3FFF);
        e.setParam(masterEntry(IdMvolR, 1), 0x3FFF);
        e.setSwitch(voiceEntry(IdKon, 1, 0), 0xFFFFFFu);
        b.run(4000);
        std::vector<int16_t> out(4000u * 2u);
        const size_t frames = e.drainOutput(out.data(), 4000, SIZE_MAX);
        size_t bad = 0;
        int firstBad = 0;
        for (size_t i = 100; i < frames; ++i)
            for (size_t side = 0; side < 2u; ++side)
                if (out[i * 2u + side] != 32767)
                {
                    if (bad == 0)
                        firstBad = out[i * 2u + side];
                    ++bad;
                }
        std::snprintf(detail, sizeof detail, "%zu frames drained, %zu samples after frame 100 differ from +32767 (first: %d)", frames, bad,
                      firstBad);
        report("loud mix keeps its sign", frames > 1000u && bad == 0u, detail);
    }
}

int main()
{
#ifdef _WIN32
    _putenv_s("GOW_AUDIO_LIMIT", "0");
#else
    setenv("GOW_AUDIO_LIMIT", "0", 1);
#endif
    exponentialReleaseReachesSilence();
    exponentialDecayEntersSustain();
    slowExponentialReleaseForEachShift();
    loudMixKeepsItsSign();
    std::printf("%s (%d failure%s)\n", g_failures == 0 ? "ALL PASS" : "FAILED", g_failures, g_failures == 1 ? "" : "s");
    return g_failures == 0 ? 0 : 1;
}
