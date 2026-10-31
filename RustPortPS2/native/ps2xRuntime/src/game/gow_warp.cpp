// GOW-Port: GOW_WARP (debug/test): the game's own level warp, driven from the pad read.

#include "gow_game.h"

#include <algorithm>
#include <cctype>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <sstream>
#include <string>
#include <vector>

namespace gow_game
{
    namespace
    {
        // GOW-Port: GOW_WARP=<zone>[;<zone>...] (debug/test): the game's own level warp. The level scripts' builtins
        // LoadForWarp(zone) and Warp() (script VM builtin dispatcher 0x1AFAD0, ids 4 and 5) load the zone's WAD next to
        // the current one (0x1BB7E8) and, once it is loaded, move Kratos to the zone's warp point (0x1B25C8 ->
        // 0x1B26F0, "Jumping to warp point"). They are called here, from the pad read (main loop, once per frame), with a
        // scratch VM stack below the guest stack. Zone names are the WAD names without ".WAD" (GODOFWAR.TOC, e.g.
        // PAND04G). GOW_WARP_DELAY=<s> (default 0.5): gameplay seconds before the first warp; GOW_WARP_STAY=<s> (default
        // 60): seconds in each zone before the next one. Logs `[gow-warp] request|arrived|timeout|done`.
        // Warp state (0x29CAB0 slot / -1, 0x29CAB4 zone loaded, 0x29CAB8 warp requested, 0x29CAFC warp due) and the
        // current zone object (0x29CAC8, name via 0x1A97C8) as decoded from 0x1AFAD0 / 0x1B25C8.

        // Runs a guest function on a copy of the context to its return: a recompiled function hands control back early at
        // an EE checkpoint (ctx->pc = where to resume), so the copy is dispatched again until it returns to ra = 0.
        void gowRunGuest(uint8_t *rdram, R5900Context &call, PS2Runtime *runtime, uint32_t entry)
        {
            SET_GPR_U32(&call, 31, 0u);
            call.pc = entry;
            for (int steps = 0; call.pc != 0u && steps < 100000 && !runtime->isStopRequested(); ++steps)
                runtime->lookupFunction(call.pc)(rdram, &call, runtime);
        }
    }

    std::string gowCurrentZone(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        const uint32_t zone = readGuest32(rdram, 0x29CAC8u) & 0x01FFFFFFu;
        if (zone < 0x100000u)
            { char text[32]; std::snprintf(text, sizeof(text), "?(zone=%x)", readGuest32(rdram, 0x29CAC8u)); return text; }
        R5900Context call = *ctx;
        SET_GPR_U32(&call, 4, zone);
        SET_GPR_U32(&call, 29, (GPR_U32(ctx, 29) - 0x400u) & ~0xFu);
        gowRunGuest(rdram, call, runtime, 0x001A97C8u);
        const uint32_t name = GPR_U32((&call), 2) & 0x01FFFFFFu;
        if (name < 0x100000u)
        {
            char text[48];
            std::snprintf(text, sizeof(text), "?(zone=%x name=%x)", zone, name);
            return text;
        }
        return std::string(reinterpret_cast<const char *>(rdram + name), strnlen(reinterpret_cast<const char *>(rdram + name), 32));
    }

    namespace
    {
        // LoadForWarp(zone) (id 4) or Warp() (id 5) through the script VM builtin dispatcher, with a scratch VM stack
        // (0x140 bytes + depth word), result word and zone name below the guest stack.
        void gowWarpBuiltin(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime, uint32_t id, const std::string &zone)
        {
            const uint32_t vm = (GPR_U32(ctx, 29) - 0x800u) & ~0xFu, result = vm + 0x150u, name = vm + 0x160u;
            std::memset(rdram + vm, 0, 0x200);
            std::memcpy(rdram + name, zone.c_str(), std::min<size_t>(zone.size(), 31));
            writeGuest32(rdram, vm, name);
            writeGuest32(rdram, vm + 0x140u, zone.empty() ? 0u : 1u);
            R5900Context call = *ctx;
            SET_GPR_U32(&call, 4, 0u);
            SET_GPR_U32(&call, 5, id);
            SET_GPR_U32(&call, 6, result);
            SET_GPR_U32(&call, 7, vm);
            SET_GPR_U32(&call, 29, vm - 0x40u);
            gowRunGuest(rdram, call, runtime, 0x001AFAD0u);
        }
    }

