// Standalone test of the pure aspect ratio logic (ps2xRuntime/src/lib/gow_aspect.h): what PC_ASPECT and PC_WIDESCREEN accept. It
// needs no raylib, GLFW or game files:
//
//   MSVC (from a "x64 Native Tools" prompt):  cl /nologo /std:c++17 /EHsc /W4 /I ..\ps2xRuntime\src\lib gow_aspect_test.cpp
//   GCC / Clang:                              c++ -std=c++17 -Wall -Wextra -I ../ps2xRuntime/src/lib gow_aspect_test.cpp -o gow_aspect_test
//
// It is not registered in CMake on purpose (a CMake edit recompiles the whole runtime); run it by hand after touching the header.
//
// The aspects the launcher offers (gow-launcher GraphicsPage.tsx and launch.rs) must all reach the Hor+ patch, 32:9 included.
#include "gow_aspect.h"

#include <cmath>
#include <cstdio>
#include <initializer_list>

#if defined(_MSC_VER)
#pragma warning(disable : 4127) // the constants of gow_aspect.h are compared on purpose
#endif

static int g_failures = 0;
#define CHECK(condition)                                                                                  \
    do                                                                                                    \
    {                                                                                                     \
        if (!(condition))                                                                                 \
        {                                                                                                 \
            ++g_failures;                                                                                 \
            std::printf("FAIL line %d: %s\n", __LINE__, #condition);                                      \
        }                                                                                                 \
    } while (0)

static bool near(float a, float b) { return std::fabs(a - b) < 1e-4f; }

using namespace gowaspect;

int main()
{
    // Ratios and decimals.
    CHECK(near(parseAspect("32:9"), 32.0f / 9.0f));
    CHECK(near(parseAspect("21:9"), 21.0f / 9.0f));
    CHECK(near(parseAspect("16:10"), 1.6f));
    CHECK(near(parseAspect("3.5556"), 3.5556f));
    CHECK(near(parseAspect("1.7778"), 1.7778f));
    // Not an aspect: the caller falls back (stretch, auto, the widescreen aspect or 4:3).
    CHECK(parseAspect("stretch") == 0.0f);
    CHECK(parseAspect("auto") == 0.0f);
    CHECK(parseAspect("") == 0.0f);
    CHECK(parseAspect("0:9") == 0.0f);
    CHECK(parseAspect("32:0") == 0.0f);
    CHECK(parseAspect("9") == 0.0f);   // a decimal outside 0.5..4.0
    CHECK(parseAspect("0.1") == 0.0f);

    // PC_WIDESCREEN: every aspect of the launcher's list reaches the patch except 4:3, which has nothing to widen.
    CHECK(widescreenAspect(nullptr) == 0.0f);
    CHECK(widescreenAspect("") == 0.0f);
    CHECK(widescreenAspect("4:3") == 0.0f);
    CHECK(widescreenAspect("1.3333") == 0.0f);
    CHECK(near(widescreenAspect("16:9"), 16.0f / 9.0f));
    CHECK(near(widescreenAspect("16:10"), 1.6f));
    CHECK(near(widescreenAspect("21:9"), 21.0f / 9.0f));
    CHECK(near(widescreenAspect("32:9"), 32.0f / 9.0f));
    // The limit: 4.0 itself is accepted, 48:9 (5.33) and anything wider is off.
    CHECK(near(widescreenAspect("4"), 4.0f));
    CHECK(near(widescreenAspect("4:1"), 4.0f));
    CHECK(widescreenAspect("48:9") == 0.0f);
    CHECK(widescreenAspect("stretch") == 0.0f);
    // 32:9 stays clear of the cap, so a change of the cap cannot silently drop it.
    CHECK(32.0f / 9.0f < kMaxAspect);

    // F * A: exactly 1 at 4:3 (unpatched image), and F * A == A / (4/3) in general (vertical field of view kept).
    CHECK(kHorPlusFactor * k4by3 == 1.0f);
    for (const float aspect : {16.0f / 9.0f, 21.0f / 9.0f, 32.0f / 9.0f})
        CHECK(near(kHorPlusFactor * aspect, aspect / k4by3));
    CHECK(near(kHorPlusFactor * (32.0f / 9.0f), 8.0f / 3.0f));

    if (g_failures == 0)
        std::printf("gow_aspect_test: OK\n");
    return g_failures == 0 ? 0 : 1;
}
