// GOW-Port: render pipeline diagnostics (GOW_RENDER_DIAG filtered copy, GOW_EE_PRIM_DIAG, GOW_MODEL_DIAG).
// They only read the guest state; the original function always runs.

#include "gow_game.h"
#include "gow_guest_functions.h"

#include <array>
#include <cstdio>
#include <cstring>

namespace gow_game
{
    namespace
    {
        struct GowPrimProbe { uint32_t object=0,caller=0,address=0; };
        std::array<GowPrimProbe,16> g_primProbes{};
        size_t g_nextPrimProbe=0;

        struct GowModelContextSamples { uint32_t physical = 0u; unsigned early = 0u, scene = 0u; };

        // GOW-Port: the master is shared by several servers; limiting each context
        // keeps the first servers from exhausting the model server's samples.
        unsigned gowModelContextSample(std::array<GowModelContextSamples, 8> &probes, uint32_t context, uint32_t state)
        {
            const uint32_t physical = context & 0x1FFFFFFFu;
            if (!physical) return 0u;
            for (auto &probe : probes)
                if (probe.physical == physical || !probe.physical)
                {
                    probe.physical = physical;
                    auto &samples = state == 11u ? probe.scene : probe.early;
                    return samples < 64u ? ++samples : 0u;
                }
            return 0u;
        }
    }

    void gowDiagFilteredCopy(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        static unsigned entries = 0, resumes = 0;
        if (ctx->pc == 0x001262C8u && ++entries <= 30)
            std::fprintf(stderr, "[gow-filter] entry a0=%x a1=%x a2=%x a3=%x t0=%x t1=%x t2=%x t3=%x ra=%x sp=%x\n",
                         GPR_U32(ctx, 4), GPR_U32(ctx, 5), GPR_U32(ctx, 6), GPR_U32(ctx, 7),
                         GPR_U32(ctx, 8), GPR_U32(ctx, 9), GPR_U32(ctx, 10), GPR_U32(ctx, 11),
                         GPR_U32(ctx, 31), GPR_U32(ctx, 29));
        if (ctx->pc == 0x00126528u && ++resumes <= 40)
            std::fprintf(stderr, "[gow-filter] resume index=%x limit=%x rows=%x ra=%x sp=%x\n",
                         GPR_U32(ctx, 10), GPR_U32(ctx, 12), GPR_U32(ctx, 14), GPR_U32(ctx, 31), GPR_U32(ctx, 29));
        sub_001262C8_0x1262c8(rdram, ctx, runtime);
    }

    // GOW-Port: watch the position producer, without altering buffers or EE execution.
    void gowDiagInitUnpack(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        if (ctx->pc != 0x00141350u) { sub_00141350_0x141350(rdram, ctx, runtime); return; }
        const uint32_t object=GPR_U32(ctx,4), data=GPR_U32(ctx,5), type=GPR_U32(ctx,6);
        const uint32_t chunk=GPR_U32(ctx,7), buffer=GPR_U32(ctx,8), caller=GPR_U32(ctx,31);
        sub_00141350_0x141350(rdram, ctx, runtime);
        static unsigned samples=0;
        if (samples++ < 64 && ctx->pc == caller)
            std::fprintf(stderr,"[gow-eeprim:init] this=%x caller=%x type=%u chunk=%u buffer=%u command=%x next=%x\n",
                         object,caller,type,chunk,buffer,data,GPR_U32(ctx,2));
    }