    void gowWarpPoll(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        static const std::vector<std::string> zones = [] {
            std::vector<std::string> list;
            if (const char *text = std::getenv("GOW_WARP"))
            {
                std::string item;
                std::istringstream stream(text);
                while (std::getline(stream, item, ';'))
                    if (!item.empty())
                        list.push_back(item);
            }
            return list;
        }();
        if (zones.empty())
            return;
        static const double delay = std::getenv("GOW_WARP_DELAY") ? std::atof(std::getenv("GOW_WARP_DELAY")) : 0.5;
        static const double stay = std::getenv("GOW_WARP_STAY") ? std::atof(std::getenv("GOW_WARP_STAY")) : 60.0;
        static const auto start = std::chrono::steady_clock::now();
        const double now = std::chrono::duration<double>(std::chrono::steady_clock::now() - start).count();
        static size_t next = 0;
        static double due = -1.0, requested = -1.0;
        static bool waiting = false;
        const bool gameplay = readGuest32(rdram, 0x29E560u) == 11u && readGuest32(rdram, 0x29E574u) == 0u;
        // two captures per zone (5 s after arrival and half way through the stay): gow_warp_<n>_<zone>_<k>.ppm
        static double captureAt[2] = {-1.0, -1.0};
        for (int k = 0; k < 2; ++k)
            if (captureAt[k] >= 0.0 && now >= captureAt[k])
            {
                captureAt[k] = -1.0;
                std::vector<uint8_t> pixels;
                uint32_t width = 0, height = 0;
                if (runtime->gs().copyLatchedHostPresentationFrame(pixels, width, height, nullptr, nullptr, nullptr))
                {
                    const std::string name = "gow_warp_" + std::to_string(next) + "_" + zones[next - 1] + "_" + std::to_string(k) + ".ppm";
                    std::ofstream file(name, std::ios::binary);
                    file << "P6\n" << width << ' ' << height << "\n255\n";
                    for (size_t i = 0; i + 3 < pixels.size(); i += 4)
                        file.write(reinterpret_cast<const char *>(pixels.data() + i), 3);
                    std::fprintf(stderr, "[gow-warp] capture %s at %.1f s\n", name.c_str(), now);
                }
            }
        if (due < 0.0)
        {
            if (gameplay)
                due = now + delay;
            return;
        }
        if (waiting)
        {
            // the warp executor (0x1B25C8) clears the request and the slot when Kratos has been moved
            const bool done = readGuest32(rdram, 0x29CAB8u) == 0u && static_cast<int32_t>(readGuest32(rdram, 0x29CAB0u)) < 0;
            // Warp() only makes the warp due when the zone is already loaded; a script calls it again later
            if (!done && readGuest32(rdram, 0x29CAB4u) != 0u && readGuest32(rdram, 0x29CAFCu) == 0u && gameplay)
                gowWarpBuiltin(rdram, ctx, runtime, 5u, {});
            if (done && gameplay)
            {
                std::fprintf(stderr, "[gow-warp] arrived %s at %.1f s (%.1f s) zone=%s\n", zones[next - 1].c_str(), now,
                             now - requested, gowCurrentZone(rdram, ctx, runtime).c_str());
                waiting = false;
                due = now + stay;
                captureAt[0] = now + 5.0;
                captureAt[1] = now + std::max(6.0, stay / 2.0);
            }
            else if (now - requested > 90.0)
            {
                std::fprintf(stderr, "[gow-warp] timeout %s at %.1f s (slot=%d loaded=%u request=%u due=%u) zone=%s\n",
                             zones[next - 1].c_str(), now, static_cast<int32_t>(readGuest32(rdram, 0x29CAB0u)),
                             readGuest32(rdram, 0x29CAB4u), readGuest32(rdram, 0x29CAB8u), readGuest32(rdram, 0x29CAFCu),
                             gowCurrentZone(rdram, ctx, runtime).c_str());
                waiting = false;
                due = now;
            }
            return;
        }
        if (now < due || !gameplay)
            return;
        if (next >= zones.size())
        {
            std::fprintf(stderr, "[gow-warp] done at %.1f s\n", now);
            due = 1e30;
            return;
        }
        const std::string &zone = zones[next++];
        const std::string current = gowCurrentZone(rdram, ctx, runtime);
        std::fprintf(stderr, "[gow-warp] request %s at %.1f s from zone=%s\n", zone.c_str(), now, current.c_str());
        // LoadForWarp ignores the zone Kratos is in
        if (std::equal(zone.begin(), zone.end(), current.begin(), current.end(),
                       [](char a, char b) { return std::toupper(static_cast<unsigned char>(a)) == std::toupper(static_cast<unsigned char>(b)); }))
        {
            std::fprintf(stderr, "[gow-warp] arrived %s at %.1f s (0.0 s) zone=%s (already there)\n", zone.c_str(), now, current.c_str());
            requested = now;
            due = now + stay;
            captureAt[0] = now + 5.0;
            captureAt[1] = now + std::max(6.0, stay / 2.0);
            return;
        }
        requested = now;
        waiting = true;
        gowWarpBuiltin(rdram, ctx, runtime, 4u, zone); // LoadForWarp(zone)
        gowWarpBuiltin(rdram, ctx, runtime, 5u, {});   // Warp()
    }
}
