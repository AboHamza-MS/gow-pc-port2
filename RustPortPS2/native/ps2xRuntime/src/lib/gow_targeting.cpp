// GOW-Port: lock-on targeting in God of War (SCUS-97399), like in modern God of War games.
//
// Alt (left or right, pressed and released alone) locks onto the current enemy and is pressed again to release it.
// With a target locked, moving the mouse left or right switches to the next enemy on that side of the screen.
// Kratos aims his attacks at the locked target: the game already has auto-aim, this just hands it the target.
// If the target dies it switches to the nearest enemy; with no enemies it releases. GOW_TARGETING=0 disables it.
//
// Game data (see docs/dev/GAMEPLAY-HOOKS.md, "Target lock (Alt)"):
//   *(*0x29CD58)        Kratos (creature): position +0xC0 (the feet), health +0x178, max +0x17C, height +0x124.
//   *0x29E414           creature pool: header (0x31415927), 0x20-byte entries from +0x20; a live one carries the
//                       creature at +0x00 and 0x2B95E8 at +0x18. Kratos and the area's enemies.
//   *0x29D2AC           identifier table: 12-byte entries from +8 {identifier, class, object};
//                       the index is identifier >> 23.
//   0x1D90F0 / 0x1D9750 pick Kratos's target ("Targeting Range" 0x30290C / "Manual Targeting Range" 0x302910)
//                       and return its identifier; 0x1D78B0 stores it.
//   *0x35FA98 + 0x40    camera view-projection matrix (row vector: clip = (x, y, z, 1) * M).
//
// Investigation tools (environment variables):
//   GOW_TARGETING_STICK="t0:dur:lx:ly;..."  forces the left stick (0..255) for dur seconds starting at t0.
//   GOW_TARGETING_DUMP="t1,t2,..."       dumps the EE RAM to fijar_ram_<t>.bin (local only; never uploaded).
//   GOW_TARGETING_DIAG=1                    logs target changes.
//   GOW_TARGETING_TEST="t:alt;t:der;t:izq" simulates Alt and the mouse at those seconds (tests without a keyboard).
#include "ps2_runtime.h"
#include "ps2_runtime_macros.h"
#include "raylib.h"

#include <algorithm>
#include <atomic>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <iostream>
#include <mutex>
#include <string>
#include <vector>

bool gowCheatsInputBlocked(); // gow_cheats.cpp

namespace
{
    constexpr uint32_t kPad2Read = 0x0027B9F0u;
    constexpr uint32_t kChooseTarget = 0x001D90F0u;
    constexpr uint32_t kChooseTargetManual = 0x001D9750u;
    constexpr uint32_t kPlayerControl = 0x0029CD58u;
    constexpr uint32_t kCreaturePool = 0x0029E414u;
    constexpr uint32_t kIdentifierTable = 0x0029D2ACu;
    constexpr uint32_t kCamera = 0x0035FA98u;
    constexpr uint32_t kPoolMagic = 0x31415927u;
    constexpr uint32_t kCreatureClass = 0x002B95E8u;
    constexpr float kMaxDistance = 3000.0f; // farther than this releases the target
    constexpr float kLockDistance = 1500.0f;  // range to pick one when pressing Alt

    PS2Runtime::RecompiledFunction g_pad2Read = nullptr;
    PS2Runtime::RecompiledFunction g_chooseTarget = nullptr;
    PS2Runtime::RecompiledFunction g_chooseTargetManual = nullptr;
    bool g_diag = false;

    // ---- EE memory ----
    bool validAddress(uint32_t a, uint32_t bytes = 4u)
    {
        a &= 0x1FFFFFFFu;
        return a >= 0x00100000u && a + bytes <= PS2_RAM_SIZE && (a & 3u) == 0u;
    }
    uint32_t read32(const uint8_t *rdram, uint32_t a)
    {
        uint32_t v = 0u;
        std::memcpy(&v, rdram + (a & 0x1FFFFFFu), 4);
        return v;
    }
    float readF(const uint8_t *rdram, uint32_t a)
    {
        float v = 0.0f;
        std::memcpy(&v, rdram + (a & 0x1FFFFFFu), 4);
        return v;
    }

    struct Creature
    {
        uint32_t addr = 0u;
        float x = 0.0f, y = 0.0f, z = 0.0f;
        float health = 0.0f;
        float height = 40.0f;
        // Screen (normalized -1..1 coordinates, y downward) of the chest.
        float px = 0.0f, py = 0.0f, w = 0.0f;
        bool onScreen = false;
    };

