#ifndef PS2_VU1_H
#define PS2_VU1_H

#include <array>
#include <cmath>
#include <cstring>
#include <limits>
#include <vector>
#include <cstdint>

// GOW-Port: the inline VU1 functions use the same floating-point mode as ps2_vu1_*.cpp (without
// /fp:fast); MSVC does not inline them if the modes do not match.
#if defined(_MSC_VER)
#pragma float_control(precise, on, push)
#pragma fp_contract(off)
#endif

// GOW-Port: small functions that the generated VU1 code must always inline (in large block functions
// MSVC exhausts its inlining budget and calls them).
#if defined(_MSC_VER)
#define VU1_HOT_INLINE __forceinline
#else
#define VU1_HOT_INLINE inline __attribute__((always_inline))
#endif

class GS;
class PS2Memory;

struct VU1State
{
    float vf[32][4];
    int32_t vi[16];
    float acc[4];
    float q;
    float p;
    float i;
    uint32_t r;
    uint32_t pc;
    uint32_t mac;
    uint32_t clip;
    uint32_t status;
    uint64_t cycles;
    bool ebit;
    bool haltAfterDelaySlot;
    bool dBitEnabled;
    bool tBitEnabled;
    bool stoppedByD;
    bool stoppedByT;
    uint32_t top;  // VIF TOP visible to XTOP
    uint32_t itop; // VIF ITOP visible to XITOP

    bool branchPending;
    uint32_t branchTarget;
    uint32_t branchDelay;
};

class VU1Interpreter
{
public:
    enum class Unit : uint8_t
    {
        VU0,
        VU1
    };

    explicit VU1Interpreter(Unit unit = Unit::VU1);

    void reset();

    void execute(uint8_t *vuCode, uint32_t codeSize,
                 uint8_t *vuData, uint32_t dataSize,
                 GS &gs, PS2Memory *memory = nullptr,
                 uint32_t startPC = 0, uint32_t top = 0, uint32_t itop = 0,
                 uint32_t maxCycles = 65536);

    void resume(uint8_t *vuCode, uint32_t codeSize,
                uint8_t *vuData, uint32_t dataSize,
                GS &gs, PS2Memory *memory = nullptr,
                uint32_t top = 0, uint32_t itop = 0, uint32_t maxCycles = 65536);

    // GOW-Port: write VF/VI/ACC on execution instead of queueing them (see m_directRegisterWrites). The
    // program's result does not change, but the intermediate state seen by whoever cuts the execution by
    // budget does; that is why the latency-model tests leave it disabled.
    void setDirectRegisterWrites(bool enabled);

    // GOW-Port: VU1 microcode compiled to C++ (tools/vu1/generate_vu1.cpp). The generated code
    // checks on every pair that the two micro-memory words are the ones it compiled; if not, it interprets
    // that pair. It runs the same steps as the interpreter (stepPair), with each pair already decoded.
    struct StepContext
    {
        uint8_t *vuCode = nullptr;
        uint32_t codeSize = 0;
        uint8_t *vuData = nullptr;
        uint32_t dataSize = 0;
        GS *gs = nullptr;
        PS2Memory *memory = nullptr;
        uint64_t budgetEnd = 0;
        bool programEnded = false;
    };
    using CompiledProgramFn = void (*)(VU1Interpreter &, StepContext &);
    static void registerCompiledProgram(CompiledProgramFn program);
    static uint64_t hashMicrocode(const uint8_t *code, uint32_t size);
    void setCompiledProgramsEnabled(bool enabled) { m_compiledEnabled = enabled; }

    // GOW-Port: last run stopped by budget, without E or D/T.
    bool lastRunHitBudget() const { return m_lastRunHitBudget; }

    VU1State &state() { return m_state; }
    const VU1State &state() const { return m_state; }

private:
    template <int Id>
    friend struct VU1CompiledProgram;
    friend struct VU1CompiledGenerator;
    friend struct VU1CompiledAccess;
    friend struct VU1NativeAccess; // GOW-Port: native (ahead-of-time translated) blocks, see ps2_vu1_native.h

    enum Pipeline : uint8_t
    {
        PipelineNone = 0,
        PipelineFmac,
        PipelineLsu,
        PipelineFdiv,
        PipelineEfu,
        PipelineIalu,
        PipelineBranch,
        PipelineXgkick
    };

    struct VfAccess
    {
        uint8_t reg = 0;
        uint8_t lanes = 0;
    };

