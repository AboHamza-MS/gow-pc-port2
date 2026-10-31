// GOW-Port: recompiled-function probe (diagnostics, only with GOW_POLLING).
//
// GOW_POLLING="start_code-end_code:start_data-end_data:t:dur" wraps every recompiled function whose
// start is in [start_code, end_code) (hex). During [t, t+dur) seconds it counts, per function, the calls in
// which some argument ($a0-$a3, $t0-$t3) falls in [start_data, end_data) and keeps the first few with their
// arguments. On completion it writes polling.txt. Useful to find which functions receive a given object
// (e.g. a Flash symbol).
#include "ps2_runtime.h"
#include "ps2_runtime_macros.h"

#include <algorithm>
#include <array>
#include <chrono>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <map>
#include <string>
#include <utility>
#include <vector>

namespace
{
    constexpr size_t kMax = 2048u;
    struct Probe
    {
        uint32_t start = 0u;
        PS2Runtime::RecompiledFunction original = nullptr;
        uint64_t calls = 0u, matches = 0u;
        std::vector<std::string> samples;
    };
    std::array<Probe, kMax> g_probes;
    size_t g_probeCount = 0u;
    uint32_t g_dataStart = 0u, g_dataEnd = 0u;
    double g_t0 = 0.0, g_t1 = 0.0;
    bool g_written = false;
    const auto g_startTime = std::chrono::steady_clock::now();

    double seconds() { return std::chrono::duration<double>(std::chrono::steady_clock::now() - g_startTime).count(); }

    void write()
    {
        g_written = true;
        std::vector<const Probe *> v;
        for (size_t i = 0; i < g_probeCount; ++i)
            if (g_probes[i].matches)
                v.push_back(&g_probes[i]);
        std::sort(v.begin(), v.end(), [](const Probe *a, const Probe *b) { return a->matches > b->matches; });
        if (std::FILE *f = std::fopen("polling.txt", "w"))
        {
            for (const Probe *s : v)
            {
                std::fprintf(f, "0x%06X calls=%llu matches=%llu\n", s->start, static_cast<unsigned long long>(s->calls),
                             static_cast<unsigned long long>(s->matches));
                for (const std::string &e : s->samples)
                    std::fprintf(f, "    %s\n", e.c_str());
            }
            std::fclose(f);
        }
        std::fprintf(stderr, "[gow-polling] %zu functions with matches in polling.txt\n", v.size());
    }

    template <size_t I>
    void wrapper(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        Probe &s = g_probes[I];
        if (ctx->pc == s.start && !g_written)
        {
            const double t = seconds();
            if (t >= g_t0 && t < g_t1)
            {
                ++s.calls;
                const uint32_t regs[8] = {GPR_U32(ctx, 4), GPR_U32(ctx, 5), GPR_U32(ctx, 6), GPR_U32(ctx, 7),
                                          GPR_U32(ctx, 8), GPR_U32(ctx, 9), GPR_U32(ctx, 10), GPR_U32(ctx, 11)};
                bool matched = false;
                for (uint32_t r : regs)
                    matched |= (r & 0x1FFFFFFFu) >= g_dataStart && (r & 0x1FFFFFFFu) < g_dataEnd;
                if (matched)
                {
                    ++s.matches;
                    if (s.samples.size() < 6u)
                    {
                        char b[200];
                        std::snprintf(b, sizeof(b), "t=%.2f ra=%06X a0=%08X a1=%08X a2=%08X a3=%08X t0=%08X t1=%08X t2=%08X t3=%08X", t,
                                      GPR_U32(ctx, 31), regs[0], regs[1], regs[2], regs[3], regs[4], regs[5], regs[6], regs[7]);
                        s.samples.emplace_back(b);
                    }
                }
            }
            else if (t >= g_t1)
                write();
        }
        s.original(rdram, ctx, runtime);
    }

    template <size_t... I>
    constexpr std::array<PS2Runtime::RecompiledFunction, sizeof...(I)> table(std::index_sequence<I...>)
    {
        return {&wrapper<I>...};
    }
    const auto g_wrappers = table(std::make_index_sequence<kMax>{});
}

void gowPollingInstall(PS2Runtime &runtime)
{
    const char *v = std::getenv("GOW_POLLING");
    if (!v)
        return;
    unsigned ci = 0, cf = 0, di = 0, df = 0;
    double t = 0.0, dur = 0.0;
    if (std::sscanf(v, "%x-%x:%x-%x:%lf:%lf", &ci, &cf, &di, &df, &t, &dur) != 6)
        return;
    g_dataStart = di;
    g_dataEnd = df;
    g_t0 = t;
    g_t1 = t + dur;
    // Start of each function: the first address in the range that the table resolves to a new pointer
    // (resume entries of the same function return the same pointer).
    std::map<PS2Runtime::RecompiledFunction, uint32_t> seen;
    for (uint32_t a = ci; a < cf && g_probeCount < kMax; a += 4u)
    {
        if (!runtime.hasFunction(a))
            continue;
        PS2Runtime::RecompiledFunction f = runtime.lookupFunction(a);
        if (!f || seen.count(f))
            continue;
        seen[f] = a;
        Probe &s = g_probes[g_probeCount];
        s.start = a;
        s.original = f;
        runtime.replaceFunction(a, g_wrappers[g_probeCount]);
        ++g_probeCount;
    }
    std::fprintf(stderr, "[gow-polling] %zu functions in 0x%X-0x%X; data 0x%X-0x%X; t=%.1f+%.1f\n", g_probeCount, ci, cf, di, df, t, dur);
}