    bool readCreature(const uint8_t *rdram, uint32_t addr, Creature &c)
    {
        if (!validAddress(addr, 0x360u))
            return false;
        c.addr = addr & 0x1FFFFFFu;
        c.x = readF(rdram, c.addr + 0xC0u);
        c.y = readF(rdram, c.addr + 0xC4u);
        c.z = readF(rdram, c.addr + 0xC8u);
        const float w = readF(rdram, c.addr + 0xCCu);
        c.health = readF(rdram, c.addr + 0x178u);
        const float max = readF(rdram, c.addr + 0x17Cu);
        const float height = readF(rdram, c.addr + 0x124u);
        // +0x124: the height (35 on legionaries; on screen they measure ~41 above the feet).
        c.height = std::isfinite(height) && height > 5.0f && height < 1000.0f ? height : 40.0f;
        return w == 1.0f && std::isfinite(c.x) && std::isfinite(c.y) && std::isfinite(c.z) && std::isfinite(c.health) &&
               std::isfinite(max) && max >= 1.0f && max <= 100000.0f && c.health <= max;
    }

    // Kratos: the creature of the player controller.
    bool kratos(const uint8_t *rdram, Creature &k)
    {
        const uint32_t control = read32(rdram, kPlayerControl);
        if (!validAddress(control))
            return false;
        return readCreature(rdram, read32(rdram, control), k) && k.health > 0.0f;
    }

    std::vector<Creature> enemies(const uint8_t *rdram, uint32_t kratosAddr)
    {
        std::vector<Creature> list;
        const uint32_t pool = read32(rdram, kCreaturePool);
        if (!validAddress(pool, 0x20u) || read32(rdram, pool) != kPoolMagic)
            return list;
        const uint32_t base = (pool & 0x1FFFFFFu) + 0x20u;
        for (uint32_t i = 0; i < 128u; ++i)
        {
            const uint32_t e = base + i * 0x20u;
            if (!validAddress(e, 0x20u))
                break;
            const uint32_t first = read32(rdram, e);
            if (read32(rdram, e + 0x18u) == kCreatureClass)
            {
                Creature c;
                if ((first & 0x1FFFFFFu) != kratosAddr && readCreature(rdram, first, c) && c.health > 0.0f)
                    list.push_back(c);
                continue;
            }
            // Free entry: links to the next one or ends the list. Anything else: end of the pool.
            if (first != 0u && (first & 0x1FFFFFFu) != e + 0x20u)
                break;
        }
        return list;
    }

    bool cameraMatrix(const uint8_t *rdram, float (&m)[16])
    {
        const uint32_t camera = read32(rdram, kCamera);
        if (!validAddress(camera, 0x80u))
            return false;
        for (int i = 0; i < 16; ++i)
        {
            m[i] = readF(rdram, camera + 0x40u + static_cast<uint32_t>(i) * 4u);
            if (!std::isfinite(m[i]))
                return false;
        }
        return true;
    }

    void project(const float (&m)[16], Creature &c)
    {
        const float x = c.x, y = c.y + c.height * 0.55f, z = c.z; // the chest, like modern games' reticle
        const float cx = x * m[0] + y * m[4] + z * m[8] + m[12];
        const float cy = x * m[1] + y * m[5] + z * m[9] + m[13];
        const float cw = x * m[3] + y * m[7] + z * m[11] + m[15];
        c.w = cw;
        if (cw <= 1.0f)
        {
            c.onScreen = false;
            return;
        }
        c.px = cx / cw;
        c.py = cy / cw;
        c.onScreen = std::fabs(c.px) < 1.1f && std::fabs(c.py) < 1.1f;
    }

    float distance(const Creature &a, const Creature &b)
    {
        const float dx = a.x - b.x, dy = a.y - b.y, dz = a.z - b.z;
        return std::sqrt(dx * dx + dy * dy + dz * dz);
    }

    // ---- State (game thread) ----
    uint32_t g_locked = 0u;     // address of the locked creature (0: none)
    uint64_t g_overridden = 0u;

    // Requests from the main thread and what it needs to draw.
    std::atomic<uint32_t> g_presses{0u};
    std::atomic<int32_t> g_changes{0};
    std::atomic<bool> g_activePublished{false};
    std::mutex g_drawMutex;
    struct Draw
    {
        bool visible = false;
        float x = 0.0f, y = 0.0f;
    } g_draw;

    const Creature *find(const std::vector<Creature> &list, uint32_t addr)
    {
        for (const Creature &c : list)
            if (c.addr == addr)
                return &c;
        return nullptr;
    }

