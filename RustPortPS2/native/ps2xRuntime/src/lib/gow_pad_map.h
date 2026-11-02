// GOW-Port: the pure logic of the gamepad layer, with no raylib and no GLFW so it can be compiled and tested on its own
// (RustPortPS2/native/tests/gow_pad_map_test.cpp). Header only and included by ps2_pad.cpp alone: it is not a header the generated
// code includes, so changing it does not recompile the ~6,400 generated units.
//
// PC_PAD_MAP="source=target;source=target;...": which PS2 button each physical gamepad button presses.
//   sources (the raylib / Xbox names): a b x y lb rb lt rt back start ls rs up down left right
//   targets (the PS2 pad): cross circle square triangle l1 r1 l2 r2 select start l3 r3 up down left right none
// Rules: a source appears once; several sources may press the same target; a source that is not listed presses nothing;
// `start` must be the target of at least one source (without it the game cannot be started). Anything else makes the whole value
// invalid and the launcher's layouts (face map and shoulder preset) stay in force.
#pragma once

#include <array>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <string>

namespace gowpad
{
    // The PS2 pad word is active low (a pressed button clears its bit), the order of the libpad packet.
    constexpr uint16_t kLeft = 0x0080u;
    constexpr uint16_t kDown = 0x0040u;
    constexpr uint16_t kRight = 0x0020u;
    constexpr uint16_t kUp = 0x0010u;
    constexpr uint16_t kStart = 0x0008u;
    constexpr uint16_t kR3 = 0x0004u;
    constexpr uint16_t kL3 = 0x0002u;
    constexpr uint16_t kSelect = 0x0001u;
    constexpr uint16_t kSquare = 0x8000u;
    constexpr uint16_t kCross = 0x4000u;
    constexpr uint16_t kCircle = 0x2000u;
    constexpr uint16_t kTriangle = 0x1000u;
    constexpr uint16_t kR1 = 0x0800u;
    constexpr uint16_t kL1 = 0x0400u;
    constexpr uint16_t kR2 = 0x0200u;
    constexpr uint16_t kL2 = 0x0100u;

    // raylib's GamepadButton ids (raylib.h): the order the game's table is indexed by. Copied so the header needs no raylib.
    enum Button : int
    {
        BtnLeftFaceUp = 1, BtnLeftFaceRight, BtnLeftFaceDown, BtnLeftFaceLeft,
        BtnRightFaceUp, BtnRightFaceRight, BtnRightFaceDown, BtnRightFaceLeft,
        BtnLeftTrigger1, BtnLeftTrigger2, BtnRightTrigger1, BtnRightTrigger2,
        BtnMiddleLeft, BtnMiddle, BtnMiddleRight, BtnLeftThumb, BtnRightThumb,
        ButtonCount // one past the last id; index 0 is "unknown" and never used
    };

    // What each raylib button id presses: bits of the PS2 word, 0 for nothing. Built once from the environment.
    using Table = std::array<uint16_t, ButtonCount>;

    // The layouts the launcher offers without PC_PAD_MAP, exactly as ps2_pad.cpp wired them one `if` at a time before this table.
    // `faceMap` is PC_PAD_FACE_MAP ("cross,circle,square,triangle" in the order A,B,X,Y; a permutation, else it is ignored);
    // `dmc5` swaps the shoulders (LT presses R2 and RT presses L2).
    inline Table presetTable(bool dmc5, const char *faceMap)
    {
        constexpr std::array<uint16_t, 4> classic{kCross, kCircle, kSquare, kTriangle};
        constexpr std::array<uint16_t, 4> modern{kCross, kTriangle, kCircle, kSquare};
        const std::array<uint16_t, 4> fallback = dmc5 ? modern : classic;
        std::array<uint16_t, 4> faces = fallback;
        if (faceMap && *faceMap)
        {
            const std::string names(faceMap);
            std::array<uint16_t, 4> mapped{};
            std::array<bool, 4> seen{};
            size_t offset = 0;
            bool valid = true;
            for (size_t i = 0; i < mapped.size() && valid; ++i)
            {
                const size_t comma = names.find(',', offset);
                if ((i < 3 && comma == std::string::npos) || (i == 3 && comma != std::string::npos))
                {
                    valid = false;
                    break;
                }
                const std::string name = names.substr(offset, comma == std::string::npos ? comma : comma - offset);
                constexpr const char *labels[] = {"cross", "circle", "square", "triangle"};
                bool known = false;
                for (size_t j = 0; j < 4; ++j)
                    if (name == labels[j] && !seen[j])
                    {
                        mapped[i] = classic[j];
                        seen[j] = known = true;
                        break;
                    }
                if (!known)
                    valid = false;
                offset = comma == std::string::npos ? names.size() : comma + 1;
            }
            if (valid)
                faces = mapped;
        }
        Table table{};
        table[BtnLeftFaceUp] = kUp;
        table[BtnLeftFaceDown] = kDown;
        table[BtnLeftFaceLeft] = kLeft;
        table[BtnLeftFaceRight] = kRight;
        table[BtnRightFaceDown] = faces[0]; // A
        table[BtnRightFaceRight] = faces[1]; // B
        table[BtnRightFaceLeft] = faces[2]; // X
        table[BtnRightFaceUp] = faces[3]; // Y
        table[BtnLeftTrigger1] = kL1;
        table[BtnRightTrigger1] = kR1;
        table[BtnLeftTrigger2] = dmc5 ? kR2 : kL2;
        table[BtnRightTrigger2] = dmc5 ? kL2 : kR2;
        table[BtnMiddleRight] = kStart;
        table[BtnMiddleLeft] = kSelect;
        table[BtnLeftThumb] = kL3;
        table[BtnRightThumb] = kR3;
        return table;
    }

