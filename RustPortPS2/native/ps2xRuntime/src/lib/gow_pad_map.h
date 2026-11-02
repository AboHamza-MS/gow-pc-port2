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
//
// It also holds what the launcher's controller picker needs: the list of joysticks the runtime writes for the launcher
// (GOW_PAD_LIST_FILE) and the choice of the joystick the game reads (GOW_PAD_DEVICE, a GLFW GUID).
#pragma once

#include <array>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <utility>
#include <vector>

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

    // ------------------------------------------------------------------------------------------------ controller picker

    // A joystick GLFW lists, as the launcher is told about it. `mapped`: GLFW has an SDL mapping for it (without one every
    // button reads as released). `index` is the GLFW joystick id, which is also raylib's gamepad slot while it is below 4.
    struct Device
    {
        int index = 0;
        std::string name;
        std::string guid;
        bool mapped = false;
        int buttons = 0;
        int hats = 0;
        int axes = 0;
    };

    // raylib reads the first four joysticks only.
    constexpr int kSlots = 4;

    enum class Choice
    {
        Automatic, // nothing was chosen: the game picks the pad itself
        Found,     // the chosen device is connected in a slot the game reads
        Absent,    // the chosen device is not connected: automatic choice
        OutOfRange // connected, but past the four slots the game reads: automatic choice
    };

    struct Pick
    {
        Choice choice = Choice::Automatic;
        int index = -1; // the joystick found: the one to read when Found, the one that is out of reach when OutOfRange
    };

    inline bool sameGuid(const std::string &a, const std::string &b)
    {
        if (a.size() != b.size())
            return false;
        for (size_t i = 0; i < a.size(); ++i)
        {
            const char x = a[i] >= 'A' && a[i] <= 'Z' ? static_cast<char>(a[i] + 32) : a[i];
            const char y = b[i] >= 'A' && b[i] <= 'Z' ? static_cast<char>(b[i] + 32) : b[i];
            if (x != y)
                return false;
        }
        return true;
    }

    // Which joystick the game reads. `wanted` is the chosen GUID (empty or null: automatic). Devices with the same GUID (two
    // identical pads) are told apart only by their order: the first one connected is used.
    inline Pick pickDevice(const std::vector<Device> &devices, const char *wanted)
    {
        Pick pick;
        if (!wanted || !*wanted)
            return pick;
        pick.choice = Choice::Absent;
        for (const Device &device : devices)
            if (sameGuid(device.guid, wanted))
            {
                if (device.index < kSlots)
                {
                    pick.choice = Choice::Found;
                    pick.index = device.index;
                }
                else
                {
                    pick.choice = Choice::OutOfRange;
                    pick.index = device.index;
                }
                return pick;
            }
        return pick;
    }

    inline const char *choiceName(Choice choice)
    {
        switch (choice)
        {
        case Choice::Found: return "found";
        case Choice::Absent: return "absent";
        case Choice::OutOfRange: return "outOfRange";
        default: return "automatic";
        }
    }

    inline std::string jsonString(const std::string &text)
    {
        std::string out = "\"";
        for (const char c : text)
        {
            switch (c)
            {
            case '"': out += "\\\""; break;
            case '\\': out += "\\\\"; break;
            case '\n': out += "\\n"; break;
            case '\r': out += "\\r"; break;
            case '\t': out += "\\t"; break;
            default:
                if (static_cast<unsigned char>(c) < 0x20)
                {
                    char buffer[8];
                    std::snprintf(buffer, sizeof(buffer), "\\u%04x", static_cast<unsigned>(static_cast<unsigned char>(c)));
                    out += buffer;
                }
                else
                    out += c;
            }
        }
        return out + "\"";
    }

    // The file the launcher reads (schema 1). `active` is the joystick the game reads now (-1: none). The timestamp is left to
    // the caller so the text can be compared to decide whether the file changed.
    inline std::string devicesJson(const std::vector<Device> &devices, const Pick &pick, const char *wanted, int active, const char *platform)
    {
        std::string out = "{\"schema\":1,\"platform\":" + jsonString(platform) + ",\"preferred\":" + jsonString(wanted ? wanted : "") +
                          ",\"choice\":" + jsonString(choiceName(pick.choice)) + ",\"active\":" + std::to_string(active) + ",\"devices\":[";
        for (size_t i = 0; i < devices.size(); ++i)
        {
            const Device &d = devices[i];
            out += std::string(i ? "," : "") + "{\"index\":" + std::to_string(d.index) + ",\"name\":" + jsonString(d.name) +
                   ",\"guid\":" + jsonString(d.guid) + ",\"mapped\":" + (d.mapped ? "true" : "false") +
                   ",\"buttons\":" + std::to_string(d.buttons) + ",\"hats\":" + std::to_string(d.hats) +
                   ",\"axes\":" + std::to_string(d.axes) + "}";
        }
        return out + "]}";
    }

    // ------------------------------------------------------------------------------------------------ manual mapping

    // A GLFW joystick GUID: 32 hexadecimal digits.
    inline bool isGuid(const std::string &text)
    {
        if (text.size() != 32)
            return false;
        for (const char c : text)
            if (!((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F')))
                return false;
        return true;
    }

    // GOW_PAD_SDL_MAPPING="<guid>,<name>,<fields>" is the launcher's hand-made SDL mapping for the chosen device (a pad that GLFW has
    // no mapping for, so that every button reads as released). GLFW wants a full SDL_GameControllerDB line, which also names the
    // platform; the runtime adds that itself so the launcher stays platform neutral. Returns "" for anything that is not exactly one
    // line of that shape, so a bad variable is ignored instead of being handed to GLFW.
    inline std::string glfwMappingLine(const char *text, const char *platform)
    {
        if (!text || !*text)
            return "";
        std::string line(text);
        if (line.find('\n') != std::string::npos || line.find('\r') != std::string::npos)
            return "";
        const size_t first = line.find(',');
        if (first == std::string::npos || !isGuid(line.substr(0, first)))
            return "";
        const size_t second = line.find(',', first + 1);
        if (second == std::string::npos || second + 1 >= line.size())
            return "";
        if (line.find("platform:") != std::string::npos)
            return "";
        if (line.back() != ',')
            line += ',';
        // GLFW compares GUIDs as text, in lower case (the case its own `glfwGetJoystickGUID` returns), so a line with a GUID in
        // capitals would be accepted and never attach to the joystick.
        for (size_t i = 0; i < first; ++i)
            if (line[i] >= 'A' && line[i] <= 'F')
                line[i] = static_cast<char>(line[i] - 'A' + 'a');
        line += "platform:";
        line += platform;
        line += ",\n";
        // GLFW's parser reads a line into a 1024 byte buffer and silently skips a longer one: better to say no here.
        if (line.size() >= 1024)
            return "";
        return line;
    }

    // The raw input of one joystick, as GLFW reports it: buttons (0 or 1), hat bit sets, axes (-1 to 1).
    struct RawSample
    {
        std::vector<unsigned char> buttons;
        std::vector<unsigned char> hats;
        std::vector<float> axes;
    };

    // What a joystick does right now compared with how it rests: the launcher's mapping dialog turns this into "button 3", "hat 0
    // up" or "axis 2 +". An axis counts once it is `threshold` away from where it first was (a trigger rests at -1 on many pads,
    // and sticks drift); the reported value is that distance.
    struct LiveDevice
    {
        int index = 0;
        std::string guid;
        std::vector<int> buttons;                  // raw buttons held
        std::vector<std::pair<int, int>> hats;     // raw hat index, hat bits, for the hats not at rest
        std::vector<std::pair<int, int>> axes;     // raw axis index, distance from rest in hundredths
    };

    inline LiveDevice liveDevice(int index, const std::string &guid, const RawSample &now, const RawSample &rest, float threshold = 0.5f)
    {
        LiveDevice device;
        device.index = index;
        device.guid = guid;
        for (size_t i = 0; i < now.buttons.size(); ++i)
            if (now.buttons[i])
                device.buttons.push_back(static_cast<int>(i));
        for (size_t i = 0; i < now.hats.size(); ++i)
        {
            const unsigned char resting = i < rest.hats.size() ? rest.hats[i] : 0;
            if (now.hats[i] != 0 && now.hats[i] != resting)
                device.hats.emplace_back(static_cast<int>(i), static_cast<int>(now.hats[i]));
        }
        for (size_t i = 0; i < now.axes.size(); ++i)
        {
            const float resting = i < rest.axes.size() ? rest.axes[i] : 0.0f;
            const float distance = now.axes[i] - resting;
            if (distance >= threshold || distance <= -threshold)
                device.axes.emplace_back(static_cast<int>(i), static_cast<int>(distance * 100.0f + (distance < 0 ? -0.5f : 0.5f)));
        }
        return device;
    }

    // The live file (schema 1). `beat` grows with every write, so the launcher can tell a quiet pad from a game that stopped.
    inline std::string liveJson(const std::vector<LiveDevice> &devices, unsigned beat)
    {
        std::string out = "{\"schema\":1,\"beat\":" + std::to_string(beat) + ",\"devices\":[";
        for (size_t d = 0; d < devices.size(); ++d)
        {
            const LiveDevice &device = devices[d];
            out += std::string(d ? "," : "") + "{\"index\":" + std::to_string(device.index) + ",\"guid\":" + jsonString(device.guid) + ",\"buttons\":[";
            for (size_t i = 0; i < device.buttons.size(); ++i)
                out += (i ? "," : "") + std::to_string(device.buttons[i]);
            out += "],\"hats\":[";
            for (size_t i = 0; i < device.hats.size(); ++i)
                out += std::string(i ? "," : "") + "[" + std::to_string(device.hats[i].first) + "," + std::to_string(device.hats[i].second) + "]";
            out += "],\"axes\":[";
            for (size_t i = 0; i < device.axes.size(); ++i)
                out += std::string(i ? "," : "") + "[" + std::to_string(device.axes[i].first) + "," + std::to_string(device.axes[i].second) + "]";
            out += "]}";
        }
        return out + "]}";
    }
}