    // The "current" enemy: the closest to Kratos among the visible ones, favoring screen center.
    uint32_t pickInitial(const std::vector<Creature> &list, const Creature &k, float range)
    {
        uint32_t best = 0u;
        float score = 1e30f;
        for (const Creature &c : list)
        {
            const float d = distance(c, k);
            if (d > range)
                continue;
            const float p = d * (c.onScreen ? 1.0f + std::fabs(c.px) : 3.0f);
            if (p < score)
            {
                score = p;
                best = c.addr;
            }
        }
        return best;
    }

    // The next visible one to the left (side < 0) or to the right of the current one.
    uint32_t pickSide(const std::vector<Creature> &list, const Creature &current, int side)
    {
        uint32_t best = 0u;
        float bestDx = 1e30f;
        for (const Creature &c : list)
        {
            if (c.addr == current.addr || !c.onScreen)
                continue;
            const float dx = (c.px - current.px) * static_cast<float>(side);
            if (dx > 0.01f && dx < bestDx)
            {
                bestDx = dx;
                best = c.addr;
            }
        }
        return best;
    }

    void setLocked(uint32_t next, const char *reason)
    {
        if (next == g_locked)
            return;
        if (g_diag)
            std::fprintf(stderr, "[gow-targeting] target 0x%x -> 0x%x (%s)\n", g_locked, next, reason);
        g_locked = next;
    }

    // Once per pad read (every game frame).
    void update(const uint8_t *rdram)
    {
        const uint32_t presses = g_presses.exchange(0u);
        const int32_t changes = g_changes.exchange(0);
        Creature k;
        float m[16];
        const bool haveKratos = kratos(rdram, k);
        const bool haveCamera = cameraMatrix(rdram, m);
        std::vector<Creature> list;
        if (haveKratos)
        {
            list = enemies(rdram, k.addr);
            if (haveCamera)
                for (Creature &c : list)
                    project(m, c);
        }
        if (!haveKratos)
            setLocked(0u, "no Kratos");
        if (haveKratos && (presses & 1u))
            setLocked(g_locked ? 0u : pickInitial(list, k, kLockDistance), g_locked ? "Alt: release" : "Alt: lock");
        if (g_locked)
        {
            const Creature *current = find(list, g_locked);
            if (!current || distance(*current, k) > kMaxDistance)
                setLocked(pickInitial(list, k, kLockDistance), current ? "too far" : "target died");
            else if (changes != 0)
            {
                const uint32_t side = pickSide(list, *current, changes > 0 ? 1 : -1);
                if (side)
                    setLocked(side, changes > 0 ? "mouse: right" : "mouse: left");
            }
        }
        if (g_diag)
        {
            static auto last = std::chrono::steady_clock::now();
            const auto now = std::chrono::steady_clock::now();
            if (now - last > std::chrono::seconds(2))
            {
                last = now;
                std::fprintf(stderr, "[gow-targeting] kratos=%d camera=%d (0x%x) enemies=%zu locked=0x%x", haveKratos ? 1 : 0, haveCamera ? 1 : 0,
                             read32(rdram, kCamera), list.size(), g_locked);
                for (const Creature &c : list)
                    std::fprintf(stderr, " [0x%x %.2f,%.2f w=%.0f%s]", c.addr, c.px, c.py, c.w, c.onScreen ? "" : " offscreen");
                std::fprintf(stderr, "\n");
            }
        }
        Draw d;
        if (const Creature *c = g_locked ? find(list, g_locked) : nullptr; c && c->onScreen && haveCamera)
        {
            d.visible = true;
            d.x = c->px;
            d.y = c->py;
        }
        {
            std::lock_guard<std::mutex> lock(g_drawMutex);
            g_draw = d;
        }
        g_activePublished.store(g_locked != 0u, std::memory_order_relaxed);
    }

    // Game identifier for an object (table at 0x29D2AC); 0 if not found.
    uint32_t identifier(const uint8_t *rdram, uint32_t object)
    {
        const uint32_t table = read32(rdram, kIdentifierTable);
        if (!validAddress(table))
            return 0u;
        const uint32_t base = (table & 0x1FFFFFFu) + 8u;
        for (uint32_t i = 0; i < 2048u; ++i)
        {
            const uint32_t e = base + i * 12u;
            if (e + 12u > PS2_RAM_SIZE)
                break;
            const uint32_t id = read32(rdram, e);
            if ((id & 7u) != 0u || (id >> 23u) != i)
                continue;
            if ((read32(rdram, e + 8u) & 0x1FFFFFFu) == object)
                return id;
        }
        return 0u;
    }

