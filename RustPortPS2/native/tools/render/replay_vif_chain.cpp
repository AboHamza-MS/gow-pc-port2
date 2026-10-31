// GOW-Port: replays a frame's VIF1 chain on the runtime's VIF1/VU1/GS, without the EE.
// Input prepared by ps2-dev vif-extract from a PCSX2 savestate or a
// dump of the port (GOW_RENDER_DIAG). It separates VIF1/VU1/GS failures from the EE's data:
// with PCSX2's packets, a correct image rules out VIF1/VU1/GS as the cause.
// The input and output files contain game data: never published.
#include "runtime/ps2_memory.h"
#include "runtime/ps2_vu1.h"
#include "runtime/gs/gs_frontend.h"
#include "runtime/gs/ps2_gif_arbiter.h"
#include <algorithm>
#include <cctype>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <iostream>
#include <string>
#include <vector>

namespace
{
    std::vector<uint8_t> load(const std::string &path)
    {
        std::ifstream file(path, std::ios::binary);
        return std::vector<uint8_t>((std::istreambuf_iterator<char>(file)), {});
    }

    void ppm(GS &gs, const std::string &path, uint32_t fbp, uint32_t fbw, uint32_t psm, uint32_t width, uint32_t height)
    {
        std::ofstream out(path, std::ios::binary);
        out << "P6\n" << width << ' ' << height << "\n255\n";
        for (uint32_t y = 0; y < height; ++y)
            for (uint32_t x = 0; x < width; ++x)
            {
                const uint32_t color = gs.ReadVram(psm, fbp * 32u, fbw, x, y);
                const char rgb[3] = {char(color & 0xFFu), char((color >> 8) & 0xFFu), char((color >> 16) & 0xFFu)};
                out.write(rgb, 3);
            }
    }
}

void gowSamplingStart(); // GOW-Port: gow_sampling.cpp (GOW_SAMPLING=<file>, sampling profile)