    struct InstructionUsage
    {
        std::array<VfAccess, 2> vfRead{};
        VfAccess vfWrite{};
        uint8_t vfReadCount = 0;
        uint16_t viRead = 0;
        uint16_t viWrite = 0;
        uint8_t accRead = 0;
        uint8_t accWrite = 0;
        uint8_t latency = 0;
        uint8_t vfLatency = 0;
        uint8_t viLatency = 0;
        Pipeline pipeline = PipelineNone;
        bool waitQ = false;
        bool waitP = false;
        bool readsClip = false;
        bool writesClip = false;
        bool delaysNextBranchRead = false;
        bool reserved = false;
    };

    struct DecodedInstructionPair
    {
        uint32_t lower = 0;
        uint32_t upper = 0;
        InstructionUsage lowerUsage{};
        InstructionUsage upperUsage{};
        bool iBit = false;
        bool eBit = false;
        bool mBit = false;
        bool dBit = false;
        bool tBit = false;
        uint8_t upperVfShadowReg = 0;
        uint8_t suppressedLowerVf = 0;
        bool upperNop = false; // GOW-Port: upper NOP (no need to call execUpper)
    };

    struct FlagPipelineEntry
    {
        uint64_t readyCycle = 0;
        uint64_t issueCycle = 0;
        uint32_t mac = 0;
        uint32_t status = 0;
        uint32_t extraSticky = 0;
        uint32_t clip = 0;
        bool valid = false;
        bool writesMac = false;
        bool writesStatus = false;
        bool writesSticky = false;
        bool writesClip = false;
    };

    struct ScalarPipelineEntry
    {
        uint64_t readyCycle = 0;
        float value = 0.0f;
        uint32_t statusDi = 0;
        bool valid = false;
    };

    struct PendingStore
    {
        uint64_t readyCycle = 0;
        uint32_t address = 0;
        std::array<uint32_t, 4> words{};
        uint8_t laneMask = 0;
        bool valid = false;
    };

    struct PendingVfWrite
    {
        uint64_t readyCycle = 0;
        uint64_t sequence = 0;
        std::array<float, 4> value{};
        uint8_t reg = 0;
        uint8_t laneMask = 0;
        bool valid = false;
    };

    struct PendingViWrite
    {
        uint64_t readyCycle = 0;
        uint64_t sequence = 0;
        int32_t value = 0;
        uint8_t reg = 0;
        bool valid = false;
    };

    struct PendingAccWrite
    {
        uint64_t readyCycle = 0;
        uint64_t sequence = 0;
        std::array<float, 4> value{};
        uint8_t laneMask = 0;
        bool valid = false;
    };

    struct XgkickPipeline
    {
        static constexpr uint32_t kBufferSize = 0x10000u;
        std::array<uint8_t, kBufferSize> packet{};
        uint32_t sourceAddress = 0;
        uint32_t totalBytes = 0;
        uint32_t copiedBytes = 0;
        uint32_t currentTagEnd = 0;
        uint32_t cycleCredit = 0;
        uint64_t issueCycle = 0;
        bool active = false;
        bool currentTagEop = false;

        // GOW-Port: like *this = {} but without zeroing the 64 KB packet (only bytes already
        // copied are read: the GIFtags in copiedBytes and the send up to totalBytes <= copiedBytes).
        void reset()
        {
            sourceAddress = totalBytes = copiedBytes = currentTagEnd = cycleCredit = 0u;
            issueCycle = 0u;
            active = currentTagEop = false;
        }
    };

    static constexpr uint32_t kFmacLatency = 4u;
    static constexpr uint32_t kAccForwardLatency = 1u;
    static constexpr uint32_t kMaxFlagEntries = 8u;
    static constexpr uint32_t kMaxPendingStores = 8u;
    static constexpr uint32_t kMaxPendingVfWrites = 16u;
    static constexpr uint32_t kMaxPendingViWrites = 8u;
    static constexpr uint32_t kMaxPendingAccWrites = 8u;
    static constexpr uint32_t kMaxDecodedPairs = 0x4000u / 8u;