    // 0x1D90F0 and 0x1D9750: after the game's own choice, overwrite with the locked target.
    void overrideTarget(uint8_t *rdram, R5900Context *ctx)
    {
        if (!g_locked)
            return;
        const uint32_t id = identifier(rdram, g_locked);
        if (!id)
            return;
        SET_GPR_U32(ctx, 2, id);
        ++g_overridden;
    }

    void chooseTarget(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        const bool entry = ctx->pc == kChooseTarget;
        g_chooseTarget(rdram, ctx, runtime);
        if (entry)
            overrideTarget(rdram, ctx);
    }

    void chooseTargetManual(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        const bool entry = ctx->pc == kChooseTargetManual;
        g_chooseTargetManual(rdram, ctx, runtime);
        if (entry)
            overrideTarget(rdram, ctx);
    }

    // ---- Investigation tools ----
    struct Forced
    {
        double start = 0.0, duration = 0.0;
        uint8_t lx = 128u, ly = 128u;
    };
    std::vector<Forced> g_forced;
    std::vector<double> g_dumps;
    std::vector<std::pair<double, int>> g_tests; // (second, 0 Alt / +1 right / -1 left)
    std::chrono::steady_clock::time_point g_t0;

    double seconds()
    {
        return std::chrono::duration<double>(std::chrono::steady_clock::now() - g_t0).count();
    }

    void readTools()
    {
        if (const char *v = std::getenv("GOW_TARGETING_STICK"))
        {
            std::string s(v);
            size_t pos = 0u;
            while (pos < s.size())
            {
                size_t end = s.find(';', pos);
                if (end == std::string::npos)
                    end = s.size();
                Forced f;
                unsigned lx = 128u, ly = 128u;
                if (std::sscanf(s.substr(pos, end - pos).c_str(), "%lf:%lf:%u:%u", &f.start, &f.duration, &lx, &ly) == 4)
                {
                    f.lx = static_cast<uint8_t>(lx);
                    f.ly = static_cast<uint8_t>(ly);
                    g_forced.push_back(f);
                }
                pos = end + 1u;
            }
        }
        if (const char *v = std::getenv("GOW_TARGETING_TEST"))
        {
            std::string s(v);
            size_t pos = 0u;
            while (pos < s.size())
            {
                size_t end = s.find(';', pos);
                if (end == std::string::npos)
                    end = s.size();
                const std::string tok = s.substr(pos, end - pos);
                const size_t colon = tok.find(':');
                if (colon != std::string::npos)
                {
                    const std::string what = tok.substr(colon + 1);
                    g_tests.push_back({std::atof(tok.substr(0, colon).c_str()), what == "der" ? 1 : (what == "izq" ? -1 : 0)});
                }
                pos = end + 1u;
            }
        }
        if (const char *v = std::getenv("GOW_TARGETING_DUMP"))
        {
            std::string s(v);
            size_t pos = 0u;
            while (pos < s.size())
            {
                size_t end = s.find(',', pos);
                if (end == std::string::npos)
                    end = s.size();
                g_dumps.push_back(std::atof(s.substr(pos, end - pos).c_str()));
                pos = end + 1u;
            }
        }
    }

    void tools(uint8_t *rdram, uint32_t buffer)
    {
        if (g_forced.empty() && g_dumps.empty() && g_tests.empty())
            return;
        const double t = seconds();
        for (auto &[when, what] : g_tests)
            if (when > 0.0 && t >= when)
            {
                if (what == 0)
                    g_presses.fetch_or(1u);
                else
                    g_changes.fetch_add(what);
                when = -1.0;
            }
        for (const Forced &f : g_forced)
            if (t >= f.start && t < f.start + f.duration)
            {
                rdram[buffer + 4u] = f.lx;
                rdram[buffer + 5u] = f.ly;
            }
        for (double &v : g_dumps)
            if (v > 0.0 && t >= v)
            {
                char name[64];
                std::snprintf(name, sizeof(name), "fijar_ram_%.0f.bin", v);
                if (FILE *f = std::fopen(name, "wb"))
                {
                    std::fwrite(rdram, 1, PS2_RAM_SIZE, f);
                    std::fclose(f);
                }
                std::fprintf(stderr, "[gow-targeting] dump %s (t=%.2f, target 0x%x, %llu overridden)\n", name, t, g_locked,
                             static_cast<unsigned long long>(g_overridden));
                v = -1.0;
            }
    }

    void pad2Read(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        const uint32_t buffer = GPR_U32(ctx, 5) & 0x01FFFFFFu;
        g_pad2Read(rdram, ctx, runtime);
        if (GPR_U32(ctx, 2) != 18u || buffer + 18u > PS2_RAM_SIZE)
            return;
        update(rdram);
        tools(rdram, buffer);
    }

