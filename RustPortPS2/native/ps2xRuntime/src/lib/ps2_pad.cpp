#include "runtime/ps2_pad.h"
#include "ps2_host_backend.h"
#include <array>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>

#ifdef _WIN32
bool ps2PadWindowsRumble(uint8_t large, bool small); // ps2_pad_rumble_win.cpp
#endif

namespace
{
    constexpr uint8_t kPadAnalogMarker = 0x73;
    constexpr uint8_t kPadStickCenter = 0x80;

    constexpr uint16_t PAD_LEFT = 0x0080u;
    constexpr uint16_t PAD_DOWN = 0x0040u;
    constexpr uint16_t PAD_RIGHT = 0x0020u;
    constexpr uint16_t PAD_UP = 0x0010u;
    constexpr uint16_t PAD_START = 0x0008u;
    constexpr uint16_t PAD_R3 = 0x0004u;
    constexpr uint16_t PAD_L3 = 0x0002u;
    constexpr uint16_t PAD_SELECT = 0x0001u;
    constexpr uint16_t PAD_SQUARE = 0x8000u;
    constexpr uint16_t PAD_CROSS = 0x4000u;
    constexpr uint16_t PAD_CIRCLE = 0x2000u;
    constexpr uint16_t PAD_TRIANGLE = 0x1000u;
    constexpr uint16_t PAD_R1 = 0x0800u;
    constexpr uint16_t PAD_L1 = 0x0400u;
    constexpr uint16_t PAD_R2 = 0x0200u;
    constexpr uint16_t PAD_L2 = 0x0100u;

    // GOW-Port: keyboard and mouse configurable from the launcher. PC_KEYS="action=code,code;..." with
    // raylib key codes (GLFW's, by physical position) and 1000 + button for the mouse (1000 left, 1001
    // right, 1002 middle, 1003/1004 side). Left Shift, Ctrl and Alt also count for the right ones.
    // Without the variable, the keyboard stays as before (fixed scheme). The pad does not change.
    //
    // God of War's own actions: each magic presses its d-pad direction, according to the game's controls table
    // (0x29D058: right, left, up and down give bits 26-29, which 0x1D7250 converts into SelectMagic
    // 3-6): right = Zeus' Rage, left = Medusa's Gaze, up = Army of Hades, down = Poseidon's
    // Rage. "artemis" presses L1 + R1 at once (draw and sheathe the Blade of Artemis); "rage", L3 + R3.
    enum Action : size_t
    {
        ActionUp, ActionDown, ActionLeft, ActionRight, ActionCross, ActionCircle, ActionSquare, ActionTriangle,
        ActionL1, ActionR1, ActionL2, ActionR2, ActionL3, ActionR3, ActionStart, ActionSelect,
        ActionPoseidon, ActionMedusa, ActionZeus, ActionHades, ActionArtemis, ActionFury,
        ActionLeftStickUp, ActionLeftStickDown, ActionLeftStickLeft, ActionLeftStickRight, ActionRightStickUp, ActionRightStickDown, ActionRightStickLeft, ActionRightStickRight, ActionRightStickModifier,
        ActionCount
    };
    constexpr const char *kActionNames[ActionCount] = {
        "up", "down", "left", "right", "cross", "circle", "square", "triangle",
        "l1", "r1", "l2", "r2", "l3", "r3", "start", "select",
        "poseidon", "medusa", "zeus", "hades", "artemis", "rage",
        "ls_up", "ls_down", "ls_left", "ls_right", "rs_up", "rs_down", "rs_left", "rs_right", "rs_mod"};
    // Buttons each action presses (up to ActionFury; the others move the sticks).
    constexpr uint16_t kActionBits[ActionFury + 1] = {
        PAD_UP, PAD_DOWN, PAD_LEFT, PAD_RIGHT, PAD_CROSS, PAD_CIRCLE, PAD_SQUARE, PAD_TRIANGLE,
        PAD_L1, PAD_R1, PAD_L2, PAD_R2, PAD_L3, PAD_R3, PAD_START, PAD_SELECT,
        PAD_DOWN, PAD_LEFT, PAD_RIGHT, PAD_UP, PAD_L1 | PAD_R1, PAD_L3 | PAD_R3};

    struct KeyMap
    {
        bool enabled = false;
        std::array<std::vector<int>, ActionCount> codes;
    };

