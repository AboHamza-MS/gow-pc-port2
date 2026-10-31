// GOW-Port: scripted-input smoke test of the pad read (GOW_PAD_TEST), with its captures and dumps.

#include "gow_game.h"
#include "runtime/ee_scheduler.h"

#include <algorithm>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <iterator>
#include <sstream>
#include <string>
#include <utility>
#include <vector>

namespace gow_game
{
    // Opt-in smoke test (GOW_PAD_TEST=1): advances the title and new-game prompts without host UI input and captures
    // the GS's own presented pixels; normal play never injects input. Driven by GOW_PAD_SCRIPT, GOW_PAD_CAPTURES,
    // GOW_RAM_DUMP, GOW_THREAD_DUMP, GOW_GOD_MODE, GOW_PLAYER_TRACE and GOW_RENDER_DIAG (see docs/).

    bool padSmokeTestEnabled()
    {
        static const bool smokeTest = [] {
            const char *value = std::getenv("GOW_PAD_TEST");
            return value && std::strcmp(value, "1") == 0;
        }();
        return smokeTest;
    }

    void padSmokeTest(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime, uint8_t (&state)[32])
    {
        static const auto testStart = std::chrono::steady_clock::now();
        // the script alone drives the test: a connected gamepad or the keyboard must not leak into it (stick drift
        // of a real pad changed the save slot picked and the walk of the zone checks)
        state[2] = state[3] = 0xFF;
        state[4] = state[5] = state[6] = state[7] = 128;
        for (size_t i = 8; i < sizeof(state); ++i)
            state[i] = 0;
        const double seconds = std::chrono::duration<double>(std::chrono::steady_clock::now() - testStart).count();
        // GOW-Port: times written "g<s>" in GOW_PAD_SCRIPT / GOW_PAD_CAPTURES count from the start of gameplay
        // (first state 11 with nothing pending), so runs at different speeds act at the same point in the game.
        // They are stored as kGameplayTime + s.
        constexpr double kGameplayTime = 1.0e6;
        static double gameplayStart = -1.0;
        if (gameplayStart < 0.0 && readGuest32(rdram, 0x29E560u) == 11u && readGuest32(rdram, 0x29E574u) == 0u)
        {
            gameplayStart = seconds;
            std::fprintf(stderr, "[gow-pad2:test] gameplay start at %.2f s\n", seconds);
        }
        auto parseTime = [](const std::string &text) {
            return !text.empty() && text[0] == 'g' ? kGameplayTime + std::strtod(text.c_str() + 1, nullptr)
                                                   : std::strtod(text.c_str(), nullptr);
        };
        auto reached = [&](double when) {
            if (when < kGameplayTime)
                return seconds >= when;
            return gameplayStart >= 0.0 && seconds - gameplayStart >= when - kGameplayTime;
        };
        // GOW-Port: GOW_RAM_DUMP=<time>:<file> writes the 32 MB EE RAM once (time as in GOW_PAD_CAPTURES,
        // e.g. g5:ram.bin), to compare with eeMemory.bin from a PCSX2 save state at the same point.
        // GOW-Port: GOW_THREAD_DUMP=<t>: lists the EE threads once (entry, priorities, status, wait).
        static const char *threadDump = std::getenv("GOW_THREAD_DUMP");
        static bool threadDumped = false;
        if (threadDump && !threadDumped && reached(parseTime(threadDump)))
        {
            threadDumped = true;
            const EeKernelSnapshot snapshot = runtime->eeScheduler().snapshot();
            std::fprintf(stderr, "[gow-threads] running=%d\n", snapshot.runningThreadId);
            for (const EeThreadSnapshot &t : snapshot.threads)
                std::fprintf(stderr, "[gow-threads] id=%d entry=%08x prio=%d/%d status=%d wait=%d/%d pc=%08x ra=%08x\n",
                             t.id, t.entry, t.initialPriority, t.currentPriority, int(t.status), int(t.waitReason),
                             t.waitId, t.pc, t.ra);
        }
        // GOW_RAM_DUMP=<t>[;<t>...]:<file>: one dump per time; with several times the files are <file>.<n>.
        static const char *ramDump = std::getenv("GOW_RAM_DUMP");
        static size_t ramDumpIndex = 0;
        if (ramDump)
        {
            const char *colon = std::strchr(ramDump, ':');
            std::vector<std::string> times;
            for (const char *c = ramDump; colon && c < colon;)
            {
                const char *e = c;
                while (e < colon && *e != ';') ++e;
                times.emplace_back(c, e);
                c = e < colon ? e + 1 : colon;
            }
            if (colon && ramDumpIndex < times.size() && reached(parseTime(times[ramDumpIndex])))
            {
                std::string path = colon + 1;
                if (times.size() > 1)
                    path += "." + std::to_string(ramDumpIndex);
                ++ramDumpIndex;
                if (std::FILE *file = std::fopen(path.c_str(), "wb"))
                {
                    std::fwrite(rdram, 1, 0x02000000u, file);
                    std::fclose(file);
                }
                std::fprintf(stderr, "[gow-pad2:test] RAM dumped to %s at %.2f s\n", path.c_str(), seconds);
            }
        }
        // GOW-Port: check the state during the profile without writing captures or dumps.
        static const bool noCapture = [] {
            const char *value = std::getenv("GOW_PAD_TEST_NO_CAPTURE");
            return value && std::strcmp(value, "1") == 0;
        }();
        // GOW-Port: GOW_PLAYER_TRACE=1 logs once per second Kratos's position (0x2C7790; player
        // object *0x29E5CC), the game state and the movie state, to steer scripted runs.
        // GOW_GOD_MODE=1 (test runs only): Kratos's health (creature *0x29D2B4, +0x198 current, +0x19C maximum,
        // floats) is refilled every poll so long scripted runs are not ended by deaths.
        static const bool godMode = std::getenv("GOW_GOD_MODE") != nullptr;
        if (godMode)
            if (const uint32_t creature = readGuest32(rdram, 0x29D2B4u) & 0x01FFFFFFu; creature >= 0x100000u)
            {
                const uint32_t maximum = readGuest32(rdram, creature + 0x19Cu);
                float value = 0;
                std::memcpy(&value, &maximum, 4);
                if (value > 0.0f && value <= 10000.0f)
                    writeGuest32(rdram, creature + 0x198u, maximum);
            }
        static const bool playerTrace = std::getenv("GOW_PLAYER_TRACE") != nullptr;
        static double nextPlayerReport = 0;
        if (playerTrace && seconds >= nextPlayerReport)
        {
            nextPlayerReport = seconds + 1;
            const uint32_t player = readGuest32(rdram, 0x29E5CCu) & 0x01FFFFFFu;
            // 0x2C7790: a world position vector that follows Kratos's walk (found by RAM diffs while walking).
            float pos[3]{};
            std::memcpy(pos, rdram + 0x2C7790u, sizeof(pos));
            const uint32_t creature = readGuest32(rdram, 0x29D2B4u) & 0x01FFFFFFu;
            float health = 0;
            if (creature >= 0x100000u)
                std::memcpy(&health, rdram + creature + 0x198u, 4);
            std::fprintf(stderr, "[gow-player] t=%.1f g=%.1f obj=%x pos=%.1f,%.1f,%.1f hp=%.0f state=%u movie=%u zone=%s\n", seconds,
                         gameplayStart >= 0.0 ? seconds - gameplayStart : -1.0, player, pos[0], pos[1], pos[2], health,
                         readGuest32(rdram, 0x29E560u), readGuest32(rdram, 0x29C838u),
                         gowCurrentZone(rdram, ctx, runtime).c_str());
        }
        static double nextStateReport = 0;
        static const bool primDiag = [] { const char *diag=std::getenv("GOW_EE_PRIM_DIAG"); return diag && std::strcmp(diag,"1")==0; }();
        if (primDiag)
            gowDiagPrimPoll(rdram,seconds);
        if (noCapture && seconds >= nextStateReport)
        {
            nextStateReport = seconds + 5;
            std::fprintf(stderr, "[gow-pad2:state] seconds=%.2f state=%u pending=%u stage=%u levelReady=%u flashReady=%u movie=%u\n",
                         seconds, readGuest32(rdram, 0x29E560u), readGuest32(rdram, 0x29E574u),
                         readGuest32(rdram, 0x29E5A0u), readGuest32(rdram, 0x29E584u),
                         readGuest32(rdram, 0x29CAB4u), readGuest32(rdram, 0x29C838u));
        }
        // GOW_PAD_SCRIPT="5:start,12:down,16:x": 0.7 s presses at those seconds (instead of the
        // fixed sequence). Buttons: start, select, up, down, left, right,
        // x, circle, square, triangle, l1, r1, l2, r2.
        // GOW-Port: also left/right stick directions (lup, ldown, lleft, lright, rup, rdown, rleft, rright),
        // and "*<s>" sets how long it is held (default 0.7 s), e.g. "140:lup*6".
        // For a stick, byte = state index of the axis and mask = the axis value to hold.
        struct ScriptedPress { double seconds; uint8_t byte; uint8_t mask; double hold = 0.7; bool axis = false; };
        static const std::vector<ScriptedPress> script = [&] {
            std::vector<ScriptedPress> presses;
            const char *text = std::getenv("GOW_PAD_SCRIPT");
            if (!text)
                return presses;
            static const std::pair<const char *, std::pair<uint8_t, uint8_t>> buttons[] = {
                {"select", {2, 0x01}}, {"start", {2, 0x08}}, {"up", {2, 0x10}}, {"right", {2, 0x20}},
                {"down", {2, 0x40}}, {"left", {2, 0x80}}, {"l2", {3, 0x01}}, {"r2", {3, 0x02}},
                {"l1", {3, 0x04}}, {"r1", {3, 0x08}}, {"triangle", {3, 0x10}}, {"circle", {3, 0x20}},
                {"x", {3, 0x40}}, {"square", {3, 0x80}}};
            std::string item;
            std::istringstream stream(text);
            while (std::getline(stream, item, ','))
            {
                const size_t colon = item.find(':');
                if (colon == std::string::npos)
                    continue;
                std::string name = item.substr(colon + 1);
                double hold = 0.7;
                if (const size_t star = name.find('*'); star != std::string::npos)
                {
                    hold = std::strtod(name.c_str() + star + 1, nullptr);
                    name.resize(star);
                }
                const double at = parseTime(item.substr(0, colon));
                for (const auto &[button, bit] : buttons)
                    if (name == button)
                        presses.push_back({at, bit.first, bit.second, hold, false});
                // state[4..7] = RX, RY, LX, LY (0 = left/up, 255 = right/down)
                static const std::pair<const char *, std::pair<uint8_t, uint8_t>> sticks[] = {
                    {"lleft", {6, 0}}, {"lright", {6, 255}}, {"lup", {7, 0}}, {"ldown", {7, 255}},
                    {"rleft", {4, 0}}, {"rright", {4, 255}}, {"rup", {5, 0}}, {"rdown", {5, 255}}};
                for (const auto &[stick, axis] : sticks)
                    if (name == stick)
                        presses.push_back({at, axis.first, axis.second, hold, true});
            }
            return presses;
        }();
        static const bool scriptUsesGameplayTime =
            std::any_of(script.begin(), script.end(), [&](const ScriptedPress &p) { return p.seconds >= kGameplayTime; });
        if (!script.empty())
        {
            for (const ScriptedPress &press : script)
                // With gameplay-relative entries, absolute ones only drive the menus (spare confirm
                // presses would otherwise act in gameplay at a different point on faster runs).
                if (reached(press.seconds) && !reached(press.seconds + press.hold) &&
                    !(scriptUsesGameplayTime && press.seconds < kGameplayTime && gameplayStart >= 0.0))
                {
                    if (press.axis)
                        state[press.byte] = press.mask;
                    else
                        state[press.byte] &= static_cast<uint8_t>(~press.mask);
                }
        }
        else
        {
            constexpr double presses[] = {5, 12, 20, 28, 36, 52, 60, 68, 76, 84, 100, 116, 132};
            for (size_t i = 0; i < std::size(presses); ++i)
                if (seconds >= presses[i] && seconds < presses[i] + 0.7)
                {
                    if (i == 0) state[2] &= ~0x08u; // Start
                    else state[3] &= ~0x40u; // Cross
                }
        }
        static size_t capture = 0;
        // GOW-Port: GOW_PAD_CAPTURES="t1,t2,..." (seconds, ascending) replaces the default capture times.
        static const std::vector<double> captures = [&] {
            std::vector<double> times;
            if (const char *text = std::getenv("GOW_PAD_CAPTURES"))
            {
                std::string item;
                std::istringstream stream(text);
                while (std::getline(stream, item, ','))
                    times.push_back(parseTime(item));
            }
            if (times.empty())
                times = {4, 10, 18, 26, 34, 44, 56, 70, 90, 110, 130, 160, 190, 240, 360, 480, 580};
            return times;
        }();
        if (!noCapture && capture < captures.size() && reached(captures[capture]))
        {
            std::vector<uint8_t> pixels;
            uint32_t width = 0, height = 0, display = 0, source = 0;
            bool preferred = false;
            if (runtime->gs().copyLatchedHostPresentationFrame(pixels, width, height, &display, &source, &preferred))
            {
                const std::string name = "gow_pad_test_" + std::to_string(capture) + ".ppm";
                std::ofstream file(name, std::ios::binary);
                file << "P6\n" << width << ' ' << height << "\n255\n";
                for (size_t i = 0; i + 3 < pixels.size(); i += 4)
                    file.write(reinterpret_cast<const char *>(pixels.data() + i), 3);
                std::fprintf(stderr, "[gow-pad2:test] frame=%s seconds=%.2f\n", name.c_str(), seconds);
                uint32_t gameState = 0, pending = 0;
                std::memcpy(&gameState, rdram + 0x0029E560u, sizeof(gameState));
                std::memcpy(&pending, rdram + 0x0029E574u, sizeof(pending));
                std::fprintf(stderr, "[gow-pad2:test] state=%u pending=%u stage=%u levelReady=%u flashReady=%u movie=%u\n",
                             gameState, pending, readGuest32(rdram, 0x29E5A0u), readGuest32(rdram, 0x29E584u),
                             readGuest32(rdram, 0x29CAB4u), readGuest32(rdram, 0x29C838u));
                if (std::getenv("GOW_RENDER_DIAG"))
                {
                    static bool renderDumped = false;
                    // GOW_RENDER_DIAG_FROM=<seconds>: dump the first in-game capture from that point on.
                    static const double dumpFrom = []
                    {
                        const char *v = std::getenv("GOW_RENDER_DIAG_FROM");
                        return v ? std::strtod(v, nullptr) : 0.0;
                    }();
                    if (gameState == 11u && !renderDumped && seconds >= dumpFrom)
                    {
                        renderDumped = true;
                        std::ofstream code("gow_vu1_code.bin", std::ios::binary);
                        code.write(reinterpret_cast<const char *>(runtime->memory().getVU1Code()), 0x4000);
                        std::ofstream data("gow_vu1_data.bin", std::ios::binary);
                        data.write(reinterpret_cast<const char *>(runtime->memory().getVU1Data()), 0x4000);
                        std::ofstream ram("gow_render_ram.bin", std::ios::binary);
                        ram.write(reinterpret_cast<const char *>(rdram), 0x02000000u);
                        // Synchronize the GPU backend's VRAM before reading the host buffer.
                        runtime->gs().refreshDisplaySnapshot();
                        const auto gsState = runtime->gs().getDebugSnapshot();
                        std::ofstream vram("gow_render_vram.bin", std::ios::binary);
                        vram.write(reinterpret_cast<const char *>(runtime->memory().getGSVRAM()), PS2_GS_VRAM_SIZE);
                        for (unsigned context = 0; context < 2; ++context)
                        {
                            const auto &frame = gsState.ctx[context].frame;
                            if (!frame.fbw || (frame.psm != 0u && frame.psm != 1u)) continue;
                            const std::string name = "gow_render_context_" + std::to_string(context) + ".ppm";
                            std::ofstream image(name, std::ios::binary);
                            image << "P6\n" << width << ' ' << height << "\n255\n";
                            uint32_t nonBlack = 0u;
                            for (uint32_t y = 0; y < height; ++y)
                                for (uint32_t x = 0; x < width; ++x)
                                {
                                    const uint32_t color = runtime->gs().ReadVram(frame.psm, frame.fbp * 32u, frame.fbw, x, y);
                                    const uint8_t rgb[] = {uint8_t(color), uint8_t(color >> 8), uint8_t(color >> 16)};
                                    image.write(reinterpret_cast<const char *>(rgb), 3);
                                    nonBlack += (color & 0xFFFFFFu) != 0u;
                                }
                            std::fprintf(stderr, "[gow-gs:buffer] context=%u fbp=%u fbw=%u psm=%u nonBlack=%u file=%s\n",
                                         context, frame.fbp, frame.fbw, frame.psm, nonBlack, name.c_str());
                        }
                    }
                    const auto snapshot = runtime->gs().getDebugSnapshot();
                    std::fprintf(stderr, "[gow-gs] ctxFbp=%u,%u display=%u source=%u\n", snapshot.ctx[0].frame.fbp,
                                 snapshot.ctx[1].frame.fbp, display, source);
                    unsigned count = 0;
                    const auto history = runtime->gs().getDebugHistory();
                    for (auto event = history.rbegin(); event != history.rend() && count < 16; ++event)
                        if (event->kind == GSDebugEventKind::Draw)
                        {
                            ++count;
                            std::fprintf(stderr, "[gow-gs:draw] prim=%u tex=%u fbp=%u xy=%g,%g:%g,%g z=%g:%g a=%u:%u test=%llx alpha=%llx\n",
                                         unsigned(event->prim.type), unsigned(event->prim.tme), event->frame.fbp,
                                         event->xMin, event->yMin, event->xMax, event->yMax, event->zMin, event->zMax,
                                         event->aMin, event->aMax, static_cast<unsigned long long>(event->test),
                                         static_cast<unsigned long long>(event->alpha));
                        }
                }
            }
            ++capture;
        }
    }
}
