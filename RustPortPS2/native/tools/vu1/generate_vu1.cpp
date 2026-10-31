// GOW-Port: generates C++ for the VU1 microcode captured with GOW_VU1_CAPTURE (16 KB micro-memories).
// Usage: generate_vu1 <output folder> <microcode.bin>...  (writes program0.cpp ... program12.cpp)
//
// Each instruction pair is written already decoded (as the interpreter decodes it) and executed with
// VU1Interpreter::stepPairT, which does the same steps as the interpreter; the dispatcher checks on each
// pair that the two micro-memory words are the compiled ones and, if not, interprets that pair.
//
// Blocks: from each header (jump target or pair following a block end) up to the delay slot of
// the first jump, the pair following an E bit or a pair with a D/T bit. A block is only used if all its
// words match, there is no pending jump and plenty of cycle budget remains. Within a
// block what comes after each pair is known, so the MAC/status flags of an
// FMAC that a later FMAC in the block replaces before anyone reads them can be omitted (liveness analysis).
//
// The micro-memory and the generated C++ come from the game's data: they are not published.
#include "runtime/ps2_memory.h"
#include "runtime/ps2_vu1.h"

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <algorithm>
#include <cerrno>
#include <climits>
#include <fstream>
#include <iostream>
#include <map>
#include <set>
#include <sstream>
#include <string>
#include <tuple>
#include <vector>

struct VU1CompiledGenerator
{
    using Pair = VU1Interpreter::DecodedInstructionPair;
    using Usage = VU1Interpreter::InstructionUsage;

    static Pair decode(const VU1Interpreter &vu, const uint8_t *code, uint32_t pc)
    {
        return vu.decodeInstructionPair(code, pc);
    }

    static bool isBranch(const Pair &p)
    {
        return !p.iBit && p.lowerUsage.pipeline == VU1Interpreter::PipelineBranch;
    }

    static void usage(std::ostringstream &out, const char *name, const Usage &u)
    {
        const Usage d{};
        for (int i = 0; i < 2; ++i)
            if (u.vfRead[i].reg != 0 || u.vfRead[i].lanes != 0)
                out << " p." << name << ".vfRead[" << i << "] = {" << int(u.vfRead[i].reg) << ", " << int(u.vfRead[i].lanes) << "};";
        if (u.vfWrite.reg != 0 || u.vfWrite.lanes != 0)
            out << " p." << name << ".vfWrite = {" << int(u.vfWrite.reg) << ", " << int(u.vfWrite.lanes) << "};";
#define FIELD(f)          \
    if (u.f != d.f)       \
        out << " p." << name << "." #f " = " << int(u.f) << ";";
        FIELD(vfReadCount)
        FIELD(viRead)
        FIELD(viWrite)
        FIELD(accRead)
        FIELD(accWrite)
        FIELD(latency)
        FIELD(vfLatency)
        FIELD(viLatency)
        FIELD(waitQ)
        FIELD(waitP)
        FIELD(readsClip)
        FIELD(writesClip)
        FIELD(delaysNextBranchRead)
        FIELD(reserved)
#undef FIELD
        if (u.pipeline != d.pipeline)
            out << " p." << name << ".pipeline = static_cast<VU1CompiledAccess::Pipeline>(" << int(u.pipeline) << ");";
    }

    static std::string pairInitializer(const Pair &p)
    {
        std::ostringstream out;
        out << "[] { P p{}; p.lower = 0x" << std::hex << p.lower << "u; p.upper = 0x" << p.upper << "u;" << std::dec;
        usage(out, "lowerUsage", p.lowerUsage);
        usage(out, "upperUsage", p.upperUsage);
#define FLAG(f)   \
    if (p.f)      \
        out << " p." #f " = true;";
        FLAG(iBit)
        FLAG(eBit)
        FLAG(mBit)
        FLAG(dBit)
        FLAG(tBit)
        FLAG(upperNop)
#undef FLAG
        if (p.upperVfShadowReg)
            out << " p.upperVfShadowReg = " << int(p.upperVfShadowReg) << ";";
        if (p.suppressedLowerVf)
            out << " p.suppressedLowerVf = " << int(p.suppressedLowerVf) << ";";
        out << " return p; }()";
        return out.str();
    }
};

namespace
{
    using Pair = VU1CompiledGenerator::Pair;