    const KeyMap &keyMap()
    {
        static const KeyMap map = []
        {
            KeyMap map;
            const char *value = std::getenv("PC_KEYS");
            if (!value || !*value)
                return map;
            map.enabled = true;
            const std::string text(value);
            size_t offset = 0;
            while (offset < text.size())
            {
                size_t end = text.find(';', offset);
                if (end == std::string::npos)
                    end = text.size();
                const std::string pair = text.substr(offset, end - offset);
                const size_t equals = pair.find('=');
                if (equals != std::string::npos)
                {
                    const std::string name = pair.substr(0, equals);
                    for (size_t a = 0; a < ActionCount; ++a)
                    {
                        if (name != kActionNames[a])
                            continue;
                        size_t cursor = equals + 1;
                        while (cursor < pair.size())
                        {
                            size_t comma = pair.find(',', cursor);
                            if (comma == std::string::npos)
                                comma = pair.size();
                            const int code = std::atoi(pair.substr(cursor, comma - cursor).c_str());
                            if (code > 0)
                                map.codes[a].push_back(code);
                            cursor = comma + 1;
                        }
                    }
                }
                offset = end + 1;
            }
            return map;
        }();
        return map;
    }

    bool isCodePressed(int code)
    {
        if (code >= 1000)
            return IsMouseButtonDown(code - 1000);
        switch (code)
        {
        case KEY_LEFT_SHIFT:
        case KEY_RIGHT_SHIFT:
            return IsKeyDown(KEY_LEFT_SHIFT) || IsKeyDown(KEY_RIGHT_SHIFT);
        case KEY_LEFT_CONTROL:
        case KEY_RIGHT_CONTROL:
            return IsKeyDown(KEY_LEFT_CONTROL) || IsKeyDown(KEY_RIGHT_CONTROL);
        case KEY_LEFT_ALT:
        case KEY_RIGHT_ALT:
            return IsKeyDown(KEY_LEFT_ALT) || IsKeyDown(KEY_RIGHT_ALT);
        default:
            return IsKeyDown(code);
        }
    }

    bool isActionPressed(const KeyMap &map, size_t action)
    {
        for (const int code : map.codes[action])
            if (isCodePressed(code))
                return true;
        return false;
    }

    bool isModifier(const KeyMap &map, int code)
    {
        for (const int modifier : map.codes[ActionRightStickModifier])
            if (modifier == code)
                return true;
        return false;
    }

    // Button: a key that is also the right-stick modifier (Ctrl = R2 and Ctrl + WASD = dodge) only counts
    // if you are not moving with the left stick; that way dodging does not also press the button.
    bool isButtonPressed(const KeyMap &map, size_t action, bool moving)
    {
        for (const int code : map.codes[action])
            if (isCodePressed(code) && !(moving && isModifier(map, code)))
                return true;
        return false;
    }

    bool isLeftStickMoving(const KeyMap &map)
    {
        return isActionPressed(map, ActionLeftStickUp) || isActionPressed(map, ActionLeftStickDown) || isActionPressed(map, ActionLeftStickLeft) ||
               isActionPressed(map, ActionLeftStickRight);
    }

    uint8_t axisValue(bool negative, bool positive)
    {
        return negative == positive ? kPadStickCenter : (negative ? 0u : 255u);
    }
}

// GOW-Port: sticks from the configurable keyboard, in libpad2 packet order (RX, RY, LX, LY). It is used by the
// wrapper of God of War's pad read (ps2_runtime.cpp), because gowPad2Read sets its own fixed sticks.
bool PCKeyboardSticks(uint8_t (&sticks)[4])
{
    const KeyMap &map = keyMap();
    if (!map.enabled || IsGamepadAvailable(0))
        return false;
    const uint8_t x = axisValue(isActionPressed(map, ActionLeftStickLeft), isActionPressed(map, ActionLeftStickRight));
    const uint8_t y = axisValue(isActionPressed(map, ActionLeftStickUp), isActionPressed(map, ActionLeftStickDown));
    if (isActionPressed(map, ActionRightStickModifier))
    {
        sticks[0] = x;
        sticks[1] = y;
        sticks[2] = sticks[3] = kPadStickCenter;
        return true;
    }
    sticks[0] = axisValue(isActionPressed(map, ActionRightStickLeft), isActionPressed(map, ActionRightStickRight));
    sticks[1] = axisValue(isActionPressed(map, ActionRightStickUp), isActionPressed(map, ActionRightStickDown));
    sticks[2] = x;
    sticks[3] = y;
    return true;
}

