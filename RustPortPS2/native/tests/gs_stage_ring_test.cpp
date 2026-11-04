// Standalone test of the GS stage queue (ps2xRuntime/src/lib/gs/gs_stage.cpp): one producer thread (the GS worker's role)
// pushes packets of every size (tiny, sizes that are not a multiple of 16, near the 1 MB inline limit, above it, and enough
// bytes to wrap the ring many times); the stage checks that every packet arrives once, in order, with its bytes. A third
// thread (the EE's role) calls waitIdle at random moments: when it returns, everything pushed before the call must have been
// processed, afterBatch included. Run with no argument for the ring (default) and with `queue` for the former mutex queue;
// it prints the time each took. No game files:
//
//   MSVC (x64 Native Tools prompt, from native/tests):
//     cl /nologo /std:c++20 /EHsc /W4 /O2 /I ..\ps2xRuntime\include gs_stage_ring_test.cpp ..\ps2xRuntime\src\lib\gs\gs_stage.cpp
//   GCC / Clang:
//     c++ -std=c++20 -O2 -pthread -I ../ps2xRuntime/include gs_stage_ring_test.cpp ../ps2xRuntime/src/lib/gs/gs_stage.cpp -o gs_stage_ring_test
//
// Not registered in CMake on purpose (a CMake edit recompiles the whole runtime); run it by hand after touching the stage.
#include "runtime/gs/gs_stage.h"

#include <atomic>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <random>
#include <string>
#include <thread>
#include <vector>

namespace
{
    constexpr uint32_t kPackets = 200000;

    std::atomic<uint64_t> g_processed{0};   // packets the stage finished
    std::atomic<uint64_t> g_batches{0};     // afterBatch calls
    std::atomic<uint64_t> g_pushed{0};      // packets the producer finished pushing
    std::atomic<bool> g_failed{false};

    uint32_t sizeOf(uint32_t index)
    {
        // Mostly small packets (like XGKICK output), some medium, a few near and above the 1 MB inline limit.
        std::mt19937 rng(index * 2654435761u + 7u);
        const uint32_t pick = rng() % 1000u;
        if (pick < 900u)
            return 1u + rng() % 4000u;
        if (pick < 995u)
            return 4096u + rng() % 200000u;
        if (pick < 998u)
            return (1u << 20) - 8u + rng() % 16u; // around the inline limit
        return (1u << 20) + 1u + rng() % (1u << 20); // heap path
    }

    uint8_t byteOf(uint32_t index, uint32_t offset) { return static_cast<uint8_t>(index * 31u + offset * 7u + (offset >> 8)); }

    void fail(const char *what, uint32_t index)
    {
        if (!g_failed.exchange(true))
            std::printf("FAIL: %s (packet %u)\n", what, index);
    }
}

int main(int argc, char **argv)
{
    const bool queue = argc > 1 && std::strcmp(argv[1], "queue") == 0;
#ifdef _WIN32
    _putenv(queue ? "GOW_GS_STAGE_RING=0" : "GOW_GS_STAGE_RING=1");
#else
    setenv("GOW_GS_STAGE_RING", queue ? "0" : "1", 1);
#endif

    uint32_t expected = 0; // stage thread only
    ps2_gs_stage::start(
        [&expected](GifPathId path, const uint8_t *data, uint32_t size)
        {
            const uint32_t index = expected++;
            if (size != sizeOf(index))
                return fail("wrong size or order", index);
            if (path != static_cast<GifPathId>(1u + index % 3u))
                return fail("wrong path", index);
            // Check the first and last 64 bytes and a sample in between (whole check of big packets is slow).
            for (uint32_t i = 0; i < size; i = (i < 64u || i + 64u >= size) ? i + 1u : i + 977u)
                if (data[i] != byteOf(index, i))
                    return fail("wrong bytes", index);
            g_processed.fetch_add(1, std::memory_order_release);
        },
        [] { g_batches.fetch_add(1, std::memory_order_relaxed); });

    const auto t0 = std::chrono::steady_clock::now();
    std::thread producer([] {
        std::vector<uint8_t> buffer;
        for (uint32_t index = 0; index < kPackets; ++index)
        {
            const uint32_t size = sizeOf(index);
            buffer.resize(size);
            for (uint32_t i = 0; i < size; ++i)
                buffer[i] = byteOf(index, i);
            ps2_gs_stage::push(static_cast<GifPathId>(1u + index % 3u), buffer.data(), size);
            g_pushed.fetch_add(1, std::memory_order_release);
        }
    });
    std::thread ee([] {
        std::mt19937 rng(99);
        unsigned waits = 0;
        while (g_pushed.load(std::memory_order_acquire) < kPackets && !g_failed.load())
        {
            std::this_thread::sleep_for(std::chrono::microseconds(rng() % 2000u));
            const uint64_t before = g_pushed.load(std::memory_order_acquire);
            ps2_gs_stage::waitIdle();
            ++waits;
            if (g_processed.load(std::memory_order_acquire) < before)
                fail("waitIdle returned before the packets pushed earlier were processed", static_cast<uint32_t>(before));
        }
        std::printf("  %u waitIdle calls from the EE thread\n", waits);
    });
    producer.join();
    ee.join();
    ps2_gs_stage::waitIdle();
    const double seconds = std::chrono::duration<double>(std::chrono::steady_clock::now() - t0).count();
    ps2_gs_stage::stop();

    if (g_processed.load() != kPackets)
        fail("not every packet was processed", static_cast<uint32_t>(g_processed.load()));
    if (g_batches.load() == 0u)
        fail("afterBatch never ran", 0);
    if (g_failed.load())
        return 1;
    std::printf("gs_stage_ring_test (%s): %u packets in order, %llu batches, %.3f s\n", queue ? "queue" : "ring", kPackets,
                static_cast<unsigned long long>(g_batches.load()), seconds);
    return 0;
}
