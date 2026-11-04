// Standalone test of the guest-frame interval histogram (ps2xRuntime/src/lib/gow_frame_metrics.cpp, GOW_FRAME_DIAG). It needs
// no raylib, GLFW or game files:
//
//   MSVC (from a "x64 Native Tools" prompt):  cl /nologo /std:c++17 /EHsc /W4 /D_CRT_SECURE_NO_WARNINGS /I ..\ps2xRuntime\src\lib frame_metrics_test.cpp ..\ps2xRuntime\src\lib\gow_frame_metrics.cpp
//   GCC / Clang:                              c++ -std=c++17 -Wall -Wextra -I ../ps2xRuntime/src/lib frame_metrics_test.cpp ../ps2xRuntime/src/lib/gow_frame_metrics.cpp -o frame_metrics_test
//
// It is not registered in CMake on purpose (a CMake edit recompiles the whole runtime); run it by hand after touching the file.
#include "gow_frame_metrics.h"

#include <cmath>
#include <cstdio>

namespace
{
    int g_failures = 0;

    void expectNear(const char *what, double got, double want)
    {
        if (std::fabs(got - want) > 1e-9)
        {
            std::printf("FAIL %s: got %.4f, want %.4f\n", what, got, want);
            ++g_failures;
        }
    }

    void expectEq(const char *what, unsigned long long got, unsigned long long want)
    {
        if (got != want)
        {
            std::printf("FAIL %s: got %llu, want %llu\n", what, got, want);
            ++g_failures;
        }
    }
}

int main()
{
    using gow_frame_metrics::Window;
    static Window w;

    // Empty window: no frames, percentiles 0.
    expectNear("empty p50", w.percentileMs(0.5), 0.0);

    // 100 frames: 90 at 16.6 ms, 9 at 33.2 ms, 1 at 120 ms (past the last bin).
    for (int i = 0; i < 90; ++i)
        w.add(16600);
    for (int i = 0; i < 9; ++i)
        w.add(33200);
    w.add(120000);
    expectEq("frames", w.frames, 100);
    expectEq("over one period", w.over1, 10);   // 33.2 and 120 ms
    expectEq("over two periods", w.over2, 1);   // only 120 ms
    expectEq("max us", w.maxUs, 120000);
    // Bins are 0.1 ms wide and percentiles report the bin's upper edge.
    expectNear("p50", w.percentileMs(0.50), 16.7);
    expectNear("p90", w.percentileMs(0.90), 16.7);
    expectNear("p95", w.percentileMs(0.95), 33.3);
    expectNear("p99", w.percentileMs(0.99), 33.3);
    expectNear("p100 (clamped to the last bin)", w.percentileMs(1.0), 100.0);

    // A frame exactly one period long is not "over" it.
    w.reset();
    w.add(16667);
    expectEq("exactly one period", w.over1, 0);
    expectEq("frames after reset", w.frames, 1);

    if (g_failures == 0)
        std::printf("frame_metrics_test: all checks passed\n");
    return g_failures == 0 ? 0 : 1;
}
