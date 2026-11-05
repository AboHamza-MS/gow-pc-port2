// GOW-Port: path (GOW_PATH_DIAG), sound driver (GOW_SND_CALLLOG, GOW_SND_STUB) and level loader (GOW_LOADER_DIAG)
// diagnostics.

#include "gow_game.h"
#include "gow_guest_functions.h"

#include <atomic>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <fstream>

namespace gow_game
{
    namespace
    {
        // Hex + ASCII dump of EE memory for the command log.
        void dumpGuestBytes(const uint8_t *rdram, uint32_t addr, uint32_t len)
        {
            for (uint32_t off = 0; off < len; off += 16u)
            {
                char hex[16 * 3 + 1] = {};
                char asc[17] = {};
                const uint32_t n = (len - off < 16u) ? (len - off) : 16u;
                for (uint32_t i = 0; i < n; ++i)
                {
                    const uint8_t c = rdram[(addr + off + i) & 0x01FFFFFFu];
                    std::snprintf(hex + i * 3, 4, "%02x ", c);
                    asc[i] = (c >= 0x20 && c < 0x7F) ? static_cast<char>(c) : '.';
                }
                std::fprintf(stderr, "[gow-snd]     +%03x: %-48s %s\n", off, hex, asc);
            }
        }

        // sub_0026BF28(cmd, size, data): sends commands to the 989snd sound driver (EE side).
        // The original does sceSifCallRpc(sid 0x123456, rpc = cmd, send = 0x305640 [size bytes],
        // reply = 0x305600 [12 bytes]) and returns word 1 of the reply.
        // cmd 0x68 = message for a 989snd plugin (smpd, the data loader): 'data' points to
        // {u32 a, u32 b, u32 len, u32 ptr} and the first 12 bytes + len bytes copied from ptr are sent.
        // Since the IOP emulator runs the original IRX modules (989NOMID.IRX + SMPD_IOP.IRX, copied into
        // IOP_MOD/ next to the ELF), the command is passed to the original; here we only log what the game asks for.
        // With GOW_SND_STUB=1 it reverts to the old behavior (answer 0 without going through the IOP).
        std::atomic<uint32_t> g_sndCmdCount{0};
        std::atomic<uint32_t> g_sndLastCmd{0};
        const bool g_sndStub = []
        {
            const char *v = std::getenv("GOW_SND_STUB");
            return v != nullptr && v[0] == '1';
        }();
    }

    void gowDiagPathSelect(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        // GOW-Port: getenv scans the whole environment; in frequently called functions it is read once.
        static const bool pathDiag = std::getenv("GOW_PATH_DIAG") != nullptr;
        if (ctx->pc == 0x00180E50u && pathDiag)
        {
            const uint32_t object = GPR_U32(ctx, 4);
            const uint32_t address = GPR_U32(ctx, 5);
            const auto *text = guestBuffer(rdram, address, 256);
            std::fprintf(stderr, "[gow-path] iterator=0x%x parent=0x%x child=%.255s\n", object,
                         readGuest32(rdram, object + 4), text ? reinterpret_cast<const char *>(text) : "<invalid>");
        }
        sub_00180E50_0x180e50(rdram, ctx, runtime);
    }

    void gowDiagAttachNode(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        static const bool pathDiag = std::getenv("GOW_PATH_DIAG") != nullptr;
        if (ctx->pc == 0x00180D08u && GPR_U32(ctx, 5) == 0 && pathDiag)
        {
            std::ofstream file("gow_path_failure.bin", std::ios::binary);
            file.write(reinterpret_cast<const char *>(rdram), 0x02000000u);
            std::fprintf(stderr, "[gow-path] NULL node iterator=0x%x ra=0x%x; RAM saved\n", GPR_U32(ctx, 4), GPR_U32(ctx, 31));
        }
        sub_00180D08_0x180d08(rdram, ctx, runtime);
    }

