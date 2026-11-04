// GOW-Port: native display and GS-status service (see runtime/gs/gs_display.h).
#include "runtime/gs/gs_display.h"
#include "runtime/ps2_memory.h"

namespace ps2_gs_display
{
    namespace
    {
        // Only used to attribute the GS worker barrier (PS2X_GS_WORKER_DIAG) like the register accesses did.
        constexpr uint32_t kDisplaySite = 0x12000000u;
        constexpr uint32_t kStatusSite = 0x12001000u;
        constexpr uint64_t kFinish = 0x2ull;
    }

    void setDisplay(PS2Memory &memory, const GSDisplayState &state)
    {
        if (memory.gsEarlyFinish())
        {
            // GOW-Port: early FINISH: the GS stage applies them once everything queued before them is drawn.
            const uint64_t regs[7] = {state.pmode, state.smode2, state.dispfb1, state.display1, state.dispfb2, state.display2,
                                      state.bgcolor};
            memory.gsQueueDisplay(regs);
            return;
        }
        memory.syncGsWorkerFor(kDisplaySite);
        GSRegisters &gs = memory.gs();
        gs.pmode = state.pmode;
        gs.smode2 = state.smode2;
        gs.dispfb1 = state.dispfb1;
        gs.display1 = state.display1;
        gs.dispfb2 = state.dispfb2;
        gs.display2 = state.display2;
        gs.bgcolor = state.bgcolor;
    }

    GSDisplayState display(const PS2Memory &memory)
    {
        GSDisplayState state;
        uint64_t queued[7];
        if (memory.gsEarlyFinish() && memory.gsDisplayShadow(queued))
        {
            // GOW-Port: what the game wrote last, which the drawing may not have reached yet.
            state.pmode = queued[0];
            state.smode2 = queued[1];
            state.dispfb1 = queued[2];
            state.display1 = queued[3];
            state.dispfb2 = queued[4];
            state.display2 = queued[5];
            state.bgcolor = queued[6];
            return state;
        }
        const GSRegisters &gs = memory.gs();
        state.pmode = gs.pmode;
        state.smode2 = gs.smode2;
        state.dispfb1 = gs.dispfb1;
        state.display1 = gs.display1;
        state.dispfb2 = gs.dispfb2;
        state.display2 = gs.display2;
        state.bgcolor = gs.bgcolor;
        return state;
    }

    uint64_t status(PS2Memory &memory)
    {
        memory.syncGsWorkerFor(kStatusSite);
        return memory.gs().csr.load(std::memory_order_acquire);
    }

    bool finishPending(PS2Memory &memory)
    {
        memory.syncGsWorkerFor(kStatusSite);
        return (memory.gs().csr.load(std::memory_order_acquire) & kFinish) != 0u;
    }

    void ackFinish(PS2Memory &memory)
    {
        memory.syncGsWorkerFor(kStatusSite);
        memory.gs().csr.fetch_and(~kFinish, std::memory_order_acq_rel);
    }
}
