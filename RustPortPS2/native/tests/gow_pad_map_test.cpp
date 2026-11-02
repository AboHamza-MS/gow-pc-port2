// Standalone test of the pure gamepad logic (ps2xRuntime/src/lib/gow_pad_map.h). It needs no raylib, GLFW or game files:
//
//   MSVC (from a "x64 Native Tools" prompt):  cl /nologo /std:c++17 /EHsc /W4 /I ..\ps2xRuntime\src\lib gow_pad_map_test.cpp
//   GCC / Clang:                              c++ -std=c++17 -Wall -Wextra -I ../ps2xRuntime/src/lib gow_pad_map_test.cpp -o gow_pad_map_test
//
// Run it with the path of gow-launcher/src/lib/padmap-cases.json as the argument to also check that the runtime and the launcher
// (Rust and TypeScript) agree on which PC_PAD_MAP values are valid.
//
// It is not registered in CMake on purpose (a CMake edit recompiles the whole runtime); run it by hand after touching the header.
//
// PC_PAD_MAP: the table must reproduce, for all 2^17 states of the raylib buttons, what the chain of `if (IsGamepadButtonDown...)`
// in ps2_pad.cpp produced before the table existed. `oracleWord` below is that chain, kept as it was.
#include "gow_pad_map.h"

#include <algorithm>
#include <array>
#include <cstdio>
#include <fstream>
#include <sstream>
#include <string>
#include <vector>

