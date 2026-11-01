// GOW-Port: PC Port functions over the Waffle565 runtime (God-Of-War-Native-Port).
//
// The Waffle runtime (native GS via render targets, compiled IOP and VU1, native videos) is the base. Here, with
// the fewest possible changes to its files, the PC port's functions are wired in:
//   - Launcher window: PC_WIDTH / PC_HEIGHT (or GOW_WINDOW=<w>x<h>), PC_SCREEN=completa
//     (F11 toggles it), PC_VSYNC=1, PC_TITLE, PC_ICON, PC_ESC_QUITS=0, PC_WINDOW_FPS.
//   - Image aspect ratio: PC_ASPECT ("stretch", "4:3", "21:9"...) and the widescreen switch, whose aspect comes in
//     PC_WIDESCREEN; with neither, the original 4:3 picture.
//   - Widescreen Hor+ (PC_WIDESCREEN=A, the launcher's "Widescreen" switch): the projection reads the factor from 0x7F00
//     (patch in games/scus-97399/gow-port.toml) and it is written here every VBlank. With the switch off the game stays
//     in its original 4:3 mode (gow_pad2.cpp does not force its own widescreen) and F = 0.75, so F * aspect is exactly 1:
//     the image is bit-for-bit the same as without the patch.
//   - Configurable keyboard (PC_KEYS, ps2_pad.cpp), cheat menu (F2, gow_cheats.cpp), target lock-on
//     (Alt, gow_targeting.cpp), agile combat (gow_animation_cancel.cpp), progressive scan (gow_video.cpp) and the function probe (GOW_POLLING, gow_polling.cpp).
//   - gow_wait.cpp (the EE sleeps while waiting for the frame) is optional with GOW_EE_WAIT=1: Waffle measures the
//     game's speed with that loop's cycles and it keeps it at 1.00.
#include "ps2_runtime.h"
#include "ps2_runtime_macros.h"
#include "raylib.h"

#include <algorithm>
#include <functional>
#include <atomic>
#include <chrono>
#include <clocale>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <iostream>
#include <string>
#include <thread>
#include <cmath>
#include <cwchar>
#include <mutex>
#include <vector>
#include "gow_osd.inl" // GOW-Port: MSI Afterburner-style performance OSD (GS FPS, GPU, CPU, RAM)

namespace ps2_stubs
{
    // GOW-Port: kept out of MemoryCard.h so generated game units do not need rebuilding for a diagnostic hook.
    void logMemoryCardDebugSnapshot();
}

// NOTE: PCKeyboardSticks (ps2_pad.cpp) and the gowIntegration*/gowFrame*/gowWindow*/gowPresentationAspect
// names below are also called from ps2_pad.cpp, ps2_runtime.cpp, Kernel/EeScheduler.cpp and gs/gs_host_present.cpp,
// which are outside this clean-up pass's scope, so they keep their current names for now.
bool PCKeyboardSticks(uint8_t (&sticks)[4]); // ps2_pad.cpp (configurable keyboard)
void gowPadDiagFrame();                       // ps2_pad.cpp (GOW_PAD_DIAG)
void gowCheatsInstall(PS2Runtime &runtime, const std::string &elfPath);
void gowVideoInstall(PS2Runtime &runtime, const std::string &elfPath);
void gowTargetingInstall(PS2Runtime &runtime, const std::string &elfPath);
void gowAnimationCancelInstall(PS2Runtime &runtime, const std::string &elfPath);
void gowPollingInstall(PS2Runtime &runtime);
void gowWaitInstall(PS2Runtime &runtime, const std::string &elfPath);
void gowTargetingInput();
void gowTargetingDraw(float x, float y, float w, float h);
bool gowCheatsInputBlocked();
void gowCheatsMenuInput(bool escClosesGame);
void gowCheatsMenuDraw();
void gowCheatsVBlank(uint8_t *rdram);
void gowRetroAchievementsInstall(const std::string &elfPath);
void gowRetroAchievementsVBlank(uint8_t *rdram);
void gowRetroAchievementsDraw();
void gowSamplingStart(); // gow_sampling.cpp (GOW_SAMPLING)
void gowSamplingBurst(const char *path, double seconds, int intervalUs); // gow_sampling.cpp: synchronous sampling for a few seconds

