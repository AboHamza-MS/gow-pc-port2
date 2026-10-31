// GOW-Port: path (GOW_PATH_DIAG) and sound driver (GOW_SND_CALLLOG, GOW_SND_STUB) diagnostics.

#include "gow_game.h"
#include "gow_guest_functions.h"

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
        int g_sndCmdCount = 0;
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
        ++g_sndCmdCount;
        if (callLog && cmd != 0x40u)
        {
            if (cmd == 0x68u)
            {
                const uint32_t a = readGuest32(rdram, data);
                const uint32_t b = readGuest32(rdram, data + 4u);
                const uint32_t len = readGuest32(rdram, data + 8u);
                const uint32_t ptr = readGuest32(rdram, data + 12u);
                std::fprintf(stderr, "[gow-snd] #%d cmd=0x68 (plugin) size=0x%x a=0x%x b=0x%x len=0x%x ptr=0x%x ra=0x%x\n",
                             g_sndCmdCount, size, a, b, len, ptr, GPR_U32(ctx, 31));
                dumpGuestBytes(rdram, ptr, (len < 0x80u) ? len : 0x80u);
            }
            else
            {
                std::fprintf(stderr, "[gow-snd] #%d cmd=0x%x size=0x%x data=0x%x ra=0x%x\n",
                             g_sndCmdCount, cmd, size, data, GPR_U32(ctx, 31));
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
}