static int g_failures = 0;
#define CHECK(condition)                                                                                  \
    do                                                                                                    \
    {                                                                                                     \
        if (!(condition))                                                                                 \
        {                                                                                                 \
            ++g_failures;                                                                                 \
            std::printf("FAIL line %d: %s\n", __LINE__, #condition);                                      \
        }                                                                                                 \
    } while (0)

using namespace gowpad;

// The old chain, as it was in PSPadBackend::readState, with `down` standing for IsGamepadButtonDown(gamepad, id).
static uint16_t oracleWord(uint32_t downMask, bool dmc5Controls, const char *faceMapEnv)
{
    auto down = [downMask](int id) { return (downMask >> id & 1u) != 0; };
    uint16_t btns = 0xFFFFu;
    auto clearBit = [&btns](uint16_t mask) { btns &= ~mask; };
    const std::array<uint16_t, 4> faceButtons = [&]
    {
        constexpr std::array<uint16_t, 4> classic{kCross, kCircle, kSquare, kTriangle};
        constexpr std::array<uint16_t, 4> modern{kCross, kTriangle, kCircle, kSquare};
        const auto fallback = dmc5Controls ? modern : classic;
        const char *value = faceMapEnv;
        if (!value || !*value)
            return fallback;
        const std::string names(value);
        std::array<uint16_t, 4> mapped{};
        std::array<bool, 4> seen{};
        size_t offset = 0;
        for (size_t i = 0; i < mapped.size(); ++i)
        {
            const size_t comma = names.find(',', offset);
            if ((i < 3 && comma == std::string::npos) || (i == 3 && comma != std::string::npos))
                return fallback;
            const std::string name = names.substr(offset, comma == std::string::npos ? comma : comma - offset);
            constexpr const char *labels[] = {"cross", "circle", "square", "triangle"};
            bool valid = false;
            for (size_t j = 0; j < 4; ++j)
                if (name == labels[j] && !seen[j])
                {
                    mapped[i] = classic[j];
                    seen[j] = valid = true;
                    break;
                }
            if (!valid)
                return fallback;
            offset = comma == std::string::npos ? names.size() : comma + 1;
        }
        return mapped;
    }();
    if (down(BtnLeftFaceUp)) clearBit(kUp);
    if (down(BtnLeftFaceDown)) clearBit(kDown);
    if (down(BtnLeftFaceLeft)) clearBit(kLeft);
    if (down(BtnLeftFaceRight)) clearBit(kRight);
    if (down(BtnRightFaceDown)) clearBit(faceButtons[0]);
    if (down(BtnRightFaceRight)) clearBit(faceButtons[1]);
    if (down(BtnRightFaceLeft)) clearBit(faceButtons[2]);
    if (down(BtnRightFaceUp)) clearBit(faceButtons[3]);
    if (dmc5Controls)
    {
        if (down(BtnLeftTrigger1)) clearBit(kL1);
        if (down(BtnRightTrigger1)) clearBit(kR1);
        if (down(BtnLeftTrigger2)) clearBit(kR2);
        if (down(BtnRightTrigger2)) clearBit(kL2);
    }
    else
    {
        if (down(BtnLeftTrigger1)) clearBit(kL1);
        if (down(BtnRightTrigger1)) clearBit(kR1);
        if (down(BtnLeftTrigger2)) clearBit(kL2);
        if (down(BtnRightTrigger2)) clearBit(kR2);
    }
    if (down(BtnMiddleRight)) clearBit(kStart);
    if (down(BtnMiddleLeft)) clearBit(kSelect);
    if (down(BtnLeftThumb)) clearBit(kL3);
    if (down(BtnRightThumb)) clearBit(kR3);
    return btns;
}

static uint16_t tableWord(const Table &table, uint32_t downMask)
{
    const uint16_t pressed = pressedBits(table, [downMask](int id) { return (downMask >> id & 1u) != 0; });
    return static_cast<uint16_t>(0xFFFFu & ~pressed);
}

// Every one of the 2^17 raylib button states (bits 1..17; bit 0 and the unused guide button 14 included, the guide is never read).
static bool sameForAllStates(bool dmc5, const char *faceMap)
{
    const Table table = presetTable(dmc5, faceMap);
    for (uint32_t state = 0; state < (1u << 18); state += 2) // ids 1..17 live in bits 1..17
        if (oracleWord(state, dmc5, faceMap) != tableWord(table, state))
            return false;
    return true;
}

static void oracleTest()
{
    CHECK(sameForAllStates(false, nullptr));
    CHECK(sameForAllStates(true, nullptr));
    CHECK(sameForAllStates(false, ""));
    // Every permutation of the four face buttons, with either shoulder layout.
    std::array<const char *, 4> names{"cross", "circle", "square", "triangle"};
    std::array<int, 4> order{0, 1, 2, 3};
    int permutations = 0;
    do
    {
        std::string text;
        for (size_t i = 0; i < 4; ++i)
            text += std::string(i ? "," : "") + names[order[i]];
        CHECK(sameForAllStates(false, text.c_str()));
        CHECK(sameForAllStates(true, text.c_str()));
        ++permutations;
    } while (std::next_permutation(order.begin(), order.end()));
    CHECK(permutations == 24);
    // A face map that is not a permutation is ignored by the old chain and by the table alike.
    for (const char *bad : {"cross,circle", "cross,cross,square,triangle", "cross,circle,square,triangle,cross", "a,b,c,d", "cross;circle;square;triangle",
                            "cross,circle,square,", ",cross,circle,square", "Cross,circle,square,triangle"})
    {
        CHECK(sameForAllStates(false, bad));
        CHECK(sameForAllStates(true, bad));
    }
    // An empty or missing PC_PAD_MAP gives exactly the presets.
    for (const bool dmc5 : {false, true})
    {
        std::string note;
        CHECK(buildTable(nullptr, nullptr, dmc5, &note) == presetTable(dmc5, nullptr));
        CHECK(buildTable("", "cross,triangle,circle,square", dmc5, &note) == presetTable(dmc5, "cross,triangle,circle,square"));
        CHECK(note.empty());
    }
}

static MapResult parse(const char *text) { return parsePadMap(text); }

static void customMapTest()
{
    // The classic layout written out in full is the classic preset.
    const char *classic = "a=cross;b=circle;x=square;y=triangle;lb=l1;rb=r1;lt=l2;rt=r2;back=select;start=start;ls=l3;rs=r3;up=up;down=down;left=left;right=right";
    CHECK(parse(classic).ok);
    CHECK(parse(classic).table == presetTable(false, nullptr));
    CHECK(parse((std::string(classic) + ";").c_str()).ok); // one trailing ';'
    // The modern layout: face buttons by the DMC5 table and swapped triggers.
    const char *modern = "a=cross;b=triangle;x=circle;y=square;lb=l1;rb=r1;lt=r2;rt=l2;back=select;start=start;ls=l3;rs=r3;up=up;down=down;left=left;right=right";
    CHECK(parse(modern).ok && parse(modern).table == presetTable(true, nullptr));

    // Shared targets add up: two buttons on cross press it together, and either one alone presses it.
    MapResult shared = parse("a=cross;b=cross;start=start");
    CHECK(shared.ok);
    CHECK(tableWord(shared.table, 1u << BtnRightFaceDown) == static_cast<uint16_t>(~kCross));
    CHECK(tableWord(shared.table, 1u << BtnRightFaceRight) == static_cast<uint16_t>(~kCross));
    CHECK(tableWord(shared.table, 1u << BtnRightFaceRight | 1u << BtnRightFaceDown) == static_cast<uint16_t>(~kCross));
    // Different sources on different targets press both.
    MapResult both = parse("a=cross;b=circle;start=start");
    CHECK(tableWord(both.table, 1u << BtnRightFaceDown | 1u << BtnRightFaceRight) == static_cast<uint16_t>(~(kCross | kCircle)));
    // `none` drops a button, and a source that is not listed presses nothing.
    MapResult dropped = parse("a=none;b=circle;start=start");
    CHECK(dropped.ok);
    CHECK(tableWord(dropped.table, 1u << BtnRightFaceDown) == 0xFFFF);
    CHECK(tableWord(dropped.table, 1u << BtnLeftTrigger1) == 0xFFFF);
    CHECK(tableWord(dropped.table, 1u << BtnRightFaceRight) == static_cast<uint16_t>(~kCircle));
    // The start source can also be a different button.
    MapResult moved = parse("a=start");
    CHECK(moved.ok && tableWord(moved.table, 1u << BtnRightFaceDown) == static_cast<uint16_t>(~kStart));
    // The guide button (14) and unknown ids never press anything.
    CHECK(tableWord(presetTable(false, nullptr), 1u << BtnMiddle) == 0xFFFF);

    // Invalid values.
    CHECK(!parse("a=cross;b=circle").ok);                    // no start
    CHECK(!parse("a=cross;start=none").ok);                  // start only as none
    CHECK(!parse("a=cross;a=circle;start=start").ok);        // duplicate source
    CHECK(!parse("q=cross;start=start").ok);                 // unknown source
    CHECK(!parse("a=jump;start=start").ok);                  // unknown target
    CHECK(!parse("a;start=start").ok);                       // no '='
    CHECK(!parse("a=cross;;start=start").ok);                // empty entry
    CHECK(!parse("A=cross;start=start").ok);                 // names are lower case
    CHECK(!parse("").ok && !parse(nullptr).ok);
    CHECK(parse("a=cross;start=start").error.empty());
    CHECK(parse("a=cross;b=circle").error == "no button presses start");
    CHECK(parse("a=cross;a=circle;start=start").error == "source 'a' appears twice");

    // An invalid map is ignored with one note and the presets (including the face map and dmc5) stay.
    std::string note;
    const Table fallback = buildTable("a=cross;b=circle", "cross,triangle,circle,square", true, &note);
    CHECK(fallback == presetTable(true, "cross,triangle,circle,square"));
    CHECK(note == "PC_PAD_MAP ignored: no button presses start");
    // A valid map wins over both the face map and the shoulder preset.
    note.clear();
    const Table custom = buildTable("a=triangle;start=start", "cross,triangle,circle,square", true, &note);
    CHECK(note.empty());
    CHECK(tableWord(custom, 1u << BtnRightFaceDown) == static_cast<uint16_t>(~kTriangle));
    CHECK(tableWord(custom, 1u << BtnLeftTrigger1) == 0xFFFF);
}

// The table of cases the launcher's tests share (padmap-cases.json): one case per line, `"value": "...", "valid": true|false`.
static void sharedCasesTest(const char *path)
{
    std::ifstream file(path);
    CHECK(file.good());
    std::string line;
    int cases = 0;
    while (std::getline(file, line))
    {
        const size_t key = line.find("\"value\": \"");
        if (key == std::string::npos)
            continue;
        const size_t begin = key + std::strlen("\"value\": \"");
        const size_t end = line.find("\", \"valid\":", begin);
        CHECK(end != std::string::npos);
        const std::string value = line.substr(begin, end - begin);
        const bool valid = line.find("\"valid\": true") != std::string::npos;
        if (parsePadMap(value.c_str()).ok != valid)
            std::printf("FAIL shared case: [%s] should be %s\n", value.c_str(), valid ? "valid" : "invalid");
        CHECK(parsePadMap(value.c_str()).ok == valid);
        ++cases;
    }
    CHECK(cases > 15);
    std::printf("%d shared cases checked\n", cases);
}

int main(int argc, char **argv)
{
    oracleTest();
    customMapTest();
    if (argc > 1)
        sharedCasesTest(argv[1]);
    if (g_failures == 0)
        std::printf("PASS: gow_pad_map\n");
    return g_failures == 0 ? 0 : 1;
}
