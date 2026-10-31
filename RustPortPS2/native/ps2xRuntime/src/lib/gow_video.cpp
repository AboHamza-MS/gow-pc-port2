// GOW-Port: video mode for God of War (SCUS-97399).
//
// The "Progressive Scan" pause-menu option (480p) removes line doubling and the de-interlacing
// flicker filter: the image is much sharper. It is always forced on here (GOW_PROGRESSIVE=0 reverts
// to the game's own setting).
//
// - Settings: func_1B5D70 registers them by name ("WideScreen" +0x29C, "ProgressiveScan" +0x2A0, "DeFlicker" +0x2A4
//   on object 0x29C9F4). The "ProgressiveScan" value lives at 0x29CC94; the menu changes it via
//   func_1B5870 -> func_1B5B20.
// - func_21FDA0 (every frame, from 0x17A2A0) compares 0x29CC94 with the applied mode (0x29E610) and, if it
//   changed, picks descriptor 0x2FD938 (progressive) or 0x2FD900 (interlaced) and calls func_1830D8, which
//   reprograms the GS (sceGsResetGraph and display registers).
// Writing 1 to 0x29CC94 before that check makes the game switch modes through its own path. If the
// player turns the option off in the menu, it is forced back on the next frame.
#include "ps2_runtime.h"

#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <iostream>
#include <string>

namespace
{
    constexpr uint32_t kCheckMode = 0x21FDA0u; // applies the chosen video mode if it changed
    constexpr uint32_t kProgressive = 0x29CC94u; // "ProgressiveScan" setting

    PS2Runtime::RecompiledFunction g_checkMode = nullptr;

    void checkMode(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        // Only on entry; a resume after a checkpoint just follows the original.
        if (ctx->pc == kCheckMode)
        {
            const uint32_t one = 1u;
            std::memcpy(rdram + kProgressive, &one, sizeof(one));
        }
        g_checkMode(rdram, ctx, runtime);
    }
}

void gowVideoInstall(PS2Runtime &runtime, const std::string &elfPath)
{
    if (std::filesystem::path(elfPath).filename() != "SCUS_973.99")
        return;
    const char *value = std::getenv("GOW_PROGRESSIVE");
    if (value && value[0] == '0')
        return;
    if (!runtime.hasFunction(kCheckMode))
        return;
    g_checkMode = runtime.lookupFunction(kCheckMode);
    runtime.replaceFunction(kCheckMode, &checkMode);
    std::cerr << "[gow-video] progressive scan (480p) always enabled" << std::endl;
}