bool PSPadBackend::readState(int /*port*/, int /*slot*/, uint8_t *data, size_t size)
{
    if (!data || size < 32)
        return false;

    std::memset(data, 0, 32);
    data[0] = 0x01;
    data[1] = kPadAnalogMarker;
    data[2] = 0xFF;
    data[3] = 0xFF;
    data[4] = data[5] = data[6] = data[7] = kPadStickCenter;

    uint16_t btns = 0xFFFFu;
    // GOW-Port: GLFW can list racing gear (handbrake, pedals) as gamepads before the real pad. The pad whose
    // button was pressed last becomes the active one (press edges, so idle devices never take over);
    // until then, the first available slot.
    static int s_activeGamepad = -1;
    for (int i = 0; i < 4; ++i)
    {
        if (i == s_activeGamepad || !IsGamepadAvailable(i))
            continue;
        for (int button = GAMEPAD_BUTTON_LEFT_FACE_UP; button <= GAMEPAD_BUTTON_RIGHT_THUMB; ++button)
            if (IsGamepadButtonPressed(i, button))
            {
                s_activeGamepad = i;
                break;
            }
    }
    if (s_activeGamepad >= 0 && !IsGamepadAvailable(s_activeGamepad))
        s_activeGamepad = -1;
    int kGamepad = s_activeGamepad;
    for (int i = 0; i < 4 && kGamepad < 0; ++i)
        if (IsGamepadAvailable(i))
            kGamepad = i;
    const bool useGamepad = kGamepad >= 0;
    static int s_loggedGamepad = -2;
    if (kGamepad != s_loggedGamepad)
    {
        s_loggedGamepad = kGamepad;
        std::fprintf(stderr, "[pad] gamepad slot %d: %s\n", kGamepad, useGamepad ? GetGamepadName(kGamepad) : "none (keyboard)");
    }
    auto clearBit = [&btns](uint16_t mask)
    { btns &= ~mask; };

    // PC DMC5 Control Layout:
    // When enabled (via PC_DMC5_CONTROLS=1, default 0):
    // Gamepad:
    //   Y (Triangle) -> Light/Normal Melee Attack (Square in GoW)
    //   B (Circle)   -> Heavy Melee Attack (Triangle in GoW)
    //   X (Square)   -> Grab / Special Interact (Circle in GoW)
    //   A (Cross)    -> Jump (Cross in GoW)
    //   LB (L1)      -> Parry / Block (L1 in GoW)
    //   RB (R1)      -> Lock-on / Combat Stance (R1 in GoW)
    //   LT (L2)      -> Magic / Devil Trigger (R2 in GoW)
    //   RT (R2)      -> Sub-weapon switch / Magic toggle (L2 in GoW)
    static const bool dmc5Controls = [] {
        const char *val = std::getenv("PC_DMC5_CONTROLS");
        return val != nullptr && std::strcmp(val, "1") == 0;
    }();
    // A, B, X, Y by physical position. An explicit launcher map takes priority over the preset.
    // It is validated as a permutation so a corrupt preference does not accidentally cancel a button.
    static const std::array<uint16_t, 4> faceButtons = [] {
        constexpr std::array<uint16_t, 4> classic{PAD_CROSS, PAD_CIRCLE, PAD_SQUARE, PAD_TRIANGLE};
        constexpr std::array<uint16_t, 4> modern{PAD_CROSS, PAD_TRIANGLE, PAD_CIRCLE, PAD_SQUARE};
        const auto fallback = dmc5Controls ? modern : classic;
        const char *value = std::getenv("PC_PAD_FACE_MAP");
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

    if (useGamepad)
    {
        if (IsGamepadButtonDown(kGamepad, GAMEPAD_BUTTON_LEFT_FACE_UP))
            clearBit(PAD_UP);
        if (IsGamepadButtonDown(kGamepad, GAMEPAD_BUTTON_LEFT_FACE_DOWN))
            clearBit(PAD_DOWN);
        if (IsGamepadButtonDown(kGamepad, GAMEPAD_BUTTON_LEFT_FACE_LEFT))
            clearBit(PAD_LEFT);
        if (IsGamepadButtonDown(kGamepad, GAMEPAD_BUTTON_LEFT_FACE_RIGHT))
            clearBit(PAD_RIGHT);

        if (IsGamepadButtonDown(kGamepad, GAMEPAD_BUTTON_RIGHT_FACE_DOWN))
            clearBit(faceButtons[0]);
        if (IsGamepadButtonDown(kGamepad, GAMEPAD_BUTTON_RIGHT_FACE_RIGHT))
            clearBit(faceButtons[1]);
        if (IsGamepadButtonDown(kGamepad, GAMEPAD_BUTTON_RIGHT_FACE_LEFT))
            clearBit(faceButtons[2]);
        if (IsGamepadButtonDown(kGamepad, GAMEPAD_BUTTON_RIGHT_FACE_UP))
            clearBit(faceButtons[3]);

        if (dmc5Controls)
        {
            if (IsGamepadButtonDown(kGamepad, GAMEPAD_BUTTON_LEFT_TRIGGER_1)) // LB -> Block / Parry
                clearBit(PAD_L1);
            if (IsGamepadButtonDown(kGamepad, GAMEPAD_BUTTON_RIGHT_TRIGGER_1)) // RB -> Stance / Charge
                clearBit(PAD_R1);
            if (IsGamepadButtonDown(kGamepad, GAMEPAD_BUTTON_LEFT_TRIGGER_2)) // LT -> Magic
                clearBit(PAD_R2);
            if (IsGamepadButtonDown(kGamepad, GAMEPAD_BUTTON_RIGHT_TRIGGER_2)) // RT -> Magic Toggle / Secondary
                clearBit(PAD_L2);
        }
        else
        {
            // Classic PS2 GoW shoulder layout.
            if (IsGamepadButtonDown(kGamepad, GAMEPAD_BUTTON_LEFT_TRIGGER_1))
                clearBit(PAD_L1);
            if (IsGamepadButtonDown(kGamepad, GAMEPAD_BUTTON_RIGHT_TRIGGER_1))
                clearBit(PAD_R1);
            if (IsGamepadButtonDown(kGamepad, GAMEPAD_BUTTON_LEFT_TRIGGER_2))
                clearBit(PAD_L2);
            if (IsGamepadButtonDown(kGamepad, GAMEPAD_BUTTON_RIGHT_TRIGGER_2))
                clearBit(PAD_R2);
        }

        if (IsGamepadButtonDown(kGamepad, GAMEPAD_BUTTON_MIDDLE_RIGHT))
            clearBit(PAD_START);
        if (IsGamepadButtonDown(kGamepad, GAMEPAD_BUTTON_MIDDLE_LEFT))
            clearBit(PAD_SELECT);
        if (IsGamepadButtonDown(kGamepad, GAMEPAD_BUTTON_LEFT_THUMB))
            clearBit(PAD_L3);
        if (IsGamepadButtonDown(kGamepad, GAMEPAD_BUTTON_RIGHT_THUMB))
            clearBit(PAD_R3);

        float lx = GetGamepadAxisMovement(kGamepad, GAMEPAD_AXIS_LEFT_X);
        float ly = GetGamepadAxisMovement(kGamepad, GAMEPAD_AXIS_LEFT_Y);
        float rx = GetGamepadAxisMovement(kGamepad, GAMEPAD_AXIS_RIGHT_X);
        float ry = GetGamepadAxisMovement(kGamepad, GAMEPAD_AXIS_RIGHT_Y);
        data[6] = static_cast<uint8_t>(128 + lx * 127);
        data[7] = static_cast<uint8_t>(128 + ly * 127);
        data[4] = static_cast<uint8_t>(128 + rx * 127);
        data[5] = static_cast<uint8_t>(128 + ry * 127);
    }
    else if (keyMap().enabled)
    {
        // GOW-Port: configurable keyboard (launcher).
        const KeyMap &map = keyMap();
        const bool moving = isLeftStickMoving(map);
        for (size_t action = 0; action <= ActionFury; ++action)
            if (isButtonPressed(map, action, moving))
                clearBit(kActionBits[action]);
        uint8_t sticks[4];
        if (PCKeyboardSticks(sticks))
        {
            data[4] = sticks[0];
            data[5] = sticks[1];
            data[6] = sticks[2];
            data[7] = sticks[3];
        }
    }
    else
    {
        if (dmc5Controls)
        {
            // GOW-Port: keyboard and mouse (PC). The sticks are set by gowPad2Read (src/game/gow_pad2.cpp):
            // WASD = left stick, Ctrl + WASD = right stick (dodge).
            //   Arrows -> d-pad (selection)        Space -> X (jump)
            //   Left click -> Square (light)       Right click -> Triangle (heavy)
            //   F -> Circle (grab)                 Shift -> L1 (block / parry)
            //   Q -> R2 (magic)                    E -> R1 (ability / action)
            //   V -> L3 + R3 (Rage of the Gods)
            if (IsKeyDown(KEY_UP))
                clearBit(PAD_UP);
            if (IsKeyDown(KEY_DOWN))
                clearBit(PAD_DOWN);
            if (IsKeyDown(KEY_LEFT))
                clearBit(PAD_LEFT);
            if (IsKeyDown(KEY_RIGHT))
                clearBit(PAD_RIGHT);

            if (IsKeyDown(KEY_SPACE))
                clearBit(PAD_CROSS);
            if (IsMouseButtonDown(MOUSE_BUTTON_LEFT))
                clearBit(PAD_SQUARE);
            if (IsMouseButtonDown(MOUSE_BUTTON_RIGHT))
                clearBit(PAD_TRIANGLE);
            if (IsKeyDown(KEY_F))
                clearBit(PAD_CIRCLE);

            if (IsKeyDown(KEY_LEFT_SHIFT) || IsKeyDown(KEY_RIGHT_SHIFT))
                clearBit(PAD_L1);
            if (IsKeyDown(KEY_Q))
                clearBit(PAD_R2);
            if (IsKeyDown(KEY_E))
                clearBit(PAD_R1);
            if (IsKeyDown(KEY_V))
            {
                clearBit(PAD_L3);
                clearBit(PAD_R3);
            }
        }
        else
        {
            if (IsKeyDown(KEY_UP) || IsKeyDown(KEY_W))
                clearBit(PAD_UP);
            if (IsKeyDown(KEY_DOWN) || IsKeyDown(KEY_S))
                clearBit(PAD_DOWN);
            if (IsKeyDown(KEY_LEFT) || IsKeyDown(KEY_A))
                clearBit(PAD_LEFT);
            if (IsKeyDown(KEY_RIGHT) || IsKeyDown(KEY_D))
                clearBit(PAD_RIGHT);
            if (IsKeyDown(KEY_X) || IsKeyDown(KEY_SPACE))
                clearBit(PAD_CROSS);
            if (IsKeyDown(KEY_C) || IsKeyDown(KEY_ESCAPE))
                clearBit(PAD_CIRCLE);
            if (IsKeyDown(KEY_Z) || IsKeyDown(KEY_KP_0))
                clearBit(PAD_SQUARE);
            if (IsKeyDown(KEY_V) || IsKeyDown(KEY_KP_1))
                clearBit(PAD_TRIANGLE);
            if (IsKeyDown(KEY_Q))
                clearBit(PAD_L1);
            if (IsKeyDown(KEY_E))
                clearBit(PAD_R1);
            if (IsKeyDown(KEY_LEFT_SHIFT))
                clearBit(PAD_L2);
            if (IsKeyDown(KEY_RIGHT_SHIFT))
                clearBit(PAD_R2);
        }

        if (IsKeyDown(KEY_ENTER))
            clearBit(PAD_START);
        if (IsKeyDown(KEY_TAB))
            clearBit(PAD_SELECT);
    }

    data[2] = static_cast<uint8_t>(btns & 0xFF);
    data[3] = static_cast<uint8_t>(btns >> 8);

    if (kGamepad != m_activeGamepad)
    {
        m_activeGamepad = kGamepad;
        applyRumble(false); // the new gamepad takes over the current actuator state
    }
    else
        applyRumble(true);
    return true;
}

void PSPadBackend::setActuators(int port, bool small, uint8_t large)
{
    if (port != 0 || (small == m_small && large == m_large))
        return;
    m_small = small;
    m_large = large;
    applyRumble(false);
}

// refresh = periodic call from readState: only backends whose rumble times out need it resent.
void PSPadBackend::applyRumble(bool refresh)
{
#ifdef _WIN32
    // raylib's GLFW backend has no vibration: XInput and Sony HID pads directly. Both keep the motors running until changed.
    if (refresh)
        return;
    ps2PadWindowsRumble(m_large, m_small);
#else
    // Other platforms (SDL, Switch): raylib's rumble, which stops after at most 2 s, so it is resent every second.
    if (m_activeGamepad < 0 || !IsWindowReady())
        return;
    const bool on = m_small || m_large;
    const double now = GetTime();
    if (refresh && (!on || now - m_rumbleSentAt < 1.0))
        return;
    m_rumbleSentAt = now;
    SetGamepadVibration(m_activeGamepad, m_large / 255.0f, m_small ? 1.0f : 0.0f, on ? 2.0f : 0.1f); // zero motors with any duration stop it
#endif
}