int main(int argc, char **argv)
{
    gowSamplingStart();
    if (argc < 3)
    {
        std::cerr << "Usage: replay_vif_chain <folder> <output prefix> [fbp=0 fbw=8 psm=0 width=512 height=448]\n";
        return 2;
    }
    const std::string dir = argv[1], prefix = argv[2];
    auto arg = [&](int i, uint32_t fallback) { return argc > i ? uint32_t(std::strtoul(argv[i], nullptr, 0)) : fallback; };
    const uint32_t fbp = arg(3, 0u), fbw = arg(4, 8u), psm = arg(5, 0u), width = arg(6, 512u), height = arg(7, 448u);

    const auto vram = load(dir + "/vram.bin");
    const auto code = load(dir + "/vu1MicroMem.bin");
    const auto data = load(dir + "/vu1Memory.bin");
    const auto stream = load(dir + "/vif1_stream.bin");
    const auto hw = load(dir + "/eeHwRegs.bin"); // optional: PCSX2's VIF1 registers
    if (vram.size() != PS2_GS_VRAM_SIZE || code.size() != PS2_VU1_CODE_SIZE || data.size() != PS2_VU1_DATA_SIZE || stream.empty())
    {
        std::cerr << "Missing vram.bin, vu1MicroMem.bin, vu1Memory.bin or vif1_stream.bin\n";
        return 1;
    }

    PS2Memory mem;
    if (!mem.initialize())
        return 1;
    std::memcpy(mem.getGSVRAM(), vram.data(), vram.size());
    std::memcpy(mem.getVU1Code(), code.data(), code.size());
    std::memcpy(mem.getVU1Data(), data.data(), data.size());
    if (hw.size() >= 0x4000u)
    {
        // VIF1 en 0x10003C00: STAT, CYCLE +0x40, MODE +0x50, MASK +0x70, ITOPS +0x90, BASE +0xA0,
        // OFST +0xB0, TOPS +0xC0, ITOP +0xD0, TOP +0xE0, R0-R3 +0x100 y C0-C3 +0x140.
        auto reg = [&](uint32_t offset) { uint32_t v; std::memcpy(&v, hw.data() + offset, 4); return v; };
        auto &vif = mem.vif1_regs;
        vif.stat = reg(0x3C00); vif.cycle = reg(0x3C40); vif.mode = reg(0x3C50); vif.mask = reg(0x3C70);
        vif.itops = reg(0x3C90); vif.base = reg(0x3CA0); vif.ofst = reg(0x3CB0); vif.tops = reg(0x3CC0);
        vif.itop = reg(0x3CD0); vif.top = reg(0x3CE0);
        for (uint32_t i = 0; i < 4u; ++i)
        {
            vif.row[i] = reg(0x3D00 + i * 16u);
            vif.col[i] = reg(0x3D40 + i * 16u);
        }
    }

    GS gs;
    gs.init(mem.getGSVRAM(), static_cast<uint32_t>(PS2_GS_VRAM_SIZE), &mem.gs());
    GifArbiter arbiter;
    // GOW-Port: GOW_REPLAY_NO_GS=1 discards the GIF packets: measures only VIF1/VU1 (in the game the GS runs on another thread).
    const bool noGs = std::getenv("GOW_REPLAY_NO_GS") != nullptr;
    // GOW-Port: fingerprint of everything going out to the GS (to compare two runs without drawing).
    uint64_t outputHash = 1469598103934665603ull, outputBytes = 0u;
    arbiter.setProcessPathPacketFn([&](GifPathId path, const uint8_t *packet, uint32_t size)
                                   {
        outputHash = (outputHash ^ static_cast<uint8_t>(path)) * 1099511628211ull;
        for (uint32_t i = 0; i < size; ++i)
            outputHash = (outputHash ^ packet[i]) * 1099511628211ull;
        outputBytes += size;
        if (!noGs) gs.processGIFPacket(packet, size, path); });
    mem.setGifArbiter(&arbiter);

    // Same as PS2Runtime::syncCoreSubsystems, without the EE's D/T bits.
    VU1Interpreter vu1;
    {
        // Like PS2Runtime: direct writes except with GOW_VU1_QUEUES=1.
        const char *queues = std::getenv("GOW_VU1_QUEUES");
        vu1.setDirectRegisterWrites(!(queues && std::strcmp(queues, "1") == 0));
    }
    uint64_t launches = 0;
    if (std::getenv("GOW_VU1_NO_COMPILE"))
        vu1.setCompiledProgramsEnabled(false);
    // GOW_REPLAY_HASHES=<file>: after each launch writes a fingerprint of VU1's state (registers,
    // flags, cycles and data memory). Comparing those of two runs gives the first differing launch.
    std::FILE *prints = nullptr;
    if (const char *path = std::getenv("GOW_REPLAY_HASHES"))
        prints = std::fopen(path, "w");
    const auto fingerprint = [&](uint32_t pc)
    {
        if (!prints)
            return;
        const VU1State &st = vu1.state();
        uint64_t hash = 1469598103934665603ull;
        const auto mix = [&](const void *data, size_t size)
        {
            const uint8_t *bytes = static_cast<const uint8_t *>(data);
            for (size_t i = 0; i < size; ++i)
                hash = (hash ^ bytes[i]) * 1099511628211ull;
        };
        // One fingerprint per group to know which part diverges.
        uint64_t parts[6]{};
        const auto part = [&](int index)
        {
            parts[index] = hash;
            hash = 1469598103934665603ull;
        };
        mix(st.vf, sizeof(st.vf));
        part(0);
        mix(st.vi, sizeof(st.vi));
        mix(st.acc, sizeof(st.acc));
        mix(&st.q, sizeof(st.q));
        mix(&st.p, sizeof(st.p));
        mix(&st.i, sizeof(st.i));
        part(1);
        mix(&st.mac, sizeof(st.mac));
        mix(&st.status, sizeof(st.status));
        mix(&st.clip, sizeof(st.clip));
        part(2);
        mix(&st.cycles, sizeof(st.cycles));
        mix(&st.pc, sizeof(st.pc));
        part(3);
        mix(mem.getVU1Data(), PS2_VU1_DATA_SIZE);
        part(4);
        std::fprintf(prints, "%llu pc=%04x vf=%016llx vi=%016llx flags=%016llx cycles=%016llx data=%016llx\n",
                     static_cast<unsigned long long>(launches), pc, static_cast<unsigned long long>(parts[0]),
                     static_cast<unsigned long long>(parts[1]), static_cast<unsigned long long>(parts[2]),
                     static_cast<unsigned long long>(parts[3]), static_cast<unsigned long long>(parts[4]));
    };
    // GOW-Port: GOW_REPLAY_PERTURB=<classes> fills with garbage, before each MSCAL, the registers of those classes:
    // "vf", "vi", "acc", "qpi" (Q, P, I), "flags" (MAC, status, clip), "all", or a single one ("vf5", "vi3"). If the
    // output to the GS does not change, no program depends on what the previous one left in those registers.
    const char *perturbText = std::getenv("GOW_REPLAY_PERTURB");
    const std::string classes = perturbText ? perturbText : "";
    const auto has = [&](const char *c) { return classes == "todo" || classes.find(c) != std::string::npos; };
    int onlyVf = -1, onlyVi = -1;
    if (classes.rfind("vf", 0) == 0 && classes.size() > 2 && std::isdigit(static_cast<unsigned char>(classes[2])))
        onlyVf = std::atoi(classes.c_str() + 2);
    if (classes.rfind("vi", 0) == 0 && classes.size() > 2 && std::isdigit(static_cast<unsigned char>(classes[2])))
        onlyVi = std::atoi(classes.c_str() + 2);
    const bool perturb = !classes.empty();
    uint64_t mscal = 0u, mscnt = 0u, seed = 0x9E3779B97F4A7C15ull;
    const auto garbage = [&] { seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17; return seed; };
    const auto garbageF = [&] { return static_cast<float>(static_cast<int32_t>(garbage() & 0xFFFFu) - 32768) * 0.37f; };
    // GOW-Port: GOW_REPLAY_TRACE_VI=<file>: per launch, pc and VI11/VI13 before and after.
    // GOW_REPLAY_CLEAR_WRITTEN=1: after each launch, garbage in everything the program wrote in data
    // memory; if the output does not change, no launch reads what another program wrote.
    std::FILE *viTrace = nullptr;
    if (const char *t = std::getenv("GOW_REPLAY_TRACE_VI"))
        viTrace = std::fopen(t, "w");
    const bool clearWritten = std::getenv("GOW_REPLAY_CLEAR_WRITTEN") != nullptr;
    std::vector<uint8_t> dataBefore(PS2_VU1_DATA_SIZE);
    mem.setVu1MscalCallback([&](uint32_t pc, uint32_t top, uint32_t itop)
                            {
        ++launches;
        ++mscal;
        const int32_t vi11 = vu1.state().vi[11], vi13 = vu1.state().vi[13];
        if (clearWritten)
            std::memcpy(dataBefore.data(), mem.getVU1Data(), PS2_VU1_DATA_SIZE);
        if (perturb)
        {
            VU1State &st = vu1.state();
            if (onlyVf > 0)
                for (int l = 0; l < 4; ++l) st.vf[onlyVf][l] = garbageF();
            else if (onlyVi > 0)
                st.vi[onlyVi] = static_cast<int16_t>(garbage());
            else
            {
                if (has("vf")) for (int r = 1; r < 32; ++r) for (int l = 0; l < 4; ++l) st.vf[r][l] = garbageF();
                if (has("vi")) for (int r = 1; r < 16; ++r) st.vi[r] = static_cast<int16_t>(garbage());
                if (has("acc")) for (int l = 0; l < 4; ++l) st.acc[l] = garbageF();
                if (has("qpi")) { st.q = garbageF(); st.p = garbageF(); st.i = garbageF(); }
                if (has("flags")) { st.mac = static_cast<uint32_t>(garbage()) & 0xFFFFu; st.status = static_cast<uint32_t>(garbage()) & 0xFFFu; st.clip = static_cast<uint32_t>(garbage()) & 0xFFFFFFu; }
            }
        }
        vu1.execute(mem.getVU1Code(), PS2_VU1_CODE_SIZE, mem.getVU1Data(), PS2_VU1_DATA_SIZE, gs, &mem, pc, top, itop, 65536);
        if (viTrace)
            std::fprintf(viTrace, "%llu pc=%04x top=%04x itop=%04x vi11 %d->%d vi13 %d->%d\n", static_cast<unsigned long long>(launches), pc, top,
                         itop, vi11, vu1.state().vi[11], vi13, vu1.state().vi[13]);
        if (clearWritten)
        {
            uint8_t *d = mem.getVU1Data();
            for (uint32_t b = 0; b < PS2_VU1_DATA_SIZE; ++b)
                if (d[b] != dataBefore[b])
                    d[b] = static_cast<uint8_t>(garbage());
        }
        fingerprint(pc); });
    mem.setVu1MscntCallback([&](uint32_t top, uint32_t itop)
                            {
        ++launches;
        ++mscnt;
        vu1.resume(mem.getVU1Code(), PS2_VU1_CODE_SIZE, mem.getVU1Data(), PS2_VU1_DATA_SIZE, gs, &mem, top, itop, 65536);
        fingerprint(0xFFFF); });

    if (!noGs) // without the GS there is no image to compare (and reading it pixel by pixel would pollute the profile)
        ppm(gs, prefix + "_before.ppm", fbp, fbw, psm, width, height);
    // GOW_REPLAY_TIMES=N repeats the chain N times and measures the time (VIF1/VU1 test bench).
    const char *timesText = std::getenv("GOW_REPLAY_TIMES");
    const uint32_t times = timesText ? std::max<uint32_t>(1u, uint32_t(std::strtoul(timesText, nullptr, 10))) : 1u;
    const auto begin = std::chrono::steady_clock::now();
    for (uint32_t pass = 0; pass < times; ++pass)
    {
        mem.processVIF1Data(stream.data(), static_cast<uint32_t>(stream.size()));
        arbiter.drain();
    }
    const double elapsed = std::chrono::duration<double>(std::chrono::steady_clock::now() - begin).count();
    if (times > 1u)
        std::printf("[replay-vif] %u passes in %.3f s (%.1f ms per frame)\n", times, elapsed, 1000.0 * elapsed / times);
    if (!noGs)
        ppm(gs, prefix + "_after.ppm", fbp, fbw, psm, width, height);
    std::printf("[replay-vif] bytes=%zu vu1_launches=%llu fbp=%u fbw=%u psm=%u\n", stream.size(),
                static_cast<unsigned long long>(launches), fbp, fbw, psm);
    std::printf("[replay-vif] MSCAL=%llu MSCNT=%llu output to the GS: %llu bytes, fingerprint %016llx\n",
                static_cast<unsigned long long>(mscal), static_cast<unsigned long long>(mscnt),
                static_cast<unsigned long long>(outputBytes), static_cast<unsigned long long>(outputHash));
    return 0;
}
