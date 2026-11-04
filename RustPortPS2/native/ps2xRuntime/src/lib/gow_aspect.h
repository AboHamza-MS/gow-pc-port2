// GOW-Port: the pure part of the image aspect ratio logic (PC_ASPECT, PC_WIDESCREEN), kept apart from gow_integration.cpp so
// that it can be tested without raylib or the game (RustPortPS2/native/tests/gow_aspect_test.cpp). Only this header and
// gow_integration.cpp include it; it is not seen by the generated code.
#pragma once

#include <cstdio>
#include <cstdlib>

namespace gowaspect
{
    // The launcher offers 4:3, 16:9, 16:10, 21:9 and 32:9 (gow-launcher, GraphicsPage.tsx). 32:9 is 3.556, so the widest
    // aspect accepted for the Hor+ patch stays at 4.0 (a 48:9 monitor is 5.33 and is not supported: the switch stays off).
    constexpr float kMinAspect = 0.5f;
    constexpr float kMaxAspect = 4.0f;
    constexpr float k4by3 = 4.0f / 3.0f;

    // Projection factor F of the Hor+ patch (the guest reads it from 0x7F00). The camera's horizontal field of view is scaled
    // by F * A (A = the chosen aspect): F = 0.75 gives A / (4/3), so that the vertical field of view is the original one at any
    // aspect; with A = 4/3 the product is exactly 1 (bit for bit the unpatched image).
    constexpr float kHorPlusFactor = 0.75f;

    // "21:9", "32:9", "1.7778"; 0 when it is not an aspect (including "stretch" and "auto").
    inline float parseAspect(const char *value)
    {
        float width = 0.0f, height = 0.0f;
        if (std::sscanf(value, "%f:%f", &width, &height) == 2 && width > 0.0f && height > 0.0f)
            return width / height;
        const float aspect = std::strtof(value, nullptr);
        return aspect >= kMinAspect && aspect <= kMaxAspect ? aspect : 0.0f;
    }

    // PC_WIDESCREEN: the aspect the camera is widened to, or 0 when the switch is off. A 4:3 or narrower aspect is off as
    // well (there is nothing to widen), and so is one wider than kMaxAspect.
    inline float widescreenAspect(const char *value)
    {
        if (!value || !*value)
            return 0.0f;
        const float result = parseAspect(value);
        return result > k4by3 + 0.02f && result <= kMaxAspect ? result : 0.0f;
    }
}