namespace
{
    bool envFlag(const char *name, bool fallback)
    {
        const char *value = std::getenv(name);
        if (!value || !*value)
            return fallback;
        return value[0] == '1';
    }

    long envLong(const char *name, long fallback, long low, long high)
    {
        const char *value = std::getenv(name);
        if (!value || !*value)
            return fallback;
        return std::clamp(std::strtol(value, nullptr, 10), low, high);
    }

    float parseAspect(const char *value)
    {
        float width = 0.0f, height = 0.0f;
        if (std::sscanf(value, "%f:%f", &width, &height) == 2 && width > 0.0f && height > 0.0f)
            return width / height;
        const float aspect = std::strtof(value, nullptr);
        return aspect >= 0.5f && aspect <= 4.0f ? aspect : 0.0f;
    }

    bool escClosesGame()
    {
        static const bool value = envFlag("PC_ESC_QUITS", true);
        return value;
    }

    void applyCursor()
    {
        if (IsWindowFullscreen() || IsWindowState(FLAG_BORDERLESS_WINDOWED_MODE))
            HideCursor();
        else
            ShowCursor();
    }

    // GOW-Port: GetWindowHandle() returns an HWND on Windows, not a GLFWwindow*. Passing it to glfwSetWindowAttrib caused
    // an access violation when starting from the launcher in fullscreen. raylib's borderless mode
    // covers the monitor without changing its resolution and keeps the previous size for F11.
    void toggleFullscreen()
    {
        ToggleBorderlessWindowed();
        applyCursor();
    }

    // GOW-Port: God of War's pad read (scePad2Read, 0x27B9F0; gowPad2Read in the Waffle overrides). With
    // PC_KEYS the keyboard sticks come from the configurable map; with the cheat menu open the packet
    // stays idle (buttons released, sticks centered, no pressure).
    void reapplyWidescreenAspect(uint8_t *rdram); // further below, next to the widescreen code

    // GOW-Port: game frames per second (PC_SHOW_FPS=1): entries to the frame function, 0x1837B8 (the one from
    // GOW_PERF_FRAME_PC). Resumes after a checkpoint (ctx->pc different from the entry) don't count.
    constexpr uint32_t kGameFrameFunction = 0x001837B8u;
    PS2Runtime::RecompiledFunction g_gameFrameFunction = nullptr;
    std::atomic<uint64_t> g_gameFrames{0};
    int g_gameFrameLimit = 60;