    // GOW-Port: watch the list used by ProcessServer before running the original.
    // SCUS-97399's list uses nodes at object+8 and ends in NULL (MIPS 0x141BF4/0x1421A4).
    void gowDiagPrimContext(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        if (ctx->pc != 0x00141B78u) { sub_00141B78_0x141b78(rdram, ctx, runtime); return; }
        static unsigned early = 0u, scene = 0u;
        const uint32_t state = readGuest32(rdram, 0x29E560u);
        auto &sample = state == 11u ? scene : early;
        const uint32_t context = GPR_U32(ctx, 4);
        if (sample < 64u && guestBuffer(rdram, context, 0x4Cu))
        {
            ++sample;
            const uint32_t view = readGuest32(rdram, 0x33104Cu);
            const bool validView = guestBuffer(rdram, view, 0x3B0u) != nullptr;
            const uint32_t viewId = validView ? readGuest32(rdram, view + 0x3ACu) : 0u;
            const uint32_t contextMask = readGuest32(rdram, context + 0x48u);
            uint32_t camera = 0u;
            if (validView)
            {
                const uint32_t cameraNode = readGuest32(rdram, view + 0x360u);
                if (cameraNode != view + 0x360u && guestBuffer(rdram, cameraNode, 12u))
                    camera = readGuest32(rdram, cameraNode + 8u);
            }
            std::array<uint32_t, 256> seen{};
            std::array<bool, 16> members{};
            uint32_t node = readGuest32(rdram, context + 0x24u);
            const uint32_t head = node;
            size_t count = 0u;
            uint32_t candidates = 0u;
            bool invalid = false, cycle = false;
            while (node && count < seen.size())
            {
                const uint32_t physical = node & 0x1FFFFFFFu;
                for (size_t i = 0u; i < count; ++i)
                    cycle |= seen[i] == physical;
                if (cycle) break;
                if (physical < 8u || !guestBuffer(rdram, node - 8u, 0x148u)) { invalid = true; break; }
                seen[count++] = physical;
                const uint32_t object = node - 8u;
                const auto *objectData = guestBuffer(rdram, object, 0x148u);
                const int8_t updated = static_cast<int8_t>(objectData[0x146u]);
                const uint32_t dma = (updated == 0 || updated == 1) ? readGuest32(rdram, object + 0xF0u + 4u * updated) : 0u;
                const uint32_t objectMask = readGuest32(rdram, object + 0xD8u);
                candidates += camera && (viewId & contextMask) && dma && (viewId & objectMask) ? 1u : 0u;
                for (size_t i = 0u; i < g_primProbes.size(); ++i)
                    members[i] = members[i] || (g_primProbes[i].object &&
                        (g_primProbes[i].object & 0x1FFFFFFFu) == (object & 0x1FFFFFFFu));
                node = readGuest32(rdram, node);
            }
            std::fprintf(stderr, "[gow-eeprim:context] sample=%u state=%u this=%x view=%x validView=%u id=%x mask=%x camera=%x head=%x nodes=%zu candidates=%u invalid=%u cycle=%u truncated=%u\n",
                         sample, state, context, view, validView ? 1u : 0u, viewId, contextMask, camera, head,
                         count, candidates, invalid ? 1u : 0u, cycle ? 1u : 0u, node && !invalid && !cycle ? 1u : 0u);
            for (size_t i = 0u; i < g_primProbes.size(); ++i)
            {
                const auto &p = g_primProbes[i];
                const auto *objectData = guestBuffer(rdram, p.object, 0x148u);
                if (!objectData) continue;
                std::fprintf(stderr, "[gow-eeprim:member] sample=%u state=%u context=%x this=%x caller=%x member=%u updated=%u rendered=%u viewMask=%x next=%x dma=%x,%x\n",
                             sample, state, context, p.object, p.caller, members[i] ? 1u : 0u,
                             objectData[0x146u], objectData[0x147u], readGuest32(rdram, p.object + 0xD8u),
                             readGuest32(rdram, p.object + 8u), readGuest32(rdram, p.object + 0xF0u), readGuest32(rdram, p.object + 0xF4u));
            }
        }
        // Never write indices, positions or registers: the original also keeps its own checkpoints.
        sub_00141B78_0x141b78(rdram, ctx, runtime);
    }

