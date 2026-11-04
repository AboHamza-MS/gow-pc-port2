// Standalone test of the batched sound mixing (ps2xIOP/src/emulator/core/sound_engine.cpp, SoundEngine::mixBatch): with
// GOW_SND_VERIFY=1 every batch is also mixed per sample from the same state and compared. This drives the LIBSD API like the
// game's sound driver does (sample RAM uploads, key on / off, pitch, envelopes, volumes, the movie input stream, reads of
// ENVX / NAX / ENDX) interleaved with advanceTo steps of every size, and fails on any "[snd-verify] ... differs" line.
// It needs no game files:
//
//   MSVC (x64 Native Tools prompt, from native/tests):
//     cl /nologo /std:c++20 /EHsc /W4 /D_CRT_SECURE_NO_WARNINGS /I ..\ps2xIOP\src\emulator\core sound_batch_test.cpp ..\ps2xIOP\src\emulator\core\sound_engine.cpp
//   GCC / Clang:
//     c++ -std=c++20 -O2 -I ../ps2xIOP/src/emulator/core sound_batch_test.cpp ../ps2xIOP/src/emulator/core/sound_engine.cpp -o sound_batch_test
//
// Not registered in CMake on purpose (a CMake edit recompiles the whole runtime); run it by hand after touching the mixer.
#include "sound_engine.h"

#include <algorithm>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <random>
#include <string>
#include <vector>

using ps2x::iop::detail::SoundEngine;

namespace
{
    uint32_t entry(uint32_t id, uint32_t voice, uint32_t core) { return (id << 8) | (voice << 1) | core; }

    // ADPCM blocks: 16 bytes each (header, flags, 14 data bytes); the last block of a sample has the end (+ repeat) flag.
    std::vector<uint8_t> makeSample(std::mt19937 &rng, uint32_t blocks, bool loop)
    {
        std::vector<uint8_t> out(blocks * 16u);
        for (uint32_t b = 0; b < blocks; ++b)
        {
            out[b * 16u] = static_cast<uint8_t>((rng() % 5u) << 4 | (rng() % 13u)); // filter, shift
            uint8_t flags = 0;
            if (b == 0)
                flags |= 4u; // loop start
            if (b + 1u == blocks)
                flags |= loop ? 3u : 1u; // end (+ repeat)
            out[b * 16u + 1u] = flags;
            for (uint32_t i = 2; i < 16u; ++i)
                out[b * 16u + i] = static_cast<uint8_t>(rng());
        }
        return out;
    }