    // FMACs that leave MAC/status flags in the queue (the same ones upperT handles with updateFmacFlags).
    bool writesFmacFlags(uint32_t upper)
    {
        const uint32_t op = upper & 0x3Fu;
        const uint32_t dest = (upper >> 21) & 0xFu;
        if (dest == 0u)
            return false;
        if (op < 0x3Cu)
        {
            if (op <= 0x0Fu || (op >= 0x18u && op <= 0x1Cu) || op == 0x1Eu || (op >= 0x20u && op <= 0x2Au) ||
                op == 0x2Cu || op == 0x2Du || op == 0x2Eu)
                return true;
            return false;
        }
        const uint32_t special = (upper & 3u) | ((upper >> 4) & 0x7Cu);
        return special <= 0x0Fu || (special >= 0x18u && special <= 0x1Cu) || special == 0x1Eu ||
               (special >= 0x20u && special <= 0x2Au) || special == 0x2Cu || special == 0x2Du || special == 0x2Eu;
    }

    // FMAND (0x1A), FMEQ (0x18) and FMOR (0x1B) read the MAC register.
    bool readsMac(const Pair &p)
    {
        if (p.iBit)
            return false;
        const uint32_t op = p.lower >> 25;
        return op == 0x18u || op == 0x1Au || op == 0x1Bu;
    }

    struct Code
    {
        std::vector<uint8_t> bytes;
        std::vector<Pair> pairs;
        std::vector<bool> valid;
        uint32_t lower(uint32_t pc) const { uint32_t w; std::memcpy(&w, bytes.data() + pc, 4); return w; }
        uint32_t upper(uint32_t pc) const { uint32_t w; std::memcpy(&w, bytes.data() + pc + 4, 4); return w; }
    };

    std::string hex4(uint32_t v)
    {
        char b[16];
        std::snprintf(b, sizeof(b), "%04X", v);
        return b;
    }
}