    // GOW-Port: pick the model context per the retail MIPS, without modifying it.
    void gowDiagModelServer(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        if (ctx->pc != 0x00159C58u) { sub_00159C58_0x159c58(rdram, ctx, runtime); return; }
        const uint32_t state = readGuest32(rdram, 0x29E560u), server = GPR_U32(ctx, 4);
        static unsigned early = 0u, scene = 0u;
        auto &samples = state == 11u ? scene : early;
        if (samples < 64u)
        {
            const unsigned sample = ++samples;
            const bool validServer = guestBuffer(rdram, server, 0xD8u) != nullptr;
            const uint32_t table = validServer ? readGuest32(rdram, server + 0x24u) : 0u;
            const uint32_t group = validServer ? readGuest32(rdram, server + 0x44u) : 0u;
            const uint32_t slot = validServer ? readGuest32(rdram, server + 0xD4u) : 0u;
            // 64-bit arithmetic to avoid wrapping indices outside of RAM.
            const uint64_t row = (table & 0x1FFFFFFFu) + uint64_t(group) * 12u;
            const bool validRow = table && row < 0x02000000u && guestBuffer(rdram, uint32_t(row), 12u);
            const uint32_t array = validRow ? readGuest32(rdram, uint32_t(row)) : 0u;
            const uint64_t cell = (array & 0x1FFFFFFFu) + uint64_t(slot) * 4u;
            const bool validCell = array && cell < 0x02000000u && guestBuffer(rdram, uint32_t(cell), 4u);
            const uint32_t context = validCell ? readGuest32(rdram, uint32_t(cell)) : 0u;
            const bool validContext = guestBuffer(rdram, context, 0x24u) != nullptr;
            const uint32_t vtable = validContext ? readGuest32(rdram, context + 0x20u) : 0u;
            const auto *methods = guestBuffer(rdram, vtable, 0x20u);
            int16_t adjustment = 0;
            if (methods) std::memcpy(&adjustment, methods + 0x18u, sizeof(adjustment));
            const uint32_t target = methods ? readGuest32(rdram, vtable + 0x1Cu) : 0u;
            std::fprintf(stderr, "[gow-model:server] sample=%u state=%u this=%x valid=%u table=%x group=%u slot=%u array=%x context=%x validContext=%u vtable=%x validMethods=%u adjustment=%d target=%x\n",
                         sample, state, server, validServer ? 1u : 0u, table, group, slot, array, context,
                         validContext ? 1u : 0u, vtable, methods ? 1u : 0u, int(adjustment), target);
        }
        // The selection is an entry snapshot; the original keeps its own checkpoints and calls.
        sub_00159C58_0x159c58(rdram, ctx, runtime);
    }

    // GOW-Port: GROB master list per the retail MIPS 0x151238-0x151334.
    // An entry snapshot does not replace execution nor test a submission to the GS.
    void gowDiagGrobMaster(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        if (ctx->pc != 0x001511F0u) { sub_001511F0_0x1511f0(rdram, ctx, runtime); return; }
        const uint32_t state = readGuest32(rdram, 0x29E560u), context = GPR_U32(ctx, 4);
        static std::array<GowModelContextSamples, 8> probes{};
        const unsigned sample = gowModelContextSample(probes, context, state);
        if (sample)
        {
            const bool validContext = guestBuffer(rdram, context, 0x28u) != nullptr;
            const uint32_t head = validContext ? readGuest32(rdram, context + 0x24u) : 0u;
            uint32_t node = head;
            std::array<uint32_t, 256> seen{};
            size_t count = 0u, active = 0u, invalidRoutes = 0u;
            bool invalid = !validContext, cycle = false;
            while (node && count < seen.size())
            {
                const uint32_t physical = node & 0x1FFFFFFFu;
                for (size_t i = 0u; i < count; ++i) cycle |= seen[i] == physical;
                if (cycle) break;
                const auto *clientData = physical >= 8u ? guestBuffer(rdram, node - 8u, 0x48u) : nullptr;
                if (!clientData) { invalid = true; break; }
                seen[count++] = physical;
                const uint32_t client = node - 8u;
                uint16_t id = 0u;
                std::memcpy(&id, clientData, sizeof(id));
                const uint32_t enabled = readGuest32(rdram, client + 0x2Cu);
                const uint32_t view = readGuest32(rdram, client + 0x44u);
                const uint32_t server = readGuest32(rdram, 0x32E848u + uint32_t(id) * 4u);
                const uint32_t vtable = readGuest32(rdram, client + 0x20u);
                const auto *methods = guestBuffer(rdram, vtable, 0x20u);
                int16_t adjustment = 0;
                if (methods) std::memcpy(&adjustment, methods + 0x18u, sizeof(adjustment));
                const uint32_t target = methods ? readGuest32(rdram, vtable + 0x1Cu) : 0u;
                const bool validRoute = methods && guestBuffer(rdram, server, 0x24u) &&
                    guestBuffer(rdram, view, 2u) && guestBuffer(rdram, target, 4u);
                active += enabled ? 1u : 0u;
                invalidRoutes += enabled && !validRoute ? 1u : 0u;
                if (sample <= 8u)
                    std::fprintf(stderr, "[gow-model:grob-client] sample=%u state=%u master=%x index=%zu this=%x id=%x enabled=%x clients=%x view=%x server=%x vtable=%x adjustment=%d target=%x validRoute=%u\n",
                                 sample, state, context, count - 1u, client, unsigned(id), enabled,
                                 readGuest32(rdram, client + 0x24u), view, server, vtable, int(adjustment), target,
                                 validRoute ? 1u : 0u);
                node = readGuest32(rdram, node);
            }
            std::fprintf(stderr, "[gow-model:grob-master] sample=%u state=%u this=%x valid=%u head=%x nodes=%zu active=%zu invalidRoutes=%zu invalid=%u cycle=%u truncated=%u\n",
                         sample, state, context, validContext ? 1u : 0u, head, count, active, invalidRoutes,
                         invalid ? 1u : 0u, cycle ? 1u : 0u, node && !invalid && !cycle ? 1u : 0u);
        }
        sub_001511F0_0x1511f0(rdram, ctx, runtime);
    }

