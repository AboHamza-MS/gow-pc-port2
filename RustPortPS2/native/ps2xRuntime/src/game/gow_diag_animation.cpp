// GOW-Port: animation / skeleton diagnostics (GOW_SKEL_*, GOW_CTRL_DIAG, GOW_BLEND_DIAG, GOW_ANM_DIAG, GOW_FAST_BOOT).

#include "gow_game.h"
#include "gow_guest_functions.h"

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <functional>
#include <map>
#include <set>
#include <thread>
#include <utility>
#include <vector>
#include <xmmintrin.h>

namespace gow_game
{
    // GOW-Port: GOW_SKEL_DIAG=1 logs each animated object passed to CalcWorldMatrix 0x1303D0 once (KIexster's
    // layout: object+0x104 skeleton, skeleton+0x60 definition with the joint count at +0x10, skeleton+0x8C joint
    // matrices, object world matrix at +0x70). GOW_SKEL_OBJECT=<hex> then logs that object's first joints'
    // local yaw (atan2 of the matrix X row) every call.
    void gowDiagCalcWorldMatrix(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        const uint32_t model = GPR_U32(ctx, 5);
        sub_00157A60_0x157a60(rdram, ctx, runtime);
        const uint32_t object = readGuest32(rdram, model + 0x18u) & 0x01FFFFFFu;
        if (!object)
            return;
        static const uint32_t watched = [] { const char *v = std::getenv("GOW_SKEL_OBJECT"); return v ? uint32_t(std::strtoul(v, nullptr, 16)) : 0u; }();
        auto f32 = [&](uint32_t a) { float v = 0; std::memcpy(&v, rdram + (a & 0x01FFFFFCu), 4); return v; };
        const uint32_t skeleton = readGuest32(rdram, object + 0x104u) & 0x01FFFFFFu;
        if (!skeleton)
            return;
        const uint32_t definition = readGuest32(rdram, skeleton + 0x60u) & 0x01FFFFFFu;
        const uint32_t joints = readGuest32(rdram, skeleton + 0x8Cu) & 0x01FFFFFFu;
        const uint32_t count = definition ? readGuest32(rdram, definition + 0x10u) : 0u;
        // GOW_SKEL_ANIM=1: per object with a joint palette, report every 90 calls how many joints moved since the previous report.
        if (std::getenv("GOW_SKEL_ANIM") && joints)
        {
            static std::map<uint32_t, std::pair<unsigned, std::vector<float>>> track;
            auto &t = track[object];
            if (++t.first % 90u == 1u)
            {
                const uint32_t n = (count > 0u && count < 256u) ? count : 125u;
                std::vector<float> cur(n * 16u);
                std::memcpy(cur.data(), rdram + (joints & 0x01FFFFFFu), n * 64u);
                unsigned moved = 0, bad = 0;
                for (uint32_t i = 0; i < n * 16u; ++i) if (!std::isfinite(cur[i])) { ++bad; break; }
                if (t.second.size() == cur.size())
                    for (uint32_t j = 0; j < n; ++j)
                        for (uint32_t k = 0; k < 16u; ++k)
                            if (std::fabs(cur[j * 16u + k] - t.second[j * 16u + k]) > 1e-4f) { ++moved; break; }
                std::fprintf(stderr, "[gow-skelanim] object=%x joints=%x n=%u moved=%u nonfinite=%u j1=%.1f,%.1f,%.1f call=%u\n", object, joints, n, moved, bad,
                             cur[16 + 12], cur[16 + 13], cur[16 + 14], t.first);
                t.second = cur;
            }
        }
        static std::set<uint32_t> seen;
        if (!watched && seen.size() < 600u && seen.insert(object).second)
            std::fprintf(stderr, "[gow-skel] object=%x skeleton=%x joints=%x count=%u pos=%.1f,%.1f,%.1f\n",
                         object, skeleton, joints, count, f32(object + 0xA0u), f32(object + 0xA4u), f32(object + 0xA8u));
        if (watched && (object & 0x01FFFFFFu) == watched && joints)
        {
            // GOW_SKEL_DUMP=<file>: append 125 joint matrices (64 bytes each) of the watched object per call.
            static const char *dumpPath = std::getenv("GOW_SKEL_DUMP");
            if (dumpPath)
                if (FILE *df = std::fopen(dumpPath, "ab")) { std::fwrite(rdram + joints, 64, 125, df); std::fclose(df); }
            if (!(count > 0u && count < 256u)) return;
            static unsigned calls = 0;
            if (++calls % 6u != 0u)
                return;
            std::fprintf(stderr, "[gow-skel] t=%u yaw", calls);
            for (uint32_t j = 0; j < count && j < 24u; ++j)
            {
                const uint32_t m = joints + j * 64u;
                std::fprintf(stderr, " %.0f", std::atan2(f32(m + 8u), f32(m)) * 57.29578f);
            }
            std::fprintf(stderr, "\n");
        }
    }

