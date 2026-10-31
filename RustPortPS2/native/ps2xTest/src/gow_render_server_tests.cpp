#include "MiniTest.h"
#include "gow_game.h"
#include "ps2_runtime.h"
#include "ps2_runtime_macros.h"

#include <cstdint>
#include <cstring>
#include <vector>

void reset_ps2_test_function_table();

namespace
{
    constexpr uint32_t kRenderServerTable = 0x0032E848u;
    constexpr uint32_t kEntry = 0x0017A830u;
    constexpr uint32_t kReturn = 0x00123456u;
    constexpr uint32_t kPrimary = 0x00100000u;
    constexpr uint32_t kPrimaryMethods = 0x00101000u;
    constexpr uint32_t kFinal = 0x00102000u;
    constexpr uint32_t kFinalMethods = 0x00103000u;
    constexpr uint32_t kFirstMethod = 0x00180000u;
    constexpr uint32_t kSecondMethod = 0x00180010u;
    constexpr uint32_t kFinalMethod = 0x00180020u;
    uint32_t g_originalCalls = 0u;
    std::vector<uint32_t> g_callbackArguments;

    void write32(std::vector<uint8_t> &rdram, uint32_t address, uint32_t value)
    {
        std::memcpy(rdram.data() + address, &value, sizeof(value));
    }

    R5900Context contextAtEntry()
    {
        R5900Context ctx{};
        ctx.pc = kEntry;
        SET_GPR_U32(&ctx, 29, 0x001FF000u);
        SET_GPR_U32(&ctx, 16, 0x00112233u);
        SET_GPR_U32(&ctx, 31, kReturn);
        return ctx;
    }

    void setServer(std::vector<uint8_t> &rdram, uint32_t slotOffset, uint32_t server, uint32_t methods)
    {
        write32(rdram, kRenderServerTable + slotOffset, server);
        write32(rdram, server + 0x20u, methods);
    }

    void setFinalReady(std::vector<uint8_t> &rdram)
    {
        setServer(rdram, 0x14u, kFinal, kFinalMethods);
        write32(rdram, kFinalMethods + 0x1Cu, kFinalMethod);
    }

    void setPrimaryReady(std::vector<uint8_t> &rdram, uint32_t enabledOffset,
                         uint32_t adjustmentOffset, uint32_t methodOffset, uint32_t method)
    {
        setServer(rdram, 0u, kPrimary, kPrimaryMethods);
        write32(rdram, kPrimary + enabledOffset, 1u);
        write32(rdram, kPrimaryMethods + adjustmentOffset, 0x10u);
        write32(rdram, kPrimaryMethods + methodOffset, method);
    }

    void callback(uint8_t *, R5900Context *ctx, PS2Runtime *)
    {
        g_callbackArguments.push_back(GPR_U32(ctx, 4));
        ctx->pc = GPR_U32(ctx, 31);
    }

    void installCallbacks(PS2Runtime &runtime)
    {
        runtime.registerFunction(kFirstMethod, callback);
        runtime.registerFunction(kSecondMethod, callback);
        runtime.registerFunction(kFinalMethod, callback);
    }
}

// The production override delegates resumed calls to the generated retail function. The synthetic
// test supplies that symbol without linking generated game code.
void sub_0017A830_0x17a830(uint8_t *, R5900Context *ctx, PS2Runtime *)
{
    ++g_originalCalls;
    ctx->pc = GPR_U32(ctx, 31);
}