    // GOW-Port: model list filters, checked against MIPS 0x1598A0-0x15991C.
    void gowDiagModelContext(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        if (ctx->pc != 0x00159878u) { sub_00159878_0x159878(rdram, ctx, runtime); return; }
        const uint32_t state = readGuest32(rdram, 0x29E560u), context = GPR_U32(ctx, 4);
        static std::array<GowModelContextSamples, 8> probes{};
        const unsigned sample = gowModelContextSample(probes, context, state);
        if (sample)
        {
            const bool validContext = guestBuffer(rdram, context, 0x60u) != nullptr;
            const uint32_t view = readGuest32(rdram, 0x33104Cu);
            const bool validView = guestBuffer(rdram, view, 0x3B0u) != nullptr;
            const uint32_t viewId = validView ? readGuest32(rdram, view + 0x3ACu) : 0u;
            const uint32_t mask = validContext ? readGuest32(rdram, context + 0x48u) : 0u;
            const uint32_t staticCount = validContext ? readGuest32(rdram, context + 0x54u) : 0u;
            const uint32_t sphereTree = validContext ? readGuest32(rdram, context + 0x58u) : 0u;
            const uint32_t sphereValid = validContext ? readGuest32(rdram, context + 0x5Cu) : 0u;
            const bool useStatic = sphereValid && staticCount;
            uint32_t node = validContext ? readGuest32(rdram, context + 0x24u) : 0u;
            const uint32_t head = node;
            std::array<uint32_t, 256> seen{};
            size_t count = 0u, viewRejected = 0u, treeCandidates = 0u, processCandidates = 0u;
            bool invalid = !validContext || !validView, cycle = false;
            while (node && count < seen.size())
            {
                const uint32_t physical = node & 0x1FFFFFFFu;
                for (size_t i = 0u; i < count; ++i) cycle |= seen[i] == physical;
                if (cycle) break;
                const auto *data = physical >= 8u ? guestBuffer(rdram, node - 8u, 0x124u) : nullptr;
                if (!data) { invalid = true; break; }
                seen[count++] = physical;
                const uint32_t model = node - 8u, modelMask = readGuest32(rdram, model + 0xD8u);
                int16_t sphereId = 0;
                std::memcpy(&sphereId, data + 0x122u, sizeof(sphereId));
                const bool passesView = (viewId & mask) && (viewId & modelMask);
                viewRejected += !passesView ? 1u : 0u;
                treeCandidates += passesView && useStatic && sphereId >= 0 ? 1u : 0u;
                processCandidates += passesView && (!useStatic || sphereId < 0) ? 1u : 0u;
                if (sample <= 8u)
                    std::fprintf(stderr, "[gow-model:client] sample=%u state=%u context=%x index=%zu this=%x mask=%x sphereId=%d passesView=%u treeCandidate=%u\n",
                                 sample, state, context, count - 1u, model, modelMask, int(sphereId), passesView ? 1u : 0u,
                                 passesView && useStatic && sphereId >= 0 ? 1u : 0u);
                node = readGuest32(rdram, node);
            }
            std::fprintf(stderr, "[gow-model:context] sample=%u state=%u this=%x view=%x validView=%u id=%x mask=%x head=%x nodes=%zu viewRejected=%zu treeCandidates=%zu processCandidates=%zu staticCount=%u sphereTree=%x sphereValid=%u invalid=%u cycle=%u truncated=%u\n",
                         sample, state, context, view, validView ? 1u : 0u, viewId, mask, head, count,
                         viewRejected, treeCandidates, processCandidates, staticCount, sphereTree, sphereValid,
                         invalid ? 1u : 0u, cycle ? 1u : 0u, node && !invalid && !cycle ? 1u : 0u);
        }
        sub_00159878_0x159878(rdram, ctx, runtime);
    }