    // GOW-Port: GOW_CTRL_DIAG=1 wraps the animation controller update 0x116EF0 (this=a0: +8 time, +0xC rate) and every
    // ~3 s prints, per controller in the animator heap area, calls/time/rate/delta since the last print.
    void gowDiagController(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        const uint32_t self = GPR_U32(ctx, 4) & 0x01FFFFFFu;
        auto f32 = [&](uint32_t a) { float v = 0; std::memcpy(&v, rdram + (a & 0x01FFFFFCu), 4); return v; };
        {
            static thread_local bool reported = false;
            if (!reported)
            {
                reported = true;
                std::fprintf(stderr, "[gow-ctrl] host thread %u MXCSR=%08x (rounding mode bits %u)\n", static_cast<unsigned>(std::hash<std::thread::id>()(std::this_thread::get_id()) & 0xFFFFu), _mm_getcsr(), (_mm_getcsr() >> 13) & 3u);
            }
        }
        sub_00116EF0_0x116ef0(rdram, ctx, runtime);
        struct Info { unsigned calls = 0, lastCalls = 0; float last = 0, rate = 0, prev = 0; };
        static std::map<uint32_t, Info> info;
        static unsigned total = 0;
        Info &i = info[self];
        ++i.calls; i.last = f32(self + 8u); i.rate = f32(self + 0xCu);
        {
            // frame delta (guest 0x29C64C): running stats per print window
            static float dmin = 1e9f, dmax = 0.f; static double dsum = 0; static unsigned dn = 0;
            const float d = f32(0x29C64Cu);
            if (d > 0.f && d < 10.f) { dmin = std::min(dmin, d); dmax = std::max(dmax, d); dsum += d; ++dn; }
            if (total % 4000u == 3999u && dn)
            {
                std::fprintf(stderr, "[gow-delta] min=%.4f max=%.4f mean=%.4f samples=%u\n", dmin, dmax, dsum / dn, dn);
                dmin = 1e9f; dmax = 0.f; dsum = 0; dn = 0;
            }
        }
        if (++total % 4000u == 0u)
        {
            for (auto &kv : info)
            {
                if (kv.second.calls == kv.second.lastCalls) continue;
                std::fprintf(stderr, "[gow-ctrl] %x calls=%u time=%.3f (was %.3f) rate=%.3f\n", kv.first, kv.second.calls - kv.second.lastCalls, kv.second.last, kv.second.prev, kv.second.rate);
                kv.second.lastCalls = kv.second.calls; kv.second.prev = kv.second.last;
            }
        }
    }

    // GOW-Port: GOW_BLEND_DIAG=1 wraps the pose blend entry 0x110E40 (a0 = blend object): logs each distinct object once
    // with its mode (+0x58), flags word (*(+0x68)), source pointers (+0x78/+0x7C), joint masks (+0x84/+0x104 halfwords) and count (+0x204).
    void gowDiagBlend(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        const uint32_t self = GPR_U32(ctx, 4) & 0x01FFFFFFu;
        static std::set<uint32_t> seen;
        if (seen.size() < 64u && seen.insert(self).second)
        {
            auto u16 = [&](uint32_t a) { uint16_t v = 0; std::memcpy(&v, rdram + (a & 0x01FFFFFEu), 2); return v; };
            const uint32_t flagsPtr = readGuest32(rdram, self + 0x68u) & 0x01FFFFFFu;
            std::fprintf(stderr, "[gow-blend] %x mode=%u flags=%x src78=%x src7c=%x p60=%x p80=%x cnt204=%x w218=%x masks84:", self, readGuest32(rdram, self + 0x58u), flagsPtr ? readGuest32(rdram, flagsPtr) : 0u,
                         readGuest32(rdram, self + 0x78u), readGuest32(rdram, self + 0x7Cu), readGuest32(rdram, self + 0x60u), readGuest32(rdram, self + 0x80u), readGuest32(rdram, self + 0x204u), readGuest32(rdram, self + 0x218u));
            for (uint32_t i = 0; i < 12; ++i) std::fprintf(stderr, " %04x", u16(self + 0x84u + i * 2u));
            std::fprintf(stderr, " | m104:");
            for (uint32_t i = 0; i < 12; ++i) std::fprintf(stderr, " %04x", u16(self + 0x104u + i * 2u));
            std::fprintf(stderr, "\n");
        }
        sub_00110E40_0x110e40(rdram, ctx, runtime);
    }

    void gowDiagFlashReady(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        static unsigned calls = 0;
        if (++calls <= 30)
            std::fprintf(stderr, "[gow-flash-ready] a0=%x event=%x ra=%x\n", GPR_U32(ctx, 4), GPR_U32(ctx, 5), GPR_U32(ctx, 31));
        sub_001B2390_0x1b2390(rdram, ctx, runtime);
    }

    void gowDiagAnimationTime(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        const uint32_t ra = GPR_U32(ctx, 31), object = GPR_U32(ctx, 4);
        static const bool fastBoot = [] { const char *value = std::getenv("GOW_FAST_BOOT"); return value && std::strcmp(value, "1") == 0; }();
        if (fastBoot && ra == 0x0021E714u &&
            readGuest32(rdram, 0x29E560u) == 4u && readGuest32(rdram, 0x29E584u) == 1u)
        {
            // Only the intro-completion query is bypassed, after the level load.
            // No animation state or game-state flags are changed.
            sub_00100BF0_0x100bf0(rdram, ctx, runtime);
            return;
        }
        sub_00100C50_0x100c50(rdram, ctx, runtime);
        static unsigned samples = 0;
        if ((readGuest32(rdram, 0x29E560u) == 4u || readGuest32(rdram, 0x29E560u) == 11u) && ctx->pc == ra && samples++ % 100 == 0)
        {
            float delta = 0;
            std::memcpy(&delta, rdram + 0x29C64Cu, sizeof(delta));
            std::fprintf(stderr, "[gow-animation] object=%x time=%g delta=%g\n", object, ctx->f[0], delta);
        }
    }

    void gowDiagAnimationDuration(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        const uint32_t ra = GPR_U32(ctx, 31);
        sub_00100BF0_0x100bf0(rdram, ctx, runtime);
        static unsigned samples = 0;
        if (readGuest32(rdram, 0x29E560u) == 4u && ctx->pc == ra && samples++ % 100 == 0)
            std::fprintf(stderr, "[gow-animation] duration=%g\n", ctx->f[0]);
    }
}