    Unit m_unit;
    VU1State m_state;
    std::array<DecodedInstructionPair, kMaxDecodedPairs> m_decodedCodeCache{};
    const uint8_t *m_cachedVuCode = nullptr;
    const PS2Memory *m_cachedMemory = nullptr;
    uint32_t m_cachedCodeSize = 0;
    uint64_t m_cachedCodeGeneration = 0;
    bool m_decodedCodeCacheValid = false;
    DecodedInstructionPair m_decodedScratch{}; // GOW-Port: pairs outside the cache (returned by reference)
    // GOW-Port: VU1 writes VF/VI/ACC on execution instead of queueing them with their latency. It is equivalent to the
    // queued model because readers stall until the register is ready (m_vfReady...) and only the last
    // issued write is confirmed. GOW_VU1_QUEUES=1 returns to the queued model.
    bool m_directRegisterWrites = false;
    bool m_compiledEnabled = true;
    uint64_t m_compiledGeneration = ~0ull;
    // GOW-Port: cycle of the next queue entry that expires; commitReadyPipelines does nothing before it.
    uint64_t m_nextCommitCycle = ~0ull;
    // GOW-Port: the loaded microcode does not read the status register (FSAND/FSEQ/FSOR); there is no need to
    // compute the sticky bits of the products (calculateFmacProductSticky).
    bool m_statusUnread = false;
    // GOW-Port: besides, nobody writes the status with FSSET; the compiled code can omit the flags of
    // an FMAC that nobody gets to see (it only accumulates its sticky bits).
    bool m_statusQuiet = false;
    // GOW-Port: GOW_VU1C_VERIFY=1 also runs each compiled block with the interpreter and compares (diagnostic).
    uint32_t m_verifyLeft = 0;
    uint32_t m_verifyHead = 0;
    uint32_t m_verifyPairs = 0;
    uint64_t m_verifyCycle = 0;
    bool m_verifySilent = false; // the interpreter pass does not send XGKICK packets to the GIF
    uint32_t m_verifyKickBytes = 0;
    bool m_verifyKickActive = false;
    std::vector<uint8_t> m_verifyKick;
    VU1State m_verifyExpected{};
    std::vector<uint8_t> m_verifyData;
    void verifyBlockStart(StepContext &context, uint32_t pc, const uint32_t *words, size_t bytes);
    void verifyBlockEnd(StepContext &context);

    std::array<FlagPipelineEntry, kMaxFlagEntries> m_flagPipeline{};
    ScalarPipelineEntry m_fdiv{};
    std::array<ScalarPipelineEntry, 2> m_efu{};
    std::array<PendingStore, kMaxPendingStores> m_storePipeline{};
    std::array<PendingVfWrite, kMaxPendingVfWrites> m_vfWritePipeline{};
    std::array<PendingViWrite, kMaxPendingViWrites> m_viWritePipeline{};
    std::array<PendingAccWrite, kMaxPendingAccWrites> m_accWritePipeline{};
    // GOW-Port: bits of the valid entries of each queue; commitReadyPipelines only walks those
    // entries (before it walked all 48 on every cycle, ~30 % of VU1 time).
    uint32_t m_storePending = 0u, m_vfPending = 0u, m_viPending = 0u, m_accPending = 0u;
    // GOW-Port: the flags (MAC/status/clip) all have the same latency, so they are committed in the order
    // they are issued: a circular queue (head and entry count) instead of searching for gaps.
    uint32_t m_flagHead = 0u, m_flagCount = 0u;
    XgkickPipeline m_xgkick{};
    std::array<std::array<uint64_t, 4>, 32> m_vfReady{};
    std::array<uint64_t, 16> m_viReady{};
    std::array<uint64_t, 4> m_accReady{};
    std::array<std::array<uint64_t, 4>, 32> m_vfLatestWrite{};
    std::array<uint64_t, 16> m_viLatestWrite{};
    std::array<uint64_t, 4> m_accLatestWrite{};

    uint64_t m_cycle = 0;
    uint64_t m_nextWriteSequence = 0;
    uint64_t m_efuResourceReady = 0;
    uint32_t m_workingClip = 0;
    uint32_t m_currentUpperInstruction = 0;
    int32_t m_viBranchBackupValue = 0;
    uint8_t m_viBranchBackupReg = 0;
    bool m_viBranchBackupValid = false;
    uint8_t *m_activeVuData = nullptr;
    uint32_t m_activeVuDataSize = 0;
    GS *m_activeGs = nullptr;
    PS2Memory *m_activeMemory = nullptr;
    bool m_stopRequested = false;
    bool m_pendingHaltD = false;
    bool m_pendingHaltT = false;
    bool m_lastRunHitBudget = false; // GOW-Port: PR #12
    // GOW-Port: native VU1 backend. runNative executes translated basic blocks with immediate (timing-free) semantics;
    // it returns when the program ended or when the current pc has no native block (the caller then steps the exact
    // engine). commitEverythingNow retires every in-flight pipeline entry without advancing the cycle counter.
    struct NativeState;
    NativeState *m_native = nullptr;
    bool runNative(StepContext &context);
    void nativeTiming(const uint8_t *code, uint32_t pc, uint32_t count, std::vector<uint8_t> &cyc);
    void commitEverythingNow();