    struct SourceName
    {
        const char *name;
        int button;
    };
    constexpr SourceName kSources[] = {
        {"a", BtnRightFaceDown}, {"b", BtnRightFaceRight}, {"x", BtnRightFaceLeft}, {"y", BtnRightFaceUp},
        {"lb", BtnLeftTrigger1}, {"rb", BtnRightTrigger1}, {"lt", BtnLeftTrigger2}, {"rt", BtnRightTrigger2},
        {"back", BtnMiddleLeft}, {"start", BtnMiddleRight}, {"ls", BtnLeftThumb}, {"rs", BtnRightThumb},
        {"up", BtnLeftFaceUp}, {"down", BtnLeftFaceDown}, {"left", BtnLeftFaceLeft}, {"right", BtnLeftFaceRight}};

    struct TargetName
    {
        const char *name;
        uint16_t bits;
    };
    constexpr TargetName kTargets[] = {
        {"cross", kCross}, {"circle", kCircle}, {"square", kSquare}, {"triangle", kTriangle},
        {"l1", kL1}, {"r1", kR1}, {"l2", kL2}, {"r2", kR2}, {"select", kSelect}, {"start", kStart},
        {"l3", kL3}, {"r3", kR3}, {"up", kUp}, {"down", kDown}, {"left", kLeft}, {"right", kRight}, {"none", 0}};

    struct MapResult
    {
        bool ok = false;
        Table table{};
        std::string error; // why the value is invalid; empty when ok
    };

    inline std::string trimmed(const std::string &text)
    {
        size_t begin = 0, end = text.size();
        while (begin < end && (text[begin] == ' ' || text[begin] == '\t'))
            ++begin;
        while (end > begin && (text[end - 1] == ' ' || text[end - 1] == '\t'))
            --end;
        return text.substr(begin, end - begin);
    }

    // Parses a PC_PAD_MAP value. Not ok for anything the rules above do not allow (including an empty value: the caller decides
    // that an empty variable simply means "no custom map").
    inline MapResult parsePadMap(const char *text)
    {
        MapResult result;
        if (!text || !*text)
        {
            result.error = "empty";
            return result;
        }
        const std::string all(text);
        std::array<bool, ButtonCount> listed{};
        bool startPressable = false;
        size_t offset = 0;
        while (offset < all.size())
        {
            size_t end = all.find(';', offset);
            if (end == std::string::npos)
                end = all.size();
            const std::string entry = trimmed(all.substr(offset, end - offset));
            offset = end + 1;
            if (entry.empty())
            {
                if (end == all.size()) // one trailing ';' is fine
                    break;
                result.error = "empty entry";
                return result;
            }
            const size_t equals = entry.find('=');
            if (equals == std::string::npos)
            {
                result.error = "'" + entry + "' is not source=target";
                return result;
            }
            const std::string source = trimmed(entry.substr(0, equals));
            const std::string target = trimmed(entry.substr(equals + 1));
            int button = 0;
            for (const SourceName &candidate : kSources)
                if (source == candidate.name)
                    button = candidate.button;
            if (button == 0)
            {
                result.error = "unknown source '" + source + "'";
                return result;
            }
            if (listed[button])
            {
                result.error = "source '" + source + "' appears twice";
                return result;
            }
            const TargetName *chosen = nullptr;
            for (const TargetName &candidate : kTargets)
                if (target == candidate.name)
                    chosen = &candidate;
            if (!chosen)
            {
                result.error = "unknown target '" + target + "'";
                return result;
            }
            listed[button] = true;
            result.table[button] = chosen->bits;
            startPressable = startPressable || (chosen->bits & kStart) != 0;
        }
        if (!startPressable)
        {
            result.table = Table{};
            result.error = "no button presses start";
            return result;
        }
        result.ok = true;
        return result;
    }

    // The table the game reads through. A valid PC_PAD_MAP replaces both the face map and the shoulder preset; an invalid one is
    // ignored (`note` then says why, to be logged once) and an empty one is not mentioned.
    inline Table buildTable(const char *padMap, const char *faceMap, bool dmc5, std::string *note = nullptr)
    {
        if (padMap && *padMap)
        {
            const MapResult parsed = parsePadMap(padMap);
            if (parsed.ok)
                return parsed.table;
            if (note)
                *note = "PC_PAD_MAP ignored: " + parsed.error;
        }
        return presetTable(dmc5, faceMap);
    }

    // The PS2 bits the pressed buttons ask for; `down(id)` says whether raylib button `id` is held. The game's word is
    // active low, so the caller clears these bits.
    template <class Down>
    inline uint16_t pressedBits(const Table &table, Down &&down)
    {
        uint16_t bits = 0;
        for (int id = 1; id < ButtonCount; ++id)
            if (table[id] != 0 && down(id))
                bits |= table[id];
        return bits;
    }
}