    void gowSnd989SendCommand(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        if (ctx->pc != 0x0026BF28u) // resume after a checkpoint inside the original
        {
            sub_0026BF28_0x26bf28(rdram, ctx, runtime);
            return;
        }
        const uint32_t cmd = GPR_U32(ctx, 4);
        const uint32_t size = GPR_U32(ctx, 5);
        const uint32_t data = GPR_U32(ctx, 6);
        // GOW_SND_CALLLOG=1: every sound command with its data, except the stream status poll 0x40. Off in normal play: each
        // command dumps up to 128 bytes and costs several stderr lines (the first 300 used to be unconditional).
        static const bool callLog = std::getenv("GOW_SND_CALLLOG") != nullptr;
        g_sndCmdCount.fetch_add(1, std::memory_order_relaxed);
        g_sndLastCmd.store(cmd, std::memory_order_relaxed);
        if (callLog && cmd != 0x40u)
        {
            if (cmd == 0x68u)
            {
                const uint32_t a = readGuest32(rdram, data);
                const uint32_t b = readGuest32(rdram, data + 4u);
                const uint32_t len = readGuest32(rdram, data + 8u);
                const uint32_t ptr = readGuest32(rdram, data + 12u);
                std::fprintf(stderr, "[gow-snd] #%d cmd=0x68 (plugin) size=0x%x a=0x%x b=0x%x len=0x%x ptr=0x%x ra=0x%x\n",
                             g_sndCmdCount.load(), size, a, b, len, ptr, GPR_U32(ctx, 31));
                dumpGuestBytes(rdram, ptr, (len < 0x80u) ? len : 0x80u);
            }
            else
            {
                std::fprintf(stderr, "[gow-snd] #%d cmd=0x%x size=0x%x data=0x%x ra=0x%x\n",
                             g_sndCmdCount.load(), cmd, size, data, GPR_U32(ctx, 31));
                dumpGuestBytes(rdram, data, (size < 0x40u) ? size : 0x40u);
            }
        }
        if (g_sndStub)
        {
            SET_GPR_U32(ctx, 2, 0u);
            ctx->pc = GPR_U32(ctx, 31);
            return;
        }
        const uint32_t ra = GPR_U32(ctx, 31);
        sub_0026BF28_0x26bf28(rdram, ctx, runtime);
        if (callLog && cmd != 0x40u && ctx->pc == ra)
        {
            std::fprintf(stderr, "[gow-snd]   -> v0=0x%x\n", GPR_U32(ctx, 2));
            // smpd read (snd_DoExternCall 'SMPD' type 6: {.., dest@+0xC, size@+0x10, ..}): first bytes read
            if (cmd == 0x4Cu && readGuest32(rdram, data) == 0x534D5044u && readGuest32(rdram, data + 4u) == 6u)
            {
                const uint32_t dst = readGuest32(rdram, data + 12u);
                std::fprintf(stderr, "[gow-snd]   data read at 0x%x:\n", dst);
                dumpGuestBytes(rdram, dst, 0x30u);
            }
        }
    }

    // GOW_SND_CALLLOG=1 (debug): the game's sound-group pause/continue (0x175148(pause, mask)) with its caller
    void gowDiagSoundGroups(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        static const bool callLog = std::getenv("GOW_SND_CALLLOG") != nullptr;
        if (callLog)
            std::fprintf(stderr, "[gow-snd-groups] pause=%u mask=%04x ra=%x\n", GPR_U32(ctx, 4), GPR_U32(ctx, 5), GPR_U32(ctx, 31));
        sub_00175148_0x175148(rdram, ctx, runtime);
    }

    // ---- GOW_LOADER_DIAG=1: the level loader (state machine 0x185A20) -------------------------------------------------
    // Between the stream commands 0x378 and 0x3E7 (handlers 0x1855E8 / 0x185678: flag 0x29C818) the loader does not
    // return to its caller: it loops until its next asynchronous disc read (0x17AFE0, through the 989snd command batch to
    // SMPD on the IOP) is reported done by the callback 0x17AFC0 (clears bit 0x4000 of the stream flags). These counters
    // and the loader's memory go into the [gow-hang] report, to tell a read that never completed from a lost callback.
    namespace
    {
        struct LoaderDiag
        {
            std::atomic<uint32_t> loader{0}, loaderCalls{0};
            std::atomic<uint32_t> reads{0}, readStream{0}, readSize{0};
            std::atomic<uint32_t> readsDone{0}, doneStream{0};
            std::atomic<int64_t> readMs{0}, doneMs{0};
        };
        LoaderDiag g_loaderDiag;

        int64_t loaderDiagNowMs()
        {
            return std::chrono::duration_cast<std::chrono::milliseconds>(std::chrono::steady_clock::now().time_since_epoch()).count();
        }