    void run(uint8_t *vuCode, uint32_t codeSize,
             uint8_t *vuData, uint32_t dataSize,
             GS &gs, PS2Memory *memory, uint32_t maxCycles);

    InstructionUsage decodeUpperUsage(uint32_t upper) const;
    InstructionUsage decodeLowerUsage(uint32_t lower) const;
    static void addVfRead(InstructionUsage &usage, uint8_t reg, uint8_t lanes);
    static void addVfWrite(InstructionUsage &usage, uint8_t reg, uint8_t lanes);
    static uint8_t vfReadLanes(const InstructionUsage &usage, uint8_t reg);
    DecodedInstructionPair decodeInstructionPair(const uint8_t *vuCode, uint32_t pc) const;
    const DecodedInstructionPair &getDecodedInstructionPairForPc(const uint8_t *vuCode, uint32_t codeSize, PS2Memory *memory, uint32_t pc);
    void rebuildDecodedCodeCache(const uint8_t *vuCode, uint32_t codeSize, const PS2Memory *memory, uint64_t generation);

    void execUpper(uint32_t instr);
    void execLower(uint32_t instr, uint8_t *vuData, uint32_t dataSize, GS &gs, PS2Memory *memory, uint32_t upperInstr);

    void applyDest(float *dst, const float *result, uint8_t dest);
    void applyDestAcc(const float *result, uint8_t dest);
    void applyFmacDest(float *dst, float *result, uint8_t dest);
    void applyFmacDestAcc(float *result, uint8_t dest);
    void normalizeFmacResult(float *result, uint8_t dest, uint8_t laneFlags[4]);
    bool calculateFmacExactResult(uint32_t component, long double &result) const;
    uint8_t normalizeFmacExactResult(float &value, long double exactResult) const;
    uint32_t calculateFmacProductSticky(uint8_t dest) const;
    void updateFmacFlags(const uint8_t laneFlags[4], uint8_t dest, uint32_t extraSticky);
    // GOW-Port: same as updateFmacFlags with MAC and status already computed (compiled VU1 code).
    void pushFmacFlags(uint32_t mac, uint32_t status, uint32_t extraSticky);
    FlagPipelineEntry *pushFlagEntry();
    void queueFsset(uint16_t immediate);
    void queueClip(uint32_t clip);
    void queueFcset(uint32_t clip);
    void queueQ(float value, uint32_t latency, uint32_t statusDi);
    void queueP(float value, uint32_t latency);
    void queueStore(uint32_t address, const uint32_t words[4], uint8_t laneMask);
    void queueVfWrite(uint8_t reg, uint8_t laneMask, const float value[4], uint32_t latency);
    void queueViWrite(uint8_t reg, int32_t value, uint32_t latency);
    void queueAccWrite(uint8_t laneMask, const float value[4], uint32_t latency);
    void startXgkick(uint32_t qwordAddress);

    void resetScheduler();
    void commitReadyPipelines();
    // GOW-Port: the MAC/status/clip flags are committed when somebody reads them (FS*/FM*/FC*), at the end of a
    // run or when commitReadyPipelines does it for another queue, applying the entries already ready in this
    // cycle in order: the result is the same as committing them in their cycle, without calling
    // commitReadyPipelines on every cycle of FMAC code.
    void commitReadyFlags();
    void advanceOneCycle();
    void advanceTo(uint64_t targetCycle);
    void flushPipelines();
    void progressXgkick();
    void finishXgkick();
    uint64_t calculatePairReadyCycle(const DecodedInstructionPair &decoded) const;
    void markPairWrites(const DecodedInstructionPair &decoded);
    bool stepPair(const DecodedInstructionPair &decoded, StepContext &context);
    // GOW-Port: specialized versions for the compiled microcode (ps2_vu1_compiled.inl).
    template <uint32_t Upper, uint32_t C>
    void fmacLaneT(float &result, uint8_t &laneFlags, uint8_t &productFlags);
    template <uint32_t Upper, bool DeadFlags>
    void fmacScalarT();
    template <uint32_t Upper, bool DeadFlags>
    void upperT();
    template <uint32_t Lower>
    void lowerT(StepContext &context, uint32_t upperInstr);
    template <class T>
    uint64_t readyCycleT() const;
    template <class T, bool DeadFlags, bool Plain = false>
    bool stepPairT(StepContext &context);
    bool blockPreconditions(StepContext &context, uint32_t pc, const uint32_t *words, size_t bytes) const;
    bool stepInterpreted(StepContext &context);
    bool compiledPrologue(StepContext &context);
    CompiledProgramFn compiledProgramFor(uint8_t *vuCode, uint32_t codeSize, PS2Memory *memory);
    bool pipelinesPending() const;