    // GOW-Port: planning 1.13 / patch 24 regression. All 48 voices at full scale through master volumes of 0x3FFF sum to
    // ~690000 per core; the int32 product with the master volume overflowed and the output flipped sign. The mix must stay
    // positive (limited or clipped), never wrap negative.
    int masterVolumeOverflow()
    {
        static SoundEngine engine;
        for (uint32_t core = 0; core < 2u; ++core)
            engine.initCore(core);
        // A looping sample at a constant +28672: filter 0, shift 0, every nibble 7.
        std::vector<uint8_t> loud(16u * 8u, 0x77u);
        for (uint32_t b = 0; b < 8u; ++b)
        {
            loud[b * 16u] = 0x00u;
            loud[b * 16u + 1u] = b == 0 ? 4u : b == 7u ? 3u : 0u;
        }
        engine.writeSampleRam(0x5000u, loud.data(), loud.size());
        for (uint32_t core = 0; core < 2u; ++core)
        {
            engine.setParam(entry(0x09, 0, core) | 0x80u, 0x3FFFu); // MVOLL
            engine.setParam(entry(0x0A, 0, core) | 0x80u, 0x3FFFu); // MVOLR
            engine.setParam(entry(0x08, 0, core), 0x0FF0u);         // MMIX
            engine.setSwitch(entry(0x18, 0, core), 0xFFFFFFu);      // VMIXL
            engine.setSwitch(entry(0x1A, 0, core), 0xFFFFFFu);      // VMIXR
            for (uint32_t voice = 0; voice < 24u; ++voice)
            {
                engine.setAddr(entry(0x20, voice, core), 0x5000u);
                engine.setParam(entry(0x02, voice, core), 0x1000u); // pitch 1.0
                engine.setParam(entry(0x00, voice, core), 0x3FFFu);
                engine.setParam(entry(0x01, voice, core), 0x3FFFu);
                engine.setParam(entry(0x03, voice, core), 0x000Fu); // fastest attack, sustain level max
                engine.setParam(entry(0x04, voice, core), 0x0000u); // sustain rising, fastest: stays at full scale
            }
            engine.setSwitch(entry(0x15, 0, core), 0xFFFFFFu); // KON all
        }
        std::vector<int16_t> sink(4096 * 2);
        int negative = 0;
        int16_t peak = 0;
        uint64_t frames = 0;
        for (uint64_t cycle = 768u * 64u; cycle <= 768ull * 48000ull / 2ull; cycle += 768u * 64u)
        {
            engine.advanceTo(cycle);
            const size_t n = engine.drainOutput(sink.data(), sink.size() / 2u, SIZE_MAX);
            // Skip the attack (first 20 ms).
            for (size_t i = 0; i < n * 2u; ++i)
                if (frames + i / 2u > 960u)
                {
                    negative += sink[i] < 0;
                    peak = std::max(peak, sink[i]);
                }
            frames += n;
        }
        if (frames == 0u || negative != 0 || peak < 16000)
        {
            std::printf("FAIL: master volume overflow: %d negative samples, peak %d, %llu frames\n", negative, peak,
                        static_cast<unsigned long long>(frames));
            return 1;
        }
        std::printf("sound_batch_test: 48 full-scale voices stay positive (peak %d)\n", peak);
        return 0;
    }
}