void register_gow_render_server_tests()
{
    MiniTest::Case("GowRenderServer", [](TestCase &tc)
    {
        tc.BeforeEach([]
        {
            reset_ps2_test_function_table();
            g_originalCalls = 0u;
            g_callbackArguments.clear();
        });

        tc.Run("dispatches the first primary callback and final callback when complete", [](TestCase &t)
        {
            std::vector<uint8_t> rdram(PS2_RAM_SIZE, 0u);
            setPrimaryReady(rdram, 0x30u, 0x60u, 0x64u, kFirstMethod);
            setFinalReady(rdram);
            R5900Context ctx = contextAtEntry();
            PS2Runtime runtime;
            installCallbacks(runtime);

            gow_game::gowRenderServerDispatch(rdram.data(), &ctx, &runtime);

            t.Equals(g_callbackArguments.size(), size_t{2}, "both selected callbacks must run");
            t.Equals(g_callbackArguments[0], kPrimary + 0x10u, "the primary JALR delay slot must adjust a0");
            t.Equals(g_callbackArguments[1], kFinal, "the final callback receives its server");
            t.Equals(ctx.pc, kReturn, "the direct dispatcher must restore the retail return address");
            t.Equals(GPR_U32(&ctx, 29), 0x001FF000u, "the direct dispatcher must restore sp");
            t.Equals(GPR_U32(&ctx, 16), 0x00112233u, "the direct dispatcher must restore s0");
        });

        tc.Run("dispatches the second primary callback when the first is disabled", [](TestCase &t)
        {
            std::vector<uint8_t> rdram(PS2_RAM_SIZE, 0u);
            setPrimaryReady(rdram, 0x34u, 0x68u, 0x6Cu, kSecondMethod);
            setFinalReady(rdram);
            R5900Context ctx = contextAtEntry();
            PS2Runtime runtime;
            installCallbacks(runtime);

            gow_game::gowRenderServerDispatch(rdram.data(), &ctx, &runtime);

            t.Equals(g_callbackArguments.size(), size_t{2}, "the second primary and final callbacks must run");
            t.Equals(g_callbackArguments[0], kPrimary + 0x10u, "the second JALR delay slot must adjust a0");
            t.Equals(ctx.pc, kReturn, "the second path must return to its caller");
        });

        tc.Run("runs the final callback when neither primary callback is enabled", [](TestCase &t)
        {
            std::vector<uint8_t> rdram(PS2_RAM_SIZE, 0u);
            setServer(rdram, 0u, kPrimary, kPrimaryMethods);
            setFinalReady(rdram);
            R5900Context ctx = contextAtEntry();
            PS2Runtime runtime;
            installCallbacks(runtime);

            gow_game::gowRenderServerDispatch(rdram.data(), &ctx, &runtime);

            t.Equals(g_callbackArguments.size(), size_t{1}, "the final-only path must stay intact");
            t.Equals(g_callbackArguments[0], kFinal, "the final-only path must use the final server");
            t.Equals(ctx.pc, kReturn, "the final-only path must return to its caller");
        });

        tc.Run("skips only a vanished primary method and keeps the final update", [](TestCase &t)
        {
            std::vector<uint8_t> rdram(PS2_RAM_SIZE, 0u);
            setServer(rdram, 0u, kPrimary, kPrimaryMethods);
            write32(rdram, kPrimary + 0x30u, 1u);
            setFinalReady(rdram);
            R5900Context ctx = contextAtEntry();
            PS2Runtime runtime;
            installCallbacks(runtime);

            gow_game::gowRenderServerDispatch(rdram.data(), &ctx, &runtime);

            t.Equals(g_callbackArguments.size(), size_t{1}, "a missing primary method must not suppress the final update");
            t.Equals(g_callbackArguments[0], kFinal, "the remaining final callback must still run");
            t.Equals(ctx.pc, kReturn, "a skipped JALR must leave the caller frame intact");
        });

        tc.Run("skips a vanished final method after the primary update", [](TestCase &t)
        {
            std::vector<uint8_t> rdram(PS2_RAM_SIZE, 0u);
            setPrimaryReady(rdram, 0x30u, 0x60u, 0x64u, kFirstMethod);
            setServer(rdram, 0x14u, kFinal, kFinalMethods);
            R5900Context ctx = contextAtEntry();
            PS2Runtime runtime;
            installCallbacks(runtime);

            gow_game::gowRenderServerDispatch(rdram.data(), &ctx, &runtime);

            t.Equals(g_callbackArguments.size(), size_t{1}, "the completed primary callback must still run");
            t.Equals(g_callbackArguments[0], kPrimary + 0x10u, "the primary callback must retain its argument");
            t.Equals(ctx.pc, kReturn, "a skipped final JALR must return normally");
        });

        tc.Run("defers when the primary server pointer itself is invalid", [](TestCase &t)
        {
            std::vector<uint8_t> rdram(PS2_RAM_SIZE, 0u);
            setFinalReady(rdram);
            R5900Context ctx = contextAtEntry();
            PS2Runtime runtime;
            installCallbacks(runtime);

            gow_game::gowRenderServerDispatch(rdram.data(), &ctx, &runtime);

            t.Equals(g_callbackArguments.size(), size_t{0}, "an invalid primary server must not be dereferenced");
            t.Equals(ctx.pc, kReturn, "a deferred update must return to the frame caller");
        });

        tc.Run("forwards resumptions without intercepting the retail state machine again", [](TestCase &t)
        {
            std::vector<uint8_t> rdram(PS2_RAM_SIZE, 0u);
            R5900Context ctx = contextAtEntry();
            ctx.pc += 4u;

            gow_game::gowRenderServerDispatch(rdram.data(), &ctx, nullptr);

            t.Equals(g_originalCalls, 1u, "a resumed retail function must not be intercepted again");
            t.Equals(ctx.pc, kReturn, "the resumed retail function should return to its caller");
        });
    });
}