    // GOW-Port: real entry of the routine that processes a model (MIPS 0x157A60).
    // Its structure matches ProcessModel from the GoW 2 reference; the retail map doesn't name it.
    void gowDiagProcessModel(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        if (ctx->pc != 0x00157A60u) { sub_00157A60_0x157a60(rdram, ctx, runtime); return; }
        const uint32_t state = readGuest32(rdram, 0x29E560u);
        static unsigned early = 0u, scene = 0u;
        auto &samples = state == 11u ? scene : early;
        if (samples < 64u)
        {
            const uint32_t context = GPR_U32(ctx, 4), model = GPR_U32(ctx, 5);
            const bool valid = guestBuffer(rdram, model, 0xE8u) != nullptr;
            const uint32_t view = readGuest32(rdram, 0x33104Cu);
            const uint32_t object = valid ? readGuest32(rdram, model + 0x18u) : 0u;
            const bool validObject = guestBuffer(rdram, object, 0x108u) != nullptr;
            const uint32_t skeleton = validObject ? readGuest32(rdram, object + 0x104u) : 0u;
            const auto *skeletonData = guestBuffer(rdram, skeleton, 0x90u);
            uint16_t rootId = 0u;
            if (skeletonData) std::memcpy(&rootId, skeletonData + 0x86u, sizeof(rootId));
            const uint32_t visibility = skeletonData ? readGuest32(rdram, skeleton + 0x54u) : 0u;
            const uint64_t rootAddress = (visibility & 0x1FFFFFFFu) + uint64_t(rootId) * 4u;
            const bool validRoot = skeletonData && (!visibility || (rootAddress < 0x02000000u && guestBuffer(rdram, uint32_t(rootAddress), 4u)));
            const uint32_t rootVisible = validRoot ? (!visibility || readGuest32(rdram, uint32_t(rootAddress)) ? 1u : 0u) : 0u;
            std::fprintf(stderr, "[gow-model:process] sample=%u state=%u context=%x this=%x valid=%u caller=%x view=%x groups=%u groupArray=%x disableCulling=%u object=%x skeleton=%x visibility=%x rootId=%u validRoot=%u rootVisible=%u groupBits=%x\n",
                         ++samples, state, context, model, valid ? 1u : 0u, GPR_U32(ctx, 31), view,
                         valid ? readGuest32(rdram, model + 0xE0u) : 0u,
                         valid ? readGuest32(rdram, model + 0xE4u) : 0u, GPR_U32(ctx, 6), object, skeleton,
                         visibility, unsigned(rootId), validRoot ? 1u : 0u, rootVisible,
                         validObject ? readGuest32(rdram, object + 0x100u) : 0u);
        }
        sub_00157A60_0x157a60(rdram, ctx, runtime);
    }