    // GOW-Port: inline; it is called several times per instruction (~9 % of VU1 time as a call).
    VU1_HOT_INLINE float normalizeOperand(float value) const
    {
        uint32_t bits = 0;
        std::memcpy(&bits, &value, sizeof(bits));
        const uint32_t exponent = (bits >> 23) & 0xFFu;
        if (exponent == 0u)
            bits &= 0x80000000u;
        else if (exponent == 0xFFu)
            bits = (bits & 0x80000000u) | 0x7F7FFFFFu;
        std::memcpy(&value, &bits, sizeof(value));
        return value;
    }
    // GOW-Port: shortcut of normalizeFmacResult for acc +/- product. With the product and the sum rounded
    // separately, |exact - result| < 2^-23 (|product| + |result|). If the result is normal,
    // is not near the extremes (exponent 2..0xFD) and is not much smaller than the product (2^-20), the
    // exact one has the same sign, is not zero and does not overflow: the value does not change and the flags are the sign.
    static VU1_HOT_INLINE bool productSumFlagsFast(float result, float product, uint8_t &laneFlags)
    {
        uint32_t r = 0u, p = 0u;
        std::memcpy(&r, &result, sizeof(r));
        std::memcpy(&p, &product, sizeof(p));
        const int er = static_cast<int>((r >> 23) & 0xFFu);
        const int ep = static_cast<int>((p >> 23) & 0xFFu);
        if (er < 2 || er > 0xFD || er < ep - 20)
            return false;
        laneFlags = (r & 0x80000000u) != 0u ? 0x2u : 0u;
        return true;
    }
    // GOW-Port: Z/S/U/O flags of the exact product a*b (the same as normalizeFmacExactResult on
    // a*b in long double). The product of two floats fits exactly in a double. If the already
    // rounded product is normal and far from the extremes (exponent 2..0xFD), the exact one is not zero, does not overflow
    // or underflow and has its sign: the flags are only the sign.
    static VU1_HOT_INLINE uint8_t productStickyFlags(float a, float b, float rounded)
    {
        uint32_t bits = 0u;
        std::memcpy(&bits, &rounded, sizeof(bits));
        const uint32_t exponent = (bits >> 23) & 0xFFu;
        if (exponent >= 2u && exponent <= 0xFDu)
            return (bits & 0x80000000u) != 0u ? 0x2u : 0u;
        // GOW-Port: with a zero factor (the operands arrive normalized: finite, without subnormals) the
        // exact product is zero with the sign of a XOR b, the same as productStickyFlagsExact.
        uint32_t aBits = 0u, bBits = 0u;
        std::memcpy(&aBits, &a, sizeof(aBits));
        std::memcpy(&bBits, &b, sizeof(bBits));
        if ((aBits & 0x7FFFFFFFu) == 0u || (bBits & 0x7FFFFFFFu) == 0u)
            return ((aBits ^ bBits) & 0x80000000u) != 0u ? 0x3u : 0x1u;
        return productStickyFlagsExact(a, b);
    }
    static uint8_t productStickyFlagsExact(float a, float b)
    {
        const double exact = static_cast<double>(a) * static_cast<double>(b);
        const double magnitude = std::fabs(exact);
        uint8_t flags = std::signbit(exact) ? 0x2u : 0u;
        if (magnitude == 0.0)
            flags |= 0x1u;
        else if (magnitude > static_cast<double>(std::numeric_limits<float>::max()))
            flags |= 0x8u;
        else if (magnitude < static_cast<double>(std::numeric_limits<float>::min()))
            flags |= 0x5u;
        return flags;
    }
    float normalizeResult(float value, uint32_t &laneFlags) const;
    uint32_t microAddressMask() const { return m_unit == Unit::VU1 ? 0x3FFFu : 0x0FFFu; }
    int32_t readBranchVi(uint8_t reg) const;
    void recordViWriteForBranch(uint8_t reg, int32_t oldValue);
    void reportReservedInstruction(bool upper, uint32_t instruction);
    float broadcast(const float *vf, uint8_t bc);
};

#if defined(_MSC_VER)
#pragma float_control(pop)
#endif

#endif
