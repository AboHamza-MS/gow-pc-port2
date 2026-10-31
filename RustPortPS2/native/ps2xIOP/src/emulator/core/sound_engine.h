#pragma once

// GOW-Port: native sound engine (replaces the register-level SPU2 model). The game's sound driver (989snd) reaches the
// sound hardware only through the LIBSD API; those calls are native (src/native/iop_native_overrides.cpp) and land here
// as typed voice / core state. No SPU2 registers and no IOP DMA controller exist: transfers are copies into the sample
// RAM. Mixing: 2 cores x 24 ADPCM voices (pitch with linear interpolation, ADSR envelope), dry mix L/R per voice,
// master volume, plus the core input stream (LIBSD block transfer, "ADMA") at the stream volume. 48 kHz stereo output.
// DSP algorithms as in PCSX2 (pcsx2/SPU2) and psx-spx (ADPCM / ADSR are the same as on the PS1). Reverb: parameters
// stored, not mixed yet. docs/dev/ARCHITECTURE.md (step 3), docs/dev/AUDIO-VIDEO.md.

#include <array>
#include <atomic>
#include <cstddef>
#include <cstdint>
#include <vector>

namespace ps2x::iop::detail
{
    class SoundEngine
    {
    public:
        static constexpr uint32_t RamHalfwords = 1u << 20u; // 2 MB sample RAM
        static constexpr uint32_t VoicesPerCore = 24u;
        static constexpr uint32_t CyclesPerSample = 768u;  // IOP 36.864 MHz / 48 kHz

        SoundEngine();
        void reset();

        // ---- LIBSD API (entry codes as libsd: (id << 8) | (voice << 1) | core; master volumes have bit 7 set) ----
        void setParam(uint32_t entry, uint16_t value);   // sceSdSetParam
        [[nodiscard]] uint16_t getParam(uint32_t entry) const; // sceSdGetParam
        void setSwitch(uint32_t entry, uint32_t mask);   // sceSdSetSwitch (KON, KOFF, VMIX*, NON, PMON)
        [[nodiscard]] uint32_t getSwitch(uint32_t entry) const; // sceSdGetSwitch
        void setAddr(uint32_t entry, uint32_t byteAddress); // sceSdSetAddr (SSA, LSA, NAX, ESA, EEA, ...)
        [[nodiscard]] uint32_t getAddr(uint32_t entry) const; // sceSdGetAddr (byte address)
        void setCoreAttr(uint32_t entry, uint32_t value);  // sceSdSetCoreAttr
        [[nodiscard]] uint32_t getCoreAttr(uint32_t entry) const;
        void initCore(uint32_t core);                      // sceSdInit state of one core

        // sample RAM transfers (sceSdVoiceTrans): byte address in sample RAM
        void writeSampleRam(uint32_t byteAddress, const uint8_t *source, size_t bytes);
        void readSampleRam(uint32_t byteAddress, uint8_t *destination, size_t bytes) const;
        void clearSampleRam(uint32_t byteAddress, size_t bytes);

        // core input stream (sceSdBlockTrans with SD_BLOCK_LOOP): the engine reads 512-byte L / R blocks from the ring in
        // IOP RAM at 48 kHz; onHalf is called (from advanceTo) each time a half of the ring has been consumed.
        struct StreamRing
        {
            const uint8_t *base = nullptr; // IOP RAM of the ring
            uint32_t size = 0;             // bytes
            uint32_t position = 0;         // next byte to read
            bool active = false;
            bool loop = false;             // false: the buffer plays once
        };
        void startStream(uint32_t core, const uint8_t *ring, uint32_t size, bool loop);
        void stopStream(uint32_t core);
        [[nodiscard]] uint32_t streamPosition(uint32_t core) const { return m_stream[core & 1u].position; }
        [[nodiscard]] bool takeStreamHalf(uint32_t core) noexcept; // a half was consumed since the last call

        // time: one stereo sample every CyclesPerSample IOP cycles
        void advanceTo(uint64_t iopCycle);

        // stereo samples produced so far (producer: IOP thread, consumer: host audio thread)
        [[nodiscard]] size_t pendingFrames() const;
        // Audio thread: fills stereo with up to maxFrames frames. With a latency target the engine keeps about that many
        // frames buffered by playing slightly slower or faster (pitch is preserved, see Stretcher); SIZE_MAX copies
        // the samples as they are.
        size_t drainOutput(int16_t *stereo, size_t maxFrames, size_t targetLatencyFrames = SIZE_MAX);