    void gameFrameHook(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        if (ctx->pc == kGameFrameFunction)
        {
            // GOW-Port: limit the game's output, not the window's repeats.
            // VBlank already imposes ~60 Hz. At 30, waiting before entering vid::Flip lets
            // the guest clock advance and the next delta represents two fields.
            if (g_gameFrameLimit == 30)
            {
                using Clock = std::chrono::steady_clock;
                constexpr auto period = std::chrono::nanoseconds(1'000'000'000 / 30);
                static Clock::time_point next{};
                auto now = Clock::now();
                if (next != Clock::time_point{} && now < next)
                {
                    std::this_thread::sleep_until(next);
                    now = Clock::now();
                }
                if (next == Clock::time_point{} || now - next > period)
                    next = now + period;
                else
                    next += period;
            }
            g_gameFrames.fetch_add(1, std::memory_order_relaxed);
            // GOW-Port: [gow-frames] (GOW_FRAMES_DIAG=1): distribution of the period between game frames, per 5 s window,
            // and every long hitch (> 100 ms) as it happens. The [gow-perf] windows hide one-second hitches.
            static const bool framesDiag = envFlag("GOW_FRAMES_DIAG", envFlag("GOW_PERF_DIAG", false)); // el lanzador define GOW_PERF_DIAG=1
            if (framesDiag)
            {
                using Clock = std::chrono::steady_clock;
                static const Clock::time_point start = Clock::now();
                static Clock::time_point lastFrame{}, lastReport = start;
                static double maxMs = 0.0;
                static unsigned frames = 0, over25 = 0, over33 = 0, over50 = 0, over100 = 0;
                const auto now = Clock::now();
                if (lastFrame != Clock::time_point{})
                {
                    const double ms = std::chrono::duration<double, std::milli>(now - lastFrame).count();
                    ++frames;
                    maxMs = std::max(maxMs, ms);
                    over25 += ms > 25.0;
                    over33 += ms > 33.4;
                    over50 += ms > 50.0;
                    over100 += ms > 100.0;
                    if (ms > 100.0)
                        std::fprintf(stderr, "[gow-frames] stutter of %.0f ms at t=%.2f\n", ms, std::chrono::duration<double>(now - start).count());
                    if (ms > 1000.0)
                        ps2_stubs::logMemoryCardDebugSnapshot();
                }
                lastFrame = now;
                if (now - lastReport >= std::chrono::seconds(5))
                {
                    std::fprintf(stderr, "[gow-frames] t=%.1f frames=%u max_ms=%.1f >25ms=%u >33ms=%u >50ms=%u >100ms=%u\n",
                                 std::chrono::duration<double>(now - start).count(), frames, maxMs, over25, over33, over50, over100);
                    lastReport = now;
                    maxMs = 0.0;
                    frames = over25 = over33 = over50 = over100 = 0;
                }
            }
        }
        g_gameFrameFunction(rdram, ctx, runtime);
    }

    void drawFps(float x, float y, float h, uint64_t presentedVsyncTick, bool hasSharedFrame)
    {
        static double last = 0.0;
        static uint64_t framesBefore = 0;
        static uint64_t lastPresentedTick = 0;
        static int gameFps = 0, newFps = 0, windowFps = 0;
        static int newCount = 0, windowCount = 0;
        const double now = GetTime();
        const uint64_t frames = g_gameFrames.load(std::memory_order_relaxed);
        ++windowCount;
        // GOW-Port: 1 % low: intervals between new GS frames (the last 600, ≈ 10 s); computed every second as the FPS that
        // come from the average of the longest 1 % of intervals (like MSI Afterburner / CapFrameX). A repeated frame lengthens the interval.
        static float intervalsMs[600];
        static int intervalCount = 0, intervalHead = 0, lowFps = 0;
        static double lastNewTime = 0.0;
        if (hasSharedFrame && presentedVsyncTick != 0 && presentedVsyncTick != lastPresentedTick)
        {
            ++newCount;
            lastPresentedTick = presentedVsyncTick;
            if (lastNewTime > 0.0)
            {
                intervalsMs[intervalHead] = static_cast<float>((now - lastNewTime) * 1000.0);
                intervalHead = (intervalHead + 1) % 600;
                intervalCount = std::min(intervalCount + 1, 600);
            }
            lastNewTime = now;
        }
        if (last == 0.0)
        {
            last = now;
            framesBefore = frames;
        }
        else if (now - last >= 1.0)
        {
            const double interval = now - last;
            gameFps = static_cast<int>(static_cast<double>(frames - framesBefore) / interval + 0.5);
            newFps = static_cast<int>(newCount / interval + 0.5);
            windowFps = static_cast<int>(windowCount / interval + 0.5);
            if (intervalCount >= 30)
            {
                std::vector<float> sorted(intervalsMs, intervalsMs + intervalCount);
                const size_t worst = std::max<size_t>(1u, sorted.size() / 100u);
                std::nth_element(sorted.begin(), sorted.begin() + static_cast<std::ptrdiff_t>(worst - 1u), sorted.end(), std::greater<float>());
                double sum = 0.0;
                for (size_t i = 0; i < worst; ++i)
                    sum += sorted[i];
                const double average = sum / static_cast<double>(worst);
                lowFps = average > 0.0 ? static_cast<int>(1000.0 / average + 0.5) : 0;
            }
            // GOW-Port: [gow-osd] (GOW_PERF_DIAG=1 or GOW_FRAMES_DIAG=1): every second in which what the OSD shows drops below 55, what the
            // user sees (gs = new GS frames, game = entries to the frame function, window = refreshes) is left in the log.
            static const bool osdDiag = envFlag("GOW_FRAMES_DIAG", envFlag("GOW_PERF_DIAG", false));
            if (osdDiag && (newFps < 55 || gameFps < 55))
                std::fprintf(stderr, "[gow-osd] t=%.1f gs=%d game=%d window=%d\n", now, newFps, gameFps, windowFps);
            last = now;
            framesBefore = frames;
            newCount = windowCount = 0;
        }
        // GOW-Port: the OSD shows the GS FPS (new frames) AND the guest's own frame rate, so a repeating picture is visible, and,
        // if the system gives them, GPU, CPU and RAM. The 1 % low (lowFps) is computed but hidden.
        gow_osd::draw(x, y, h, newFps, -1, windowFps, gameFps); // HZ = window refreshes per second
    }
    constexpr uint32_t kGowPad2Read = 0x0027B9F0u;
    PS2Runtime::RecompiledFunction g_gowPad2Read = nullptr;

    void PCPad2Read(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        const uint32_t buffer = GPR_U32(ctx, 5) & 0x01FFFFFFu;
        g_gowPad2Read(rdram, ctx, runtime);
        // The Waffle overrides write the 16/9 aspect on every pad read; with PC_WIDESCREEN the chosen
        // one is written again before the game uses it (otherwise it alternates between the two every frame).
        reapplyWidescreenAspect(rdram);
        if (GPR_U32(ctx, 2) != 18u || buffer + 18u > PS2_RAM_SIZE)
            return;
        if (gowCheatsInputBlocked())
        {
            uint8_t *packet = rdram + buffer;
            packet[0] = packet[1] = 0xFFu;
            std::memset(packet + 2u, 0x80, 4u);
            std::memset(packet + 6u, 0, 12u);
            return;
        }
        // GOW-Port: the packet's sticks go in, so that with a gamepad listed only the axes the keyboard moves change.
        uint8_t sticks[4];
        std::memcpy(sticks, rdram + buffer + 2u, sizeof(sticks));
        if (PCKeyboardSticks(sticks))
            std::memcpy(rdram + buffer + 2u, sticks, sizeof(sticks));
    }

    // Widescreen Hor+ (see docs/LAUNCHER.md, "Widescreen (Hor+)"): F at 0x7F00 and the video flag at 0x7F04, every
    // VBlank and like the PCSX2 patch: aspect A at 0x32F1B0 and in both display modes (0x2FD930/0x2FD968),
    // the game's own flag 0x29CC90 = 1, 0x304D94 = 2 and videos without widescreen mode; F = 0.75 (more is seen on the
    // sides) except with the menu background (0x29CC50 = 1), where F = 1/A (vertical crop).
    void gowWidescreenVBlank(uint8_t *rdram);
}

// GOW-Port: the launcher's widescreen switch: PC_WIDESCREEN=A turns on PCSX2's "Widescreen 16:9" patch (Hor+, the
// camera is widened to A). 0 = switch off: the game keeps its original 4:3 picture (gow_pad2.cpp does not force its
// own widescreen mode). A 4:3 or narrower aspect is off as well: there is nothing to widen and, without the switch, the
// game must not be put in the widescreen mode it uses with vertical cropping.
float gowWidescreenAspect()
{
    static const float aspect = []
    {
        const char *value = std::getenv("PC_WIDESCREEN");
        if (!value || !*value)
            return 0.0f;
        const float result = parseAspect(value);
        return result > 4.0f / 3.0f + 0.02f && result <= 4.0f ? result : 0.0f;
    }();
    return aspect;
}

namespace
{
    void gowWidescreenVBlank(uint8_t *rdram)
    {
        if (!rdram)
            return;
        const auto read32 = [rdram](uint32_t address)
        {
            uint32_t value = 0u;
            std::memcpy(&value, rdram + address, sizeof(value));
            return value;
        };
        const auto write32 = [rdram](uint32_t address, uint32_t value) { std::memcpy(rdram + address, &value, sizeof(value)); };
        const auto floatBits = [](float value)
        {
            uint32_t bits = 0u;
            std::memcpy(&bits, &value, sizeof(bits));
            return bits;
        };
        const float target = gowWidescreenAspect();
        float factor = 0.75f;
        // With a video playing (state 11 of the video thread, 0x29C838; 10 is a preloaded one waiting while
        // playing) Waffle's 16:9 mode is kept: with
        // 0x304D94 = 2 the game draws the widescreen video in a 4:3 box (narrow and letterboxed); this fills the image.
        static uint32_t original304D94 = 0xFFFFFFFFu;
        const uint32_t videoState = read32(0x29C838u);
        const bool video = (videoState >= 3u && videoState <= 9u) || videoState == 11u; // loading or playing
        if (target > 0.0f && original304D94 == 0xFFFFFFFFu)
            original304D94 = read32(0x304D94u);
        if (target > 0.0f && video)
        {
            if (original304D94 != 0xFFFFFFFFu)
                write32(0x304D94u, original304D94);
            write32(0x7F04u, 1u);
        }
        else if (target > 0.0f)
        {
            const uint32_t bits = floatBits(target);
            write32(0x32F1B0u, bits);
            write32(0x2FD930u, bits);
            write32(0x2FD968u, bits);
            rdram[0x29CC90u] = 1u;
            // 0x304D94 = 2 (PCSX2 patch) makes the game put videos in a 4:3 box; with Waffle's native 16:9 it isn't
            // needed. PC_WIDESCREEN_304D94=1 writes it again (for comparisons).
            static const bool write304D94 = [] { const char *v = std::getenv("PC_WIDESCREEN_304D94"); return v && v[0] == '1'; }();
            if (write304D94)
                write32(0x304D94u, 2u);
            // Widescreen videos (.PSW): the Waffle runtime plays them natively and they fill the image;
            // the previous implementation used 4:3 because FFmpeg drew the video on top at its own aspect ratio.
            write32(0x7F04u, 1u);
            uint16_t menuBackground = 0u;
            std::memcpy(&menuBackground, rdram + 0x29CC50u, sizeof(menuBackground));
            if (menuBackground == 1u)
                factor = 1.0f / target;
        }
        else
        {
            const uint32_t bits = read32(0x32F1B0u);
            float aspect = 0.0f;
            std::memcpy(&aspect, &bits, sizeof(aspect));
            if (bits == 0x3FE38E39u) // 16/9: the game's own widescreen mode
                factor = 0.5625f;
            else if (bits != 0x3FAAAAABu && aspect > 0.5f && aspect < 4.0f) // 4/3 or still unwritten: 0.75
                factor = 1.0f / aspect;
            write32(0x7F04u, read32(0x29CC90u));
        }
        write32(0x7F00u, floatBits(factor));
    }
}

namespace
{
    void reapplyWidescreenAspect(uint8_t *rdram)
    {
        const float target = gowWidescreenAspect();
        if (target <= 0.0f || !rdram)
            return;
        uint32_t video = 0u;
        std::memcpy(&video, rdram + 0x29C838u, sizeof(video));
        if ((video >= 3u && video <= 9u) || video == 11u) // video loading or playing: Waffle's 16/9
            return;
        uint32_t bits = 0u;
        std::memcpy(&bits, &target, sizeof(bits));
        for (const uint32_t address : {0x32F1B0u, 0x2FD930u, 0x2FD968u})
            std::memcpy(rdram + address, &bits, sizeof(bits));
    }
}

// GOW-Port: Kernel/EeScheduler.cpp, at the start of every VBlank and when the scheduler restarts.
void gowIntegrationVBlank(uint8_t *rdram)
{
    gowWidescreenVBlank(rdram);
    gowCheatsVBlank(rdram);
    gowRetroAchievementsVBlank(rdram);
}

// GOW-Port: window image aspect ratio (gs_host_present.cpp). 0 = stretch to fill the window.
float gowPresentationAspect()
{
    static const float aspect = []
    {
        if (const char *value = std::getenv("PC_ASPECT"); value && *value && std::strcmp(value, "auto") != 0)
        {
            if (std::strcmp(value, "stretch") == 0)
                return 0.0f;
            if (const float fixed = parseAspect(value); fixed > 0.0f)
                return fixed;
        }
        if (const float wide = gowWidescreenAspect(); wide > 0.0f)
            return wide;
        // GOW-Port: with the widescreen switch off the game draws its original 4:3 picture (gow_pad2.cpp): presenting it
        // as 16:9 would stretch it, so the picture keeps its own geometry and the window shows side bars.
        return 4.0f / 3.0f;
    }();
    return aspect;
}

// GOW-Port: ps2_runtime.cpp (initialize), in place of Waffle's InitWindow.
void gowWindowOpen(const char *title)
{
    int width = static_cast<int>(envLong("PC_WIDTH", 1920, 320, 7680));
    int height = static_cast<int>(envLong("PC_HEIGHT", 1080, 224, 4320));
    if (!std::getenv("PC_WIDTH") && !std::getenv("PC_HEIGHT"))
        if (const char *size = std::getenv("GOW_WINDOW"))
        {
            int w = 0, h = 0;
            if (std::sscanf(size, "%dx%d", &w, &h) == 2 && w >= 320 && h >= 240)
                width = w, height = h;
        }
    const char *mode = std::getenv("PC_SCREEN");
    const bool fullscreen = mode && std::strcmp(mode, "fullscreen") == 0;
    SetConfigFlags(FLAG_WINDOW_RESIZABLE | (envFlag("PC_VSYNC", false) ? FLAG_VSYNC_HINT : 0u));
    const char *customTitle = std::getenv("PC_TITLE");
    InitWindow(width, height, customTitle && *customTitle ? customTitle : title);
    // GOW-Port: GLFW sets the system's LC_CTYPE. With tr_TR, tolower('I') is 'ı' and toupper('i') is 'İ': ASCII
    // name comparisons (IRX modules, disc files) were failing. The game doesn't use text input.
    std::setlocale(LC_CTYPE, "C");
    if (!IsWindowReady())
        return;
    const int monitor = GetCurrentMonitor();
    const int maxWidth = GetMonitorWidth(monitor), maxHeight = GetMonitorHeight(monitor);
    if (!fullscreen && maxWidth > 0 && maxHeight > 0 && (width > maxWidth || height > maxHeight))
    {
        const float scale = std::min(static_cast<float>(maxWidth) / width, static_cast<float>(maxHeight) / height);
        SetWindowSize(static_cast<int>(width * scale), static_cast<int>(height * scale));
        SetWindowPosition(0, 0);
    }
    SetWindowMinSize(320, 224);
    if (const char *icon = std::getenv("PC_ICON"); icon && *icon)
    {
        Image image = LoadImage(icon);
        if (image.data)
        {
            SetWindowIcon(image);
            UnloadImage(image);
        }
    }
    if (fullscreen)
        toggleFullscreen();
    if (!escClosesGame())
        SetExitKey(KEY_NULL);
}

// GOW-Port: window frames per second (Waffle: 60). PC_WINDOW_FPS=0 leaves it unlimited.
int gowWindowFps()
{
    const char *value = std::getenv("PC_WINDOW_FPS");
    if (!value || !*value)
        return 60;
    const long fps = std::strtol(value, nullptr, 10);
    return fps == 0 ? 0 : static_cast<int>(std::clamp<long>(fps, 30, 360));
}

// GOW-Port: ps2_runtime.cpp (loadELF), after the Waffle overrides (applyMatching): each wrapper calls the
// previous one.
void gowIntegrationInstall(PS2Runtime &runtime, const std::string &elfPath)
{
    gowSamplingStart(); // optional sampling profiler (GOW_SAMPLING=<file>)
    if (runtime.hasFunction(kGowPad2Read))
    {
        if (PS2Runtime::RecompiledFunction actual = runtime.lookupFunction(kGowPad2Read))
        {
            g_gowPad2Read = actual;
            runtime.replaceFunction(kGowPad2Read, &PCPad2Read);
            if (std::getenv("PC_KEYS"))
                std::cerr << "[teclado] configurable map active (PC_KEYS); keyboard sticks from the map" << std::endl;
        }
    }
    g_gameFrameLimit = envLong("PC_GAME_FPS", 60, 30, 60) <= 30 ? 30 : 60;
    if ((g_gameFrameLimit == 30 || envFlag("PC_SHOW_FPS", false)) && runtime.hasFunction(kGameFrameFunction))
        if (PS2Runtime::RecompiledFunction actual = runtime.lookupFunction(kGameFrameFunction))
        {
            g_gameFrameFunction = actual;
            runtime.replaceFunction(kGameFrameFunction, &gameFrameHook);
        }
    // GOW-Port: hang watchdog (GOW_PERF_DIAG=1 or GOW_FRAMES_DIAG=1): if the game stops entering the frame function for
    // 5 s, it samples the process's threads for 3 s (gow_hang_<n>.sampling, in the working folder; summarize with
    // ps2-dev sampling-win and gow-2005.map) to see where the EE spins and what the others wait for.
    if (g_gameFrameFunction && (envFlag("GOW_FRAMES_DIAG", false) || envFlag("GOW_PERF_DIAG", false)))
        std::thread([]
                    {
                        uint64_t last = 0;
                        int still = 0, dumps = 0;
                        for (;;)
                        {
                            std::this_thread::sleep_for(std::chrono::seconds(1));
                            const uint64_t now = g_gameFrames.load(std::memory_order_relaxed);
                            if (now != last || now == 0)
                                still = 0;
                            else if (++still == 5 && dumps < 3)
                            {
                                char name[64];
                                std::snprintf(name, sizeof(name), "gow_hang_%d.sampling", ++dumps);
                                std::fprintf(stderr, "[gow-frames] the game has not entered the frame function for 5 s: 3 s sampling -> %s\n", name);
                                gowSamplingBurst(name, 3.0, 1000);
                            }
                            last = now;
                        }
                    })
            .detach();
    gowCheatsInstall(runtime, elfPath);
    gowRetroAchievementsInstall(elfPath); // login, hash (ISO or ELF) and RDRAM evaluation on VBlank
    gowVideoInstall(runtime, elfPath);    // progressive scan always enabled
    gowTargetingInstall(runtime, elfPath);    // target lock-on (wraps the pad read)
    gowAnimationCancelInstall(runtime, elfPath); // cancel attacks (after fijar: chains its wrapper)
    gowPollingInstall(runtime);            // GOW_POLLING (diagnostics)
    // GOW-Port: the EE's wait for the frame sleeps instead of spinning (gow_wait.cpp), by default. Spinning occupied a whole logical thread
    // and took clock away from the VU1 one; with GOW_EE_WAIT=0 the original loop returns. Combat (26 Oct 2026): 59.1 -> 59.9 fps on average,
    // minimum 50 -> 55.4.
    if (envFlag("GOW_EE_WAIT", true))
        gowWaitInstall(runtime, elfPath);
}

// GOW-Port: ps2_runtime.cpp (window loop), before drawing every frame.
void gowFrameInput()
{
    if (IsKeyPressed(KEY_F11))
        toggleFullscreen();
    gowPadDiagFrame();                   // GOW_PAD_DIAG: what GLFW and raylib see of the joysticks
    gowCheatsMenuInput(escClosesGame()); // F2, cheat menu
    gowTargetingInput();                     // Alt locks the target, the mouse switches it
}

// GOW-Port: gs_host_present.cpp (renderFrame), on top of the game image (destination = image rectangle).
void gowFrameDraw(float x, float y, float w, float h, uint64_t presentedVsyncTick, bool hasSharedFrame)
{
    gowTargetingDraw(x, y, w, h);
    gowCheatsMenuDraw();
    gowRetroAchievementsDraw();
    if (g_gameFrameFunction)
        drawFps(x, y, h, presentedVsyncTick, hasSharedFrame);
}