int main()
{
#ifdef _WIN32
    _putenv("GOW_SND_VERIFY=1");
#else
    setenv("GOW_SND_VERIFY", "1", 1);
#endif
    const int overflowFailures = masterVolumeOverflow();
    // The mixer reports a mismatch on stderr: send it to a file and read it at the end.
    const char *logPath = "sound_batch_test.log";
    if (!std::freopen(logPath, "w", stderr))
    {
        std::printf("FAIL: cannot redirect stderr\n");
        return 1;
    }

    static SoundEngine engine;
    std::mt19937 rng(12345);
    for (uint32_t core = 0; core < 2u; ++core)
        engine.initCore(core);

    // Samples in sample RAM.
    std::vector<uint32_t> starts;
    uint32_t address = 0x5000u;
    for (int s = 0; s < 12; ++s)
    {
        const auto data = makeSample(rng, 4u + rng() % 60u, (s % 3) != 0);
        engine.writeSampleRam(address, data.data(), data.size());
        starts.push_back(address);
        address += static_cast<uint32_t>(data.size() + 32u);
    }
    // Master volumes, mixes.
    for (uint32_t core = 0; core < 2u; ++core)
    {
        engine.setParam(entry(0x09, 0, core) | 0x80u, 0x3FFFu);
        engine.setParam(entry(0x0A, 0, core) | 0x80u, 0x3000u);
        engine.setSwitch(entry(0x18, 0, core), 0xFFFFFFu);
        engine.setSwitch(entry(0x1A, 0, core), 0x00FFFFu);
        engine.setParam(entry(0x08, 0, core), 0x0FF0u | 0x00C0u); // MMIX with the input-stream bits
        engine.setParam(entry(0x0F, 0, core), 0x7FFEu);           // BVOL
        engine.setParam(entry(0x10, 0, core), 0x6000u);
    }
    // An input stream (a movie's audio): 4 KB ring of L/R blocks in "IOP RAM".
    static std::vector<uint8_t> streamRing(16384);
    for (auto &b : streamRing)
        b = static_cast<uint8_t>(rng());

    uint64_t cycle = 0;
    uint64_t drained = 0;
    std::vector<int16_t> sink(4096 * 2);
    const uint64_t endCycle = 768ull * 48000ull * 130ull; // 130 s of audio: the verify mode reports every 60 s
    while (cycle < endCycle)
    {
        const uint32_t action = rng() % 100u;
        const uint32_t core = rng() & 1u;
        const uint32_t voice = rng() % 24u;
        if (action < 8u)
        {
            const uint32_t sample = starts[rng() % starts.size()];
            engine.setAddr(entry(0x20, voice, core), sample); // SSA
            engine.setParam(entry(0x02, voice, core), static_cast<uint16_t>(0x0400u + rng() % 0x3000u));
            engine.setParam(entry(0x00, voice, core), static_cast<uint16_t>(rng() % 0x3FFFu));
            engine.setParam(entry(0x01, voice, core), static_cast<uint16_t>(rng() % 0x3FFFu));
            engine.setParam(entry(0x03, voice, core), static_cast<uint16_t>(rng()));
            engine.setParam(entry(0x04, voice, core), static_cast<uint16_t>(rng()));
            engine.setSwitch(entry(0x15, 0, core), 1u << voice); // KON
        }
        else if (action < 12u)
            engine.setSwitch(entry(0x16, 0, core), 1u << voice); // KOFF
        else if (action < 16u)
            engine.setParam(entry(0x02, voice, core), static_cast<uint16_t>(rng() % 0x3FFFu));
        else if (action < 20u)
            (void)engine.getParam(entry(0x05, voice, core)); // ENVX
        else if (action < 22u)
            (void)engine.getAddr(entry(0x22, voice, core)); // NAX
        else if (action < 24u)
            (void)engine.getSwitch(entry(0x17, 0, core)); // ENDX
        else if (action < 25u)
            engine.startStream(core, streamRing.data(), static_cast<uint32_t>(streamRing.size()), (rng() & 1u) != 0u);
        else if (action < 26u)
            engine.stopStream(core);
        else if (action < 28u)
            (void)engine.takeStreamHalf(core);
        else if (action < 29u)
            engine.setSwitch(entry(0x18, 0, core), rng() & 0xFFFFFFu); // VMIXL
        // Time steps of every size: shorter than a batch, around it and many batches at once.
        const uint32_t pick = rng() % 4u;
        const uint64_t step = pick == 0 ? rng() % 768u : pick == 1 ? rng() % (768u * 80u) : pick == 2 ? 768u * 64u : rng() % (768u * 4000u);
        cycle += step;
        engine.advanceTo(cycle);
        drained += engine.drainOutput(sink.data(), sink.size() / 2u, SIZE_MAX);
    }
    std::fflush(stderr);
    std::fclose(stderr);

    int failures = overflowFailures;
    if (std::FILE *log = std::fopen(logPath, "r"))
    {
        char line[512];
        unsigned checkedLines = 0;
        while (std::fgets(line, sizeof(line), log))
        {
            if (std::strstr(line, "[snd-verify]") && std::strstr(line, "differs"))
            {
                if (failures++ < 5)
                    std::printf("FAIL: %s", line);
            }
            if (std::strstr(line, "frames checked"))
                ++checkedLines;
        }
        std::fclose(log);
        if (checkedLines == 0u)
        {
            std::printf("FAIL: the verify mode never reported (was GOW_SND_VERIFY read?)\n");
            ++failures;
        }
    }
    if (drained == 0u)
    {
        std::printf("FAIL: no audio frames came out\n");
        ++failures;
    }
    if (failures == 0)
        std::printf("sound_batch_test: batched mixing matches per-sample mixing (%llu frames drained)\n",
                    static_cast<unsigned long long>(drained));
    return failures == 0 ? 0 : 1;
}