    private:
        // Output path. The guest produces samples on the EE thread at the pace of its own clock, which follows the
        // emulation, not the wall clock: a slow frame leaves the device without samples, a catch-up delivers a burst.
        // step() writes into a lock-free ring (the audio thread never waits for the EE thread) and the audio thread
        // reads it through a WSOLA time stretcher that holds a standing buffer and absorbs the difference between the
        // two clocks with a small change of tempo instead of silences, skipped blocks or a change of pitch.
        static constexpr size_t kRingFrames = 1u << 17;   // 2.7 s
        static constexpr int64_t kHop = 512;              // output frames per grain (10.7 ms)
        static constexpr int64_t kGrain = 2 * kHop;       // Hann grain, 50 % overlap
        static constexpr int64_t kSeek = 256;             // +-5.3 ms search for the best splice point
        struct Stretcher
        {
            bool waiting = true;       // building the first buffer, or rebuilding it after running dry
            bool primed = false;       // the last grain's continuation is the reference of the next one
            bool tailValid = false;    // the fading half of the last grain has not been played yet
            bool levelInit = false;
            int64_t lastStart = 0;     // input frame where the last grain began
            double nominal = 0.0;      // input position the tempo asks for (a grain starts within kSeek of it)
            double rate = 1.0;         // input frames consumed per output frame
            double levelAvg = 0.0;
            std::array<float, kHop * 2> tail{};
            std::vector<float> ready;  // finished frames (interleaved), not yet taken by the device
        };

        void pushFrame(int16_t left, int16_t right) noexcept;
        bool stretchGrain(uint64_t written);
        struct Voice
        {
            // parameters (as the driver sets them)
            uint16_t volL = 0, volR = 0, pitch = 0, adsr1 = 0, adsr2 = 0;
            uint32_t ssa = 0;                // start, halfwords
            // playback state
            bool active = false;
            uint32_t nax = 0;                // next ADPCM block to decode (halfwords)
            uint32_t playBlock = 0;          // block being played (NAX reads from it, like PCSX2's NextA)
            bool playStarted = false;
            uint32_t lsa = 0;                // loop start
            bool lsaFromDriver = false;      // the driver set LSA: block loop-start flags do not override it
            uint32_t pitchCounter = 0;       // 4.12
            int16_t decoded[28]{};
            uint32_t decodedIndex = 28;      // 28 = decode the next block first
            int32_t hist1 = 0, hist2 = 0;
            int16_t prev[2]{};
            uint8_t blockFlags = 0;
            enum class Phase : uint8_t { Off, Attack, Decay, Sustain, Release } phase = Phase::Off;
            int32_t envelope = 0;
            uint32_t envelopeCounter = 0;
        };
        struct Core
        {
            std::array<Voice, VoicesPerCore> voices{};
            uint32_t vmixL = 0, vmixR = 0, vmixEL = 0, vmixER = 0, non = 0, pmon = 0, endx = 0;
            uint16_t mmix = 0, attr = 0;
            uint16_t mvolL = 0, mvolR = 0, evolL = 0, evolR = 0, avolL = 0, avolR = 0, bvolL = 0, bvolR = 0;
            uint32_t esa = 0, eea = 0, irqa = 0, tsa = 0; // halfwords
        };

        void keyOn(Core &core, uint32_t mask);
        void keyOff(Core &core, uint32_t mask);
        void decodeBlock(Core &core, uint32_t index);
        int32_t stepVoice(Core &core, uint32_t index);
        static void stepEnvelope(Voice &v);
        static uint32_t naxOf(const Voice &v) noexcept;
        void streamSample(uint32_t core, int32_t &left, int32_t &right);
        void step();

        std::vector<uint16_t> m_ram;
        std::array<Core, 2> m_cores{};
        std::array<StreamRing, 2> m_stream{};
        std::array<bool, 2> m_streamHalf{};
        std::vector<int16_t> m_ring;                // interleaved stereo, kRingFrames frames
        std::atomic<uint64_t> m_writeCount{0};      // frames written (EE thread)
        std::atomic<uint64_t> m_readCount{0};       // oldest frame the audio thread may still read
        std::atomic<bool> m_flush{false};           // reset(): the audio thread discards what is buffered
        std::atomic<uint64_t> m_overflow{0};        // frames dropped because the ring was full
        std::atomic<uint64_t> m_clipped{0};         // output samples at full scale (GOW_AUDIO_STATS)
        Stretcher m_stretch;                        // audio thread only
        float m_limiterGain = 1.0f;                 // peak limiter of the final mix (EE thread)
        std::atomic<uint32_t> m_limiterMin{65536u}; // GOW_AUDIO_STATS: deepest limiter gain since the last report (x 65536)
        std::atomic<uint64_t> m_limiterDeep{0};     // GOW_AUDIO_STATS: frames with the limiter gain below 0.9
        uint64_t m_lastCycle = 0;
    };
}