        void gowDiagLoaderStep(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
        {
            if (ctx->pc == 0x00185A20u)
            {
                g_loaderDiag.loader.store(GPR_U32(ctx, 4), std::memory_order_relaxed);
                g_loaderDiag.loaderCalls.fetch_add(1, std::memory_order_relaxed);
            }
            sub_00185A20_0x185a20(rdram, ctx, runtime);
        }

        void gowDiagLoaderRead(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
        {
            if (ctx->pc == 0x0017AFE0u)
            {
                g_loaderDiag.reads.fetch_add(1, std::memory_order_relaxed);
                g_loaderDiag.readStream.store(GPR_U32(ctx, 4), std::memory_order_relaxed);
                g_loaderDiag.readSize.store(GPR_U32(ctx, 6), std::memory_order_relaxed);
                g_loaderDiag.readMs.store(loaderDiagNowMs(), std::memory_order_relaxed);
            }
            sub_0017AFE0_0x17afe0(rdram, ctx, runtime);
        }

        void gowDiagLoaderReadDone(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
        {
            if (ctx->pc == 0x0017AFC0u)
            {
                g_loaderDiag.readsDone.fetch_add(1, std::memory_order_relaxed);
                g_loaderDiag.doneStream.store(GPR_U32(ctx, 5), std::memory_order_relaxed);
                g_loaderDiag.doneMs.store(loaderDiagNowMs(), std::memory_order_relaxed);
            }
            sub_0017AFC0_0x17afc0(rdram, ctx, runtime);
        }

        void loaderHangReport(PS2Runtime &runtime)
        {
            const uint8_t *rdram = runtime.memory().getRDRAM();
            if (!rdram)
                return;
            const int64_t now = loaderDiagNowMs();
            const uint32_t loader = g_loaderDiag.loader.load();
            std::fprintf(stderr, "[gow-hang] loader=%08x calls=%u state=%u flag@29C804=%u pending_read@29C814=%x blocking@29C818=%u\n",
                         loader, g_loaderDiag.loaderCalls.load(), loader ? readGuest32(rdram, loader) : 0u,
                         readGuest32(rdram, 0x0029C804u), readGuest32(rdram, 0x0029C814u), readGuest32(rdram, 0x0029C818u));
            if (loader)
            {
                const uint32_t stream = loader + 4u;
                std::fprintf(stderr, "[gow-hang] loader stream=%08x index=%u flags=%08x (0x4000: read in flight) size=%x position=%x\n",
                             stream, readGuest32(rdram, stream), readGuest32(rdram, stream + 4u), readGuest32(rdram, stream + 8u),
                             readGuest32(rdram, stream + 0xCu));
                for (uint32_t off = 0u; off < 0x58u; off += 0x10u)
                    std::fprintf(stderr, "[gow-hang] loader+%02x: %08x %08x %08x %08x\n", off, readGuest32(rdram, loader + off),
                                 readGuest32(rdram, loader + off + 4u), readGuest32(rdram, loader + off + 8u),
                                 readGuest32(rdram, loader + off + 12u));
            }
            const uint32_t buffer = readGuest32(rdram, 0x0029C810u);
            if (buffer)
                std::fprintf(stderr, "[gow-hang] stream buffer=%08x base=%08x read=%x write=%x end=%x count=%x\n", buffer,
                             readGuest32(rdram, buffer), readGuest32(rdram, buffer + 4u), readGuest32(rdram, buffer + 8u),
                             readGuest32(rdram, buffer + 0xCu), readGuest32(rdram, buffer + 0x10u));
            std::fprintf(stderr,
                         "[gow-hang] reads asked=%u (last stream=%08x size=%x, %lld ms ago) done=%u (last stream=%08x, %lld ms ago) snd_cmds=%u last=0x%x\n",
                         g_loaderDiag.reads.load(), g_loaderDiag.readStream.load(), g_loaderDiag.readSize.load(),
                         static_cast<long long>(now - g_loaderDiag.readMs.load()), g_loaderDiag.readsDone.load(),
                         g_loaderDiag.doneStream.load(), static_cast<long long>(now - g_loaderDiag.doneMs.load()),
                         g_sndCmdCount.load(), g_sndLastCmd.load());
        }
    }

    void gowLoaderDiagInstall(PS2Runtime &runtime)
    {
        runtime.replaceFunction(0x00185A20u, gowDiagLoaderStep);
        runtime.replaceFunction(0x0017AFE0u, gowDiagLoaderRead);
        runtime.replaceFunction(0x0017AFC0u, gowDiagLoaderReadDone);
        gowSetHangReportExtra(loaderHangReport);
        std::fprintf(stderr, "[gow-loader] loader diagnostics on (GOW_LOADER_DIAG)\n");
    }
}