    bool g_installed = false;
}

// Main thread, every window frame: Alt locks or releases; the mouse switches targets.
void gowTargetingInput()
{
    if (!g_installed || gowCheatsInputBlocked())
        return;
    // Alt counts on release, with the window focused, if it was pressed alone and briefly: this way Alt+Tab, other
    // combinations and the phantom presses that arrive when focus changes without the matching release don't count.
    static bool altDown = false, otherKey = false;
    static double altSince = 0.0;
    const bool alt = IsKeyDown(KEY_LEFT_ALT) || IsKeyDown(KEY_RIGHT_ALT);
    if (alt && !altDown)
    {
        altDown = true;
        otherKey = false;
        altSince = GetTime();
    }
    if (altDown)
        for (int key = GetKeyPressed(); key != 0; key = GetKeyPressed())
            if (key != KEY_LEFT_ALT && key != KEY_RIGHT_ALT)
                otherKey = true;
    if (!alt && altDown)
    {
        altDown = false;
        if (!otherKey && IsWindowFocused() && GetTime() - altSince < 0.6)
            g_presses.fetch_or(1u);
    }
    if (!IsWindowFocused())
        altDown = false;
    static float accumulated = 0.0f;
    static double lastChange = 0.0;
    if (!g_activePublished.load(std::memory_order_relaxed))
    {
        accumulated = 0.0f;
        return;
    }
    accumulated += GetMouseDelta().x;
    const double now = GetTime();
    constexpr float kThreshold = 60.0f; // mouse pixels to switch targets
    if (std::fabs(accumulated) >= kThreshold && now - lastChange > 0.25)
    {
        g_changes.fetch_add(accumulated > 0.0f ? 1 : -1);
        accumulated = 0.0f;
        lastChange = now;
    }
    // Without sustained movement the accumulator decays gradually.
    accumulated *= 0.9f;
}

// Main thread, after drawing the game frame to the destination: the mark over the locked target.
void gowTargetingDraw(float dx, float dy, float dw, float dh)
{
    if (!g_installed)
        return;
    Draw d;
    {
        std::lock_guard<std::mutex> lock(g_drawMutex);
        d = g_draw;
    }
    if (!d.visible)
        return;
    const float x = dx + (d.x * 0.5f + 0.5f) * dw;
    const float y = dy + (d.y * 0.5f + 0.5f) * dh;
    const float r = std::max(6.0f, dh * 0.018f);
    const float pulse = 0.75f + 0.25f * std::sin(static_cast<float>(GetTime()) * 6.0f);
    const Color red{230, 40, 30, static_cast<unsigned char>(255.0f * pulse)};
    const Color border{20, 0, 0, 200};
    // Diamond with a dark border, like modern games' target reticle.
    const Vector2 top{x, y - r}, right{x + r * 0.75f, y}, bottom{x, y + r}, left{x - r * 0.75f, y};
    DrawTriangle(top, left, bottom, red);
    DrawTriangle(top, bottom, right, red);
    DrawLineEx(top, right, 2.0f, border);
    DrawLineEx(right, bottom, 2.0f, border);
    DrawLineEx(bottom, left, 2.0f, border);
    DrawLineEx(left, top, 2.0f, border);
}

void gowTargetingInstall(PS2Runtime &runtime, const std::string &elfPath)
{
    if (std::filesystem::path(elfPath).filename() != "SCUS_973.99")
        return;
    g_t0 = std::chrono::steady_clock::now();
    readTools();
    const char *active = std::getenv("GOW_TARGETING");
    if (active && active[0] == '0')
        return;
    const char *diag = std::getenv("GOW_TARGETING_DIAG");
    g_diag = diag && diag[0] == '1';
    if (!runtime.hasFunction(kPad2Read) || !runtime.hasFunction(kChooseTarget) || !runtime.hasFunction(kChooseTargetManual))
        return;
    g_pad2Read = runtime.lookupFunction(kPad2Read);
    runtime.replaceFunction(kPad2Read, &pad2Read);
    g_chooseTarget = runtime.lookupFunction(kChooseTarget);
    runtime.replaceFunction(kChooseTarget, &chooseTarget);
    g_chooseTargetManual = runtime.lookupFunction(kChooseTargetManual);
    runtime.replaceFunction(kChooseTargetManual, &chooseTargetManual);
    g_installed = true;
    std::cerr << "[gow-targeting] target lock: Alt; mouse switches targets" << std::endl;
}
