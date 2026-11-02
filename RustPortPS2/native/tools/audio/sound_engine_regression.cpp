// GOW-Port: regression cases of the SPU2 engine (ps2xIOP/src/emulator/core/sound_engine.cpp) through its public API, no game
// needed. Covers the exponential envelope: a keyed-off voice must fall to silence and a decay must reach the sustain level.
// Prints one PASS / FAIL line per case; the exit code is non-zero when any case fails.
// Build (MSVC, from a developer prompt), in this folder:
//   cl /std:c++20 /O2 /EHsc /D_CRT_SECURE_NO_WARNINGS /I ..\..\ps2xIOP\src sound_engine_regression.cpp
//      ..\..\ps2xIOP\src\emulator\core\sound_engine.cpp
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
}

int main()
{
    exponentialReleaseReachesSilence();
    exponentialDecayEntersSustain();
    slowExponentialReleaseForEachShift();
    std::printf("%s (%d failure%s)\n", g_failures == 0 ? "ALL PASS" : "FAILED", g_failures, g_failures == 1 ? "" : "s");
    return g_failures == 0 ? 0 : 1;
}