    // GOW-Port: result of the Clip call from ProcessModel (MIPS 0x157FA8).
    // Only watch argument and result bits; don't change the FPU or the game's own criteria.
    void gowDiagModelClip(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        if (ctx->pc != 0x00169120u) { sub_00169120_0x169120(rdram, ctx, runtime); return; }
        const uint32_t caller = GPR_U32(ctx, 31), state = readGuest32(rdram, 0x29E560u);
        static unsigned early = 0u, scene = 0u;
        auto &samples = state == 11u ? scene : early;
        const unsigned sample = caller == 0x00157FB0u && samples < 64u ? ++samples : 0u;
        const uint32_t view = GPR_U32(ctx, 4), model = GPR_U32(ctx, 17), group = GPR_U32(ctx, 19);
        std::array<uint32_t, 4> sphere{};
        std::array<uint32_t, 10> planes{};
        const bool validView = guestBuffer(rdram, view, 0x38Cu) != nullptr;
        if (sample)
        {
            for (size_t i = 0u; i < sphere.size(); ++i) std::memcpy(&sphere[i], &ctx->f[12u + i], 4u);
            const std::array<uint32_t, 10> offsets = {0x384u, 0x388u, 0x2C0u, 0x2C8u, 0x2D4u, 0x2D8u, 0x2E0u, 0x2E8u, 0x2F4u, 0x2F8u};
            if (validView)
                for (size_t i = 0u; i < offsets.size(); ++i) planes[i] = readGuest32(rdram, view + offsets[i]);
        }
        sub_00169120_0x169120(rdram, ctx, runtime);
        if (sample)
            std::fprintf(stderr, "[gow-model:clip] sample=%u state=%u this=%x group=%x view=%x validView=%u sphere=%08x,%08x,%08x,%08x planes=%08x,%08x,%08x,%08x,%08x,%08x,%08x,%08x,%08x,%08x completed=%u result=%x\n",
                         sample, state, model, group, view, validView ? 1u : 0u,
                         sphere[0], sphere[1], sphere[2], sphere[3], planes[0], planes[1], planes[2], planes[3],
                         planes[4], planes[5], planes[6], planes[7], planes[8], planes[9], ctx->pc == caller ? 1u : 0u,
                         ctx->pc == caller ? GPR_U32(ctx, 2) : 0u);
    }

    void gowDiagPrimPoll(uint8_t *rdram, double seconds)
    {
        static double next=0;
        if(seconds<next) return;
        next=seconds+5;
        if(readGuest32(rdram,0x29E560u)!=11u) return;
        for(const auto &p:g_primProbes)
        {
            const auto *data=guestBuffer(rdram,p.address,16);
            if(!data) continue;
            uint32_t words[4]{}; std::memcpy(words,data,16);
            std::fprintf(stderr,"[gow-eeprim:later] seconds=%.2f this=%x caller=%x data=%x first=%08x,%08x,%08x,%08x\n",
                         seconds,p.object,p.caller,p.address,words[0],words[1],words[2],words[3]);
        }
    }

    void gowDiagUpdateAddress(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        if (ctx->pc != 0x001417D8u) { sub_001417D8_0x1417d8(rdram, ctx, runtime); return; }
        const uint32_t object=GPR_U32(ctx,4), type=GPR_U32(ctx,5), offset=GPR_U32(ctx,6);
        const uint32_t chunk=GPR_U32(ctx,7), buffer=GPR_U32(ctx,8), caller=GPR_U32(ctx,31);
        sub_001417D8_0x1417d8(rdram, ctx, runtime);
        static unsigned early=0, scene=0;
        auto &samples=readGuest32(rdram,0x29E560u)==11u ? scene : early;
        if (type != 0u || samples >= (readGuest32(rdram,0x29E560u)==11u ? 256u : 64u)) return;
        ++samples;
        // A checkpoint is not a return: don't interpret v0 as an address in that case.
        if (ctx->pc != caller)
        {
            std::fprintf(stderr,"[gow-eeprim:update] caller=%x this=%x checkpoint=%x\n",caller,object,ctx->pc);
            return;
        }
        const uint32_t address=GPR_U32(ctx,2);
        const auto *data=guestBuffer(rdram,address,16);
        uint32_t words[4]{}; if(data) std::memcpy(words,data,16);
        if(data)
        {
            bool found=false;
            for(const auto &p:g_primProbes) found |= p.address==address;
            if(!found) g_primProbes[g_nextPrimProbe++ % g_primProbes.size()]={object,caller,address};
        }
        std::fprintf(stderr,"[gow-eeprim:update] this=%x caller=%x type=%u offset=%u chunk=%u buffer=%u data=%x valid=%u first=%08x,%08x,%08x,%08x\n",
                     object,caller,type,offset,chunk,buffer,address,data?1u:0u,words[0],words[1],words[2],words[3]);
    }
}
