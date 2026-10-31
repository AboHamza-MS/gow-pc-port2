// GOW-Port: render-server lifecycle guard for the main frame update.

#include "gow_game.h"
#include "gow_guest_functions.h"

#include <cstdio>
#include <cstdlib>

namespace gow_game
{
    namespace
    {
        constexpr uint32_t kRenderServerTable = 0x0032E848u;
        constexpr uint32_t kPrimaryServer = 0u;
        constexpr uint32_t kFinalServer = 0x14u;
        constexpr uint32_t kInterface = 0x20u;
        constexpr uint32_t kFirstCallEnabled = 0x30u;
        constexpr uint32_t kSecondCallEnabled = 0x34u;

        struct RenderServerCall
        {
            uint32_t step = 0u;
            uint32_t server = 0u;
            uint32_t interfaceTable = 0u;
            uint32_t methodOffset = 0u;
            uint32_t method = 0u;
            int16_t adjustment = 0;
        };

        bool readRenderServerCall(uint8_t *rdram, uint32_t step, uint32_t server,
                                  uint32_t adjustmentOffset, uint32_t methodOffset,
                                  RenderServerCall &call)
        {
            call = {step, server, 0u, methodOffset, 0u, 0};
            if (guestBuffer(rdram, server, kInterface + sizeof(uint32_t)) == nullptr)
                return false;

            const uint32_t interfaceTable = readGuest32(rdram, server + kInterface);
            call.interfaceTable = interfaceTable;
            if (guestBuffer(rdram, interfaceTable, methodOffset + sizeof(uint32_t)) == nullptr)
                return false;

            call.adjustment = static_cast<int16_t>(readGuest32(rdram, interfaceTable + adjustmentOffset));
            call.method = readGuest32(rdram, interfaceTable + methodOffset);
            return call.method != 0u;
        }

        void reportSkippedCall(const RenderServerCall &call, const R5900Context *ctx)
        {
            static const bool diagnostics = std::getenv("GOW_RENDER_SERVER_DIAG") != nullptr;
            static uint32_t skippedCalls = 0u;
            if (!diagnostics)
                return;

            ++skippedCalls;
            if (skippedCalls <= 4u || (skippedCalls & (skippedCalls - 1u)) == 0u)
            {
                std::fprintf(stderr,
                             "[gow-render-server] skipped count=%u step=%u server=%08x table=%08x method@+%x=%08x ra=%08x\n",
                             skippedCalls, call.step, call.server, call.interfaceTable,
                             call.methodOffset, call.method, GPR_U32(ctx, 31));
            }
        }

        bool dispatchRenderServerCall(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime,
                                      const RenderServerCall &call, uint32_t sourcePc, uint32_t fallthroughPc)
        {
            // Mirror the generated JALR and its delay slot. Read the callback immediately before
            // dispatching it: the render server can be torn down by another host-side task between frames.
            SET_GPR_S32(ctx, 2, static_cast<int32_t>(call.method));
            SET_GPR_S32(ctx, 4, static_cast<int32_t>(call.server + static_cast<int32_t>(call.adjustment)));
            SET_GPR_U32(ctx, 31, fallthroughPc);

            if (call.method == 0u)
            {
                reportSkippedCall(call, ctx);
                ctx->pc = fallthroughPc;
                return true;
            }

            ctx->pc = call.method;
            return runtime != nullptr && runtime->dispatchGuestBranch(
                rdram, ctx, call.method, sourcePc, fallthroughPc,
                PS2Runtime::GuestBranchKind::IndirectCall, "JALR");
        }

        void restoreRenderServerFrame(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
        {
            const uint32_t sp = GPR_U32(ctx, 29);
            SET_GPR_VEC(ctx, 16, READ128(sp + 0x10u));
            SET_GPR_U64(ctx, 31, READ64(sp));
            SET_GPR_S32(ctx, 29, static_cast<int32_t>(sp + 0x20u));
            ctx->pc = GPR_U32(ctx, 31);
        }
    }

    void gowRenderServerDispatch(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        if (ctx->pc != 0x0017A830u)
        {
            sub_0017A830_0x17a830(rdram, ctx, runtime);
            return;
        }

        // The retail routine dereferences this primary entry before its first branch. If the
        // server itself is absent, defer the update intact; otherwise protect each virtual call
        // at the instant it is dispatched instead of relying on an earlier readiness snapshot.
        const uint32_t primary = readGuest32(rdram, kRenderServerTable + kPrimaryServer);
        if (guestBuffer(rdram, primary, kSecondCallEnabled + sizeof(uint32_t)) == nullptr)
        {
            RenderServerCall missing{1u, primary, 0u, 0u, 0u, 0};
            reportSkippedCall(missing, ctx);
            ctx->pc = GPR_U32(ctx, 31);
            return;
        }

        // Exact prologue of sub_0017A830. Keeping it here lets a missing callback skip only
        // that call while preserving the final server update and the caller's stack frame.
        SET_GPR_S32(ctx, 29, static_cast<int32_t>(GPR_U32(ctx, 29) - 0x20u));
        WRITE128(GPR_U32(ctx, 29) + 0x10u, GPR_VEC(ctx, 16));
        SET_GPR_S32(ctx, 16, 0x00330000);
        WRITE64(GPR_U32(ctx, 29), GPR_U64(ctx, 31));
        SET_GPR_S32(ctx, 3, static_cast<int32_t>(primary));

        const uint32_t firstEnabled = readGuest32(rdram, primary + kFirstCallEnabled);
        const uint32_t secondEnabled = readGuest32(rdram, primary + kSecondCallEnabled);
        SET_GPR_S32(ctx, 2, static_cast<int32_t>(secondEnabled)); // branch delay slot at 0x17A84C

        RenderServerCall call{};
        if (firstEnabled != 0u)
        {
            (void)readRenderServerCall(rdram, 1u, primary, 0x60u, 0x64u, call);
            if (!dispatchRenderServerCall(rdram, ctx, runtime, call, 0x0017A85Cu, 0x0017A864u))
                return;
        }
        else if (secondEnabled != 0u)
        {
            (void)readRenderServerCall(rdram, 2u, primary, 0x68u, 0x6Cu, call);
            if (!dispatchRenderServerCall(rdram, ctx, runtime, call, 0x0017A880u, 0x0017A888u))
                return;
        }

        const uint32_t finalServer = readGuest32(rdram, kRenderServerTable + kFinalServer);
        SET_GPR_S32(ctx, 2, static_cast<int32_t>(kRenderServerTable));
        SET_GPR_S32(ctx, 3, static_cast<int32_t>(finalServer));
        (void)readRenderServerCall(rdram, 3u, finalServer, 0x18u, 0x1Cu, call);
        if (!dispatchRenderServerCall(rdram, ctx, runtime, call, 0x0017A89Cu, 0x0017A8A4u))
            return;

        restoreRenderServerFrame(rdram, ctx, runtime);
    }
}