int main(int argc, char **argv)
{
    if (argc < 3)
    {
        std::cerr << "Usage: generate_vu1 <output folder> [--max-memory-mb N] <micromemory.bin | folder>...\n";
        return 2;
    }
    // GOW-Port: this tool used to retain a decoded copy of every recorded micro-memory. A long playthrough can
    // contain hundreds of thousands of captures, which made the installer consume the whole machine. Keep a
    // deliberately conservative fraction of the requested budget for captures; the remaining headroom is for the
    // generated source, maps and the C++ runtime. Zero keeps the old unlimited developer behaviour.
    uint32_t maxMemoryMiB = 0;
    int inputStart = 2;
    if (std::strcmp(argv[inputStart], "--max-memory-mb") == 0)
    {
        if (inputStart + 2 >= argc)
        {
            std::cerr << "--max-memory-mb needs a positive integer\n";
            return 2;
        }
        char *end = nullptr;
        errno = 0;
        const unsigned long parsed = std::strtoul(argv[inputStart + 1], &end, 10);
        if (errno != 0 || !end || *end != '\0' || parsed == 0 || parsed > UINT_MAX)
        {
            std::cerr << "--max-memory-mb needs a positive integer\n";
            return 2;
        }
        maxMemoryMiB = static_cast<uint32_t>(parsed);
        inputStart += 2;
    }
    if (inputStart >= argc)
    {
        std::cerr << "No VU1 micro-memory input was supplied\n";
        return 2;
    }
    // GOW-Port: a folder counts as all its micro-memories (*.bin, by name). Thousands of paths do not fit
    // in Windows' command line (32 K characters).
    std::vector<std::string> inputs;
    for (int a = inputStart; a < argc; ++a)
    {
        if (std::filesystem::is_directory(argv[a]))
        {
            std::vector<std::string> files;
            for (const auto &entry : std::filesystem::directory_iterator(argv[a]))
                if (entry.is_regular_file() && entry.path().extension() == ".bin")
                    files.push_back(entry.path().string());
            std::sort(files.begin(), files.end());
            inputs.insert(inputs.end(), files.begin(), files.end());
        }
        else
            inputs.push_back(argv[a]);
    }
    constexpr size_t kCaptureBudgetBytes = 1024u * 1024u;
    size_t selectedCount = inputs.size();
    if (maxMemoryMiB != 0)
    {
        const size_t limit = std::max<size_t>(1u, (static_cast<size_t>(maxMemoryMiB) * 1024u * 1024u) / kCaptureBudgetBytes);
        selectedCount = std::min(inputs.size(), limit);
        if (selectedCount < inputs.size())
            std::cerr << "[generate_vu1] RAM budget " << maxMemoryMiB << " MiB: sampling " << selectedCount << " of "
                      << inputs.size() << " captured micro-memories\n";
    }
    VU1Interpreter vu;
    std::vector<Code> codes;
    codes.reserve(selectedCount);
    for (size_t selected = 0; selected < selectedCount; ++selected)
    {
        // Spread the bounded set across the full session. Taking only the first files would overfit to the opening
        // scene and leave later areas interpreted.
        const size_t inputIndex = selectedCount == inputs.size() ? selected : (selected * inputs.size()) / selectedCount;
        const std::string &input = inputs[inputIndex];
        std::ifstream file(input, std::ios::binary);
        Code code;
        code.bytes.assign((std::istreambuf_iterator<char>(file)), {});
        if (code.bytes.size() != PS2_VU1_CODE_SIZE)
        {
            std::cerr << input << ": the VU1 micro-memory must be 16384 bytes\n";
            return 1;
        }
        const uint32_t count = static_cast<uint32_t>(code.bytes.size() / 8u);
        code.pairs.resize(count);
        code.valid.resize(count);
        for (uint32_t i = 0; i < count; ++i)
        {
            const uint32_t pc = i * 8u;
            code.pairs[i] = VU1CompiledGenerator::decode(vu, code.bytes.data(), pc);
            code.valid[i] = !(code.lower(pc) == 0u && code.upper(pc) == 0u) &&
                            !code.pairs[i].upperUsage.reserved && !code.pairs[i].lowerUsage.reserved;
        }
        codes.push_back(std::move(code));
    }
    std::cerr << "[generate_vu1] using " << codes.size() << " captured micro-memories\n";

    // Functions per pair: (pc, words, dead flags) -> name.
    std::map<std::tuple<uint32_t, uint32_t, uint32_t, bool>, std::string> pairFns;
    std::map<uint32_t, uint32_t> variantCount;
    constexpr size_t kParts = 12;
    std::vector<std::ostringstream> parts(kParts);
    std::ostringstream declarations;
    // Decoded pair (pc, words) -> name of the K struct; each file defines the ones it uses.
    std::map<std::tuple<uint32_t, uint32_t, uint32_t>, std::string> kNames;
    std::vector<std::set<std::string>> kInPart(kParts);
    const auto kStruct = [&](const Code &code, uint32_t pc, size_t part) -> std::string
    {
        const auto key = std::make_tuple(pc, code.lower(pc), code.upper(pc));
        auto it = kNames.find(key);
        if (it == kNames.end())
            it = kNames.emplace(key, "K" + hex4(pc) + "_" + std::to_string(variantCount[pc]++)).first;
        if (kInPart[part].insert(it->second).second)
            parts[part] << "    struct " << it->second << "\n    {\n        static constexpr P value = "
                        << VU1CompiledGenerator::pairInitializer(code.pairs[pc / 8u]) << ";\n    };\n";
        return it->second;
    };
    const auto pairFunction = [&](const Code &code, uint32_t pc, bool dead) -> std::string
    {
        const auto key = std::make_tuple(pc, code.lower(pc), code.upper(pc), dead);
        if (const auto it = pairFns.find(key); it != pairFns.end())
            return it->second;
        const size_t part = pairFns.size() % kParts;
        const std::string k = kStruct(code, pc, part);
        const std::string name = "p" + k.substr(1) + (dead ? "d" : "");
        parts[part] << "    VU1C_FAST bool " << name << "(VU1Interpreter &vu, C &c)\n    {\n"
                    << "        return VU1CompiledAccess::step<" << k << ", " << (dead ? "true" : "false") << ">(vu, c);\n    }\n";
        declarations << "    bool " << name << "(VU1Interpreter &vu, C &c);\n";
        pairFns[key] = name;
        return name;
    };

    // Pair-by-pair dispatch (no dead flags) for all the words seen.
    std::map<uint32_t, std::vector<std::string>> pairCases;
    std::set<std::tuple<uint32_t, uint32_t, uint32_t>> seenPairs;
    for (const Code &code : codes)
        for (uint32_t pc = 0; pc + 8u <= code.bytes.size(); pc += 8u)
        {
            if (!code.valid[pc / 8u] || !seenPairs.insert({pc, code.lower(pc), code.upper(pc)}).second)
                continue;
            const std::string name = pairFunction(code, pc, false);
            std::ostringstream test;
            test << "                if (lower == 0x" << std::hex << code.lower(pc) << "u && upper == 0x" << code.upper(pc)
                 << "u && inRange(pc))\n" << std::dec
                 << "                {\n                    ++g_stats.compiled;\n                    if (!" << name << "(vu, c))\n"
                 << "                        return;\n                    continue;\n                }\n";
            pairCases[pc].push_back(test.str());
        }

    // Bloques.
    // Test of each block in the dispatch and its successors known at generation time (to chain without the switch).
    struct BlockCase
    {
        std::string test;
        std::vector<uint32_t> successors;
    };
    std::map<uint32_t, std::vector<BlockCase>> blockCases;
    std::set<std::pair<uint32_t, std::vector<uint32_t>>> seenBlocks;
    size_t blockCount = 0, deadCount = 0, fmacCount = 0;
    for (const Code &code : codes)
    {
        const uint32_t count = static_cast<uint32_t>(code.bytes.size() / 8u);
        std::set<uint32_t> heads;
        for (uint32_t i = 0; i < count; ++i)
        {
            if (!code.valid[i])
                continue;
            const Pair &p = code.pairs[i];
            const uint32_t pc = i * 8u;
            if (i == 0u || !code.valid[i - 1u])
                heads.insert(pc);
            if (VU1CompiledGenerator::isBranch(p))
            {
                const uint32_t op = p.lower >> 25;
                if (op != 0x24u && op != 0x25u) // JR/JALR: target in a register
                {
                    const int32_t imm = static_cast<int32_t>(p.lower << 21) >> 21;
                    heads.insert(static_cast<uint32_t>(static_cast<int32_t>(pc) + 8 + imm * 8) & 0x3FFFu);
                }
                heads.insert((pc + 16u) & 0x3FFFu); // after the delay slot
            }
            if (p.eBit)
                heads.insert((pc + 16u) & 0x3FFFu);
            if (p.dBit || p.tBit)
                heads.insert((pc + 8u) & 0x3FFFu);
        }
        for (const uint32_t head : heads)
        {
            if (head + 8u > code.bytes.size() || !code.valid[head / 8u])
                continue;
            std::vector<uint32_t> pcs;
            for (uint32_t pc = head; pc + 8u <= code.bytes.size() && pcs.size() < 64u; pc += 8u)
            {
                if (!code.valid[pc / 8u])
                    break;
                pcs.push_back(pc);
                const Pair &p = code.pairs[pc / 8u];
                if (p.dBit || p.tBit)
                    break;
                if (VU1CompiledGenerator::isBranch(p) || p.eBit)
                {
                    if (pc + 16u <= code.bytes.size() && code.valid[pc / 8u + 1u])
                        pcs.push_back(pc + 8u);
                    break;
                }
                // A jump in the previous pair (if entered mid-way) is covered by the pending-jump check.
            }
            if (pcs.size() < 2u)
                continue;
            std::vector<uint32_t> words;
            for (const uint32_t pc : pcs)
            {
                words.push_back(code.lower(pc));
                words.push_back(code.upper(pc));
            }
            if (!seenBlocks.insert({head, words}).second)
                continue;

            // Liveness of each FMAC's flags in the block.
            const size_t length = pcs.size();
            std::vector<bool> dead(length, false);
            for (size_t i = 0; i < length; ++i)
            {
                if (!writesFmacFlags(code.upper(pcs[i])))
                    continue;
                ++fmacCount;
                for (size_t j = i + 1u; j + 4u <= length; ++j)
                {
                    if (readsMac(code.pairs[pcs[j] / 8u]))
                        break; // a reader before the next FMAC: live
                    if (!writesFmacFlags(code.upper(pcs[j])))
                        continue;
                    bool readerSoon = false;
                    for (size_t r = j + 1u; r <= j + 3u && r < length; ++r)
                        readerSoon = readerSoon || readsMac(code.pairs[pcs[r] / 8u]);
                    if (!readerSoon)
                        dead[i] = true;
                    break;
                }
                deadCount += dead[i] ? 1u : 0u;
            }

            // The block is a single function with the steps inline, in the same file as its K structs.
            // Intermediate pairs that neither jump nor end ("simple") do not check jumps or ends.
            const std::string name = "b" + hex4(head) + "_" + std::to_string(blockCases[head].size());
            const size_t part = blockCount % kParts;
            std::ostringstream body;
            body << "    VU1C_FAST int " << name << "(VU1Interpreter &vu, C &c)\n    {\n"
                 << "        static constexpr uint32_t words[" << words.size() << "] = {";
            for (size_t w = 0; w < words.size(); ++w)
                body << (w ? ", " : "") << "0x" << std::hex << words[w] << "u" << std::dec;
            body << "};\n"
                 << "        if (!VU1CompiledAccess::block(vu, c, " << head << "u, words, sizeof(words)))\n            return 0;\n"
                 << "        g_blockPairs += " << length << "u;\n";
            for (size_t i = 0; i < length; ++i)
            {
                const Pair &p = code.pairs[pcs[i] / 8u];
                const bool plain = i + 1u < length && !VU1CompiledGenerator::isBranch(p) && !p.eBit && !p.dBit && !p.tBit;
                const std::string k = kStruct(code, pcs[i], part);
                body << "        if (!VU1CompiledAccess::step<" << k << ", " << (dead[i] ? "true" : "false") << ", "
                     << (plain ? "true" : "false") << ">(vu, c))\n            return 2;\n";
                if (i + 1u < length && !plain)
                    body << "        if (VU1CompiledAccess::pc(vu) != " << pcs[i + 1u] << "u)\n            return 1;\n";
            }
            body << "        return 1;\n    }\n";
            parts[part] << body.str();
            declarations << "    int " << name << "(VU1Interpreter &vu, C &c);\n";
            std::ostringstream test;
            test << "                if (inRange(pc))\n                {\n                    const int r = " << name << "(vu, c);\n"
                 << "                    if (r == 2)\n                        return;\n                    if (r == 1)\n";
            // Successors: after the last pair the next one follows; if the block ends in a jump with its delay
            // slot, also the target (JR/JALR take it from a register: only the next one).
            std::vector<uint32_t> successors{(pcs.back() + 8u) & 0x3FFFu};
            if (pcs.size() >= 2u)
            {
                const Pair &b = code.pairs[pcs[pcs.size() - 2u] / 8u];
                const uint32_t op = b.lower >> 25;
                if (VU1CompiledGenerator::isBranch(b) && op != 0x24u && op != 0x25u)
                {
                    const int32_t imm = static_cast<int32_t>(b.lower << 21) >> 21;
                    const uint32_t target = static_cast<uint32_t>(static_cast<int32_t>(pcs[pcs.size() - 2u]) + 8 + imm * 8) & 0x3FFFu;
                    if (target != successors[0])
                        successors.push_back(target);
                }
            }
            blockCases[head].push_back({test.str(), successors});
            ++blockCount;
        }
    }

    const std::string header =
        "// Generated by tools/vu1/generate_vu1.cpp from the game's microcode. Do not publish.\n"
        "#include \"runtime/ps2_vu1.h\"\n#include \"ps2_vu1_compiled.inl\"\n#include <cstdio>\n#include <cstdlib>\n#include <cstring>\n\n"
        "// Same floating-point mode as the VU interpreter (MSVC does not inline across different modes).\n"
        "#if defined(_MSC_VER)\n#pragma float_control(precise, on, push)\n#pragma fp_contract(off)\n#endif\n\n"
        "using P = VU1CompiledAccess::P;\nusing C = VU1CompiledAccess::C;\n\n"
        "namespace vu1c_gen\n{\n    inline unsigned long long g_blockPairs = 0;\n}\n\n"
        "// No stack overflow checks (/GS): the local arrays have a fixed size.\n"
        "#if defined(_MSC_VER)\n#define VU1C_FAST __declspec(safebuffers)\n#else\n#define VU1C_FAST\n#endif\n\n";
    const std::string footer = "\n#if defined(_MSC_VER)\n#pragma float_control(pop)\n#endif\n";
    const std::string base = argv[1];
    // A file is only rewritten if it changes: that way the build (Ninja with restat) does not recompile the
    // generated code every time the generator is relinked.
    const auto writeIfChanged = [](const std::string &path, const std::string &text)
    {
        std::ifstream in(path, std::ios::binary);
        const std::string old((std::istreambuf_iterator<char>(in)), {});
        const bool existed = in.is_open();
        in.close();
        if (!existed || old != text)
            std::ofstream(path, std::ios::binary) << text;
    };
    for (size_t k = 0; k < kParts; ++k)
        writeIfChanged(base + "/program" + std::to_string(k + 1u) + ".cpp",
                       header + "namespace vu1c_gen\n{\n" + parts[k].str() + "}\n" + footer);

    std::ostringstream out;
    out << header
        << "namespace\n{\n"
        << "    // GOW_VU1C_DIAG: compiled and interpreted pairs on exit. GOW_VU1C_RANGE=start-end (hex): only uses the\n"
        << "    // compiled code in that address range (to narrow down a difference with the interpreter).\n"
        << "    // GOW_VU1C_NO_BLOCKS: no blocks (only single pairs).\n"
        << "    struct Stats\n    {\n        unsigned long long compiled = 0, interpreted = 0;\n"
        << "        ~Stats()\n        {\n            if (std::getenv(\"GOW_VU1C_DIAG\"))\n"
        << "                std::fprintf(stderr, \"[vu1c] compiled=%llu interpreted=%llu\\n\", compiled + vu1c_gen::g_blockPairs, interpreted);\n        }\n    } g_stats;\n"
        << "    struct Range\n    {\n        uint32_t lo = 0, hi = 0xFFFFFFFFu;\n        bool blocks = true;\n        Range()\n        {\n"
        << "            if (const char *v = std::getenv(\"GOW_VU1C_RANGE\"))\n            {\n"
        << "                char *end = nullptr;\n                lo = static_cast<uint32_t>(std::strtoul(v, &end, 16));\n"
        << "                hi = end && *end == '-' ? static_cast<uint32_t>(std::strtoul(end + 1, nullptr, 16)) : lo;\n            }\n"
        << "            blocks = std::getenv(\"GOW_VU1C_NO_BLOCKS\") == nullptr;\n        }\n    } g_range;\n"
        << "    inline bool inRange(uint32_t pc) { return pc >= g_range.lo && pc <= g_range.hi; }\n}\n\n"
        << "namespace vu1c_gen\n{\n" << declarations.str()
        << "    void run(VU1Interpreter &vu, C &c)\n    {\n        for (;;)\n        {\n"
        << "            uint32_t pc = VU1CompiledAccess::pc(vu);\n"
        << "            if (pc + 8u <= c.codeSize)\n            {\n"
        << "                uint32_t lower, upper;\n"
        << "                std::memcpy(&lower, c.vuCode + pc, 4);\n                std::memcpy(&upper, c.vuCode + pc + 4, 4);\n"
        << "                switch (pc)\n                {\n";
    std::set<uint32_t> allPcs;
    for (const auto &[pc, v] : pairCases)
        allPcs.insert(pc);
    for (const auto &[pc, v] : blockCases)
        allPcs.insert(pc);
    for (const uint32_t pc : allPcs)
    {
        out << "                case 0x" << hex4(pc) << ":\n                L" << hex4(pc) << ":\n";
        if (const auto it = blockCases.find(pc); it != blockCases.end())
        {
            out << "                if (g_range.blocks)\n                {\n";
            for (const BlockCase &t : it->second)
            {
                // GOW-Port: if the block ends at a known successor with its own case, it jumps to it
                // directly (same work as returning to the top of the loop, without the switch).
                out << t.test << "                    {\n                        pc = VU1CompiledAccess::pc(vu);\n";
                for (const uint32_t next : t.successors)
                    if (allPcs.count(next) != 0u && next + 8u <= PS2_VU1_CODE_SIZE)
                        out << "                        if (pc == 0x" << hex4(next) << "u && pc + 8u <= c.codeSize)\n"
                            << "                        {\n"
                            << "                            std::memcpy(&lower, c.vuCode + pc, 4);\n"
                            << "                            std::memcpy(&upper, c.vuCode + pc + 4, 4);\n"
                            << "                            goto L" << hex4(next) << ";\n"
                            << "                        }\n";
                out << "                        continue;\n                    }\n                }\n";
            }
            out << "                }\n";
        }
        if (const auto it = pairCases.find(pc); it != pairCases.end())
            for (const auto &t : it->second)
                out << t;
        out << "                break;\n";
    }
    out << "                default:\n                    break;\n                }\n            }\n"
        << "            ++g_stats.interpreted;\n"
        << "            if (!VU1CompiledAccess::interpret(vu, c))\n                return;\n        }\n    }\n}\n\n"
        << "static const bool g_registered = (VU1Interpreter::registerCompiledProgram(&vu1c_gen::run), true);\n"
        << footer;
    writeIfChanged(base + "/program0.cpp", out.str());
    std::printf("[generate_vu1] %zu pair functions, %zu blocks, %zu of %zu FMACs with dead flags, %zu images\n",
                pairFns.size(), blockCount, deadCount, fmacCount, codes.size());
    return 0;
}
