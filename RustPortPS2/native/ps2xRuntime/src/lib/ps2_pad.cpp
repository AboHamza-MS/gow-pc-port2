#include "runtime/ps2_pad.h"
#include "ps2_host_backend.h"
#include <algorithm>
#include <array>
#include <cmath>
#include <cstdarg>
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

// GOW-Port: GOW_PAD_DIAG=1 settles "my pad does nothing" reports. Every [gow-paddiag] line says what GLFW and raylib see for
// each joystick (name, GUID, SDL mapping, then raw buttons / hats / axes, the mapped state and raylib's buttons as they
// change) and what the game's pad read returns. Without the variable nothing is logged and nothing is called. GLFW is linked
// into raylib; its header is not on this file's include path on Windows, so the few calls used are declared here.
extern "C"
{
    struct GLFWgamepadstate
    {
        unsigned char buttons[15];
        float axes[6];
    };
    int glfwJoystickPresent(int jid);
    int glfwJoystickIsGamepad(int jid);
    const char *glfwGetJoystickName(int jid);
    const char *glfwGetJoystickGUID(int jid);
    const char *glfwGetGamepadName(int jid);
    const float *glfwGetJoystickAxes(int jid, int *count);
    const unsigned char *glfwGetJoystickButtons(int jid, int *count);
    const unsigned char *glfwGetJoystickHats(int jid, int *count);
    int glfwGetGamepadState(int jid, GLFWgamepadstate *state);
}

namespace
{
    bool padDiagEnabled()
    {
        static const bool enabled = [] {
            const char *value = std::getenv("GOW_PAD_DIAG");
            return value && *value && std::strcmp(value, "0") != 0;
        }();
        return enabled;
    }

    constexpr int kDiagJoysticks = 16;  // GLFW_JOYSTICK_LAST + 1
    constexpr int kDiagMaxButtons = 32;
    constexpr int kDiagMaxHats = 8;
    constexpr int kDiagMaxAxes = 12;
    constexpr int kDiagMaxLines = 600; // a flood guard: a drifting stick must not fill the log

    struct JoystickDiag
    {
        bool known = false;
        unsigned char buttons[kDiagMaxButtons]{};
        unsigned char hats[kDiagMaxHats]{};
        float axes[kDiagMaxAxes]{};
        GLFWgamepadstate mapped{};
        bool mappedValid = false;
    };

    // Names of the SDL-mapped buttons, in GLFW_GAMEPAD_BUTTON_* order.
    constexpr const char *kMappedButtons[15] = {"A", "B", "X", "Y", "LB", "RB", "Back", "Start", "Guide", "LThumb", "RThumb",
                                                "DUp", "DRight", "DDown", "DLeft"};
    constexpr const char *kMappedAxes[6] = {"LX", "LY", "RX", "RY", "LT", "RT"};

    int g_diagLines = 0;
    int g_diagInputChanges = 0;

    // One log line; `change` marks a button / hat / axis change (as opposed to the device description).
    void diagLine(bool change, const char *format, ...)
    {
        if (change)
            ++g_diagInputChanges;
        if (g_diagLines > kDiagMaxLines)
            return;
        if (g_diagLines++ == kDiagMaxLines)
        {
            std::fprintf(stderr, "[gow-paddiag] line limit reached, no more changes are logged\n");
            return;
        }
        va_list args;
        va_start(args, format);
        std::vfprintf(stderr, format, args);
        va_end(args);
    }
}

// GOW-Port: window loop (gowFrameInput), the thread that owns the raylib window: GLFW's joystick calls belong to it.
void gowPadDiagFrame()
{
    if (!padDiagEnabled())
        return;
    static JoystickDiag joysticks[kDiagJoysticks];
    static bool idleReported = false;
    static bool headerPrinted = false;
    const double now = GetTime();
    if (!headerPrinted)
    {
        headerPrinted = true;
        const char *keys = std::getenv("PC_KEYS");
        const char *face = std::getenv("PC_PAD_FACE_MAP");
        const char *dmc5 = std::getenv("PC_DMC5_CONTROLS");
        std::fprintf(stderr, "[gow-paddiag] started: PC_KEYS=%s PC_DMC5_CONTROLS=%s PC_PAD_FACE_MAP=%s; the keyboard "
                             "is read together with the gamepad (F1/F2 and the cheat menu are host features, not game input)\n",
                     keys && *keys ? "set" : "unset", dmc5 ? dmc5 : "unset", face && *face ? face : "unset");
    }
    int present = 0;
    for (int jid = 0; jid < kDiagJoysticks; ++jid)
    {
        JoystickDiag &joy = joysticks[jid];
        if (!glfwJoystickPresent(jid))
        {
            if (joy.known)
                diagLine(false, "[gow-paddiag] t=%.2f joystick %d disconnected\n", now, jid);
            joy = JoystickDiag{};
            continue;
        }
        ++present;
        int buttonCount = 0, hatCount = 0, axisCount = 0;
        const unsigned char *buttons = glfwGetJoystickButtons(jid, &buttonCount);
        const unsigned char *hats = glfwGetJoystickHats(jid, &hatCount);
        const float *axes = glfwGetJoystickAxes(jid, &axisCount);
        buttonCount = buttons ? std::min(buttonCount, kDiagMaxButtons) : 0;
        hatCount = hats ? std::min(hatCount, kDiagMaxHats) : 0;
        axisCount = axes ? std::min(axisCount, kDiagMaxAxes) : 0;
        GLFWgamepadstate mapped{};
        const bool isGamepad = glfwJoystickIsGamepad(jid) != 0;
        const bool mappedValid = isGamepad && glfwGetGamepadState(jid, &mapped) != 0;
        if (!joy.known)
        {
            joy.known = true;
            const char *name = glfwGetJoystickName(jid);
            const char *guid = glfwGetJoystickGUID(jid);
            const char *mapping = isGamepad ? glfwGetGamepadName(jid) : nullptr;
            diagLine(false, "[gow-paddiag] t=%.2f joystick %d: name=\"%s\" guid=%s sdl_mapping=%s (%s) buttons=%d hats=%d axes=%d raylib_slot_available=%d\n",
                now, jid, name ? name : "?", guid ? guid : "?", isGamepad ? "yes" : "NO", mapping ? mapping : "-", buttonCount, hatCount,
                axisCount, jid < 4 ? static_cast<int>(IsGamepadAvailable(jid)) : -1);
            std::string rest = "[gow-paddiag]   resting axes:";
            for (int a = 0; a < axisCount; ++a)
                rest += " " + std::to_string(a) + "=" + std::to_string(axes[a]).substr(0, 5);
            std::fprintf(stderr, "%s\n", rest.c_str());
            std::memset(joy.buttons, 0, sizeof(joy.buttons));
            std::memset(joy.hats, 0, sizeof(joy.hats));
            for (int a = 0; a < kDiagMaxAxes; ++a)
                joy.axes[a] = a < axisCount ? axes[a] : 0.0f;
            joy.mapped = mapped;
            joy.mappedValid = mappedValid;
            if (!isGamepad)
                std::fprintf(stderr, "[gow-paddiag]   GLFW has no SDL mapping for this device: raylib lists it as a gamepad "
                                     "(it only checks that the joystick is present) but every mapped button reads as released\n");
        }
        for (int b = 0; b < buttonCount; ++b)
            if (buttons[b] != joy.buttons[b])
            {
                diagLine(true, "[gow-paddiag] t=%.2f joystick %d raw button %d %s\n", now, jid, b, buttons[b] ? "down" : "up");
                joy.buttons[b] = buttons[b];
            }
        for (int h = 0; h < hatCount; ++h)
            if (hats[h] != joy.hats[h])
            {
                diagLine(true, "[gow-paddiag] t=%.2f joystick %d raw hat %d = 0x%x\n", now, jid, h, static_cast<unsigned>(hats[h]));
                joy.hats[h] = hats[h];
            }
        for (int a = 0; a < axisCount; ++a)
            if (std::fabs(axes[a] - joy.axes[a]) >= 0.25f)
            {
                diagLine(true, "[gow-paddiag] t=%.2f joystick %d raw axis %d = %.2f\n", now, jid, a, static_cast<double>(axes[a]));
                joy.axes[a] = axes[a];
            }
        if (mappedValid)
        {
            for (int b = 0; b < 15; ++b)
                if (mapped.buttons[b] != joy.mapped.buttons[b])
                    diagLine(true, "[gow-paddiag] t=%.2f joystick %d mapped button %s %s\n", now, jid, kMappedButtons[b], mapped.buttons[b] ? "down" : "up");
            for (int a = 0; a < 6; ++a)
                if (std::fabs(mapped.axes[a] - joy.mapped.axes[a]) >= 0.25f)
                    diagLine(true, "[gow-paddiag] t=%.2f joystick %d mapped axis %s = %.2f\n", now, jid, kMappedAxes[a], static_cast<double>(mapped.axes[a]));
            joy.mapped = mapped;
        }
        joy.mappedValid = mappedValid;
    }
    // What raylib reports for its four gamepad slots (the ones the game reads).
    static bool raylibDown[4][GAMEPAD_BUTTON_RIGHT_THUMB + 1]{};
    for (int slot = 0; slot < 4; ++slot)
        if (IsGamepadAvailable(slot))
            for (int button = GAMEPAD_BUTTON_LEFT_FACE_UP; button <= GAMEPAD_BUTTON_RIGHT_THUMB; ++button)
            {
                const bool down = IsGamepadButtonDown(slot, button);
                if (down != raylibDown[slot][button])
                {
                    diagLine(true, "[gow-paddiag] t=%.2f raylib gamepad %d button %d %s\n", now, slot, button, down ? "down" : "up");
                    raylibDown[slot][button] = down;
                }
            }
    if (!idleReported && now >= 20.0 && g_diagInputChanges == 0)
    {
        idleReported = true;
        std::fprintf(stderr, "[gow-paddiag] t=%.2f no joystick input change seen yet (%d joystick(s) present)\n", now, present);
    }
}

// GOW-Port: sticks from the configurable keyboard, in libpad2 packet order (RX, RY, LX, LY). It is used by the
// wrapper of God of War's pad read (ps2_runtime.cpp), because gowPad2Read sets its own fixed sticks.
// `sticks` holds the current values on entry. Without a gamepad the keyboard sets all four axes; with one (GLFW can list
// devices that never send input, such as dongles or virtual Steam pads) only the axes the keyboard moves replace the
// gamepad's, so the keyboard still works and a working pad is not overridden.
bool PCKeyboardSticks(uint8_t (&sticks)[4])
{
    const KeyMap &map = keyMap();
    if (!map.enabled)
        return false;
    bool gamepad = false;
    for (int i = 0; i < 4 && !gamepad; ++i)
        gamepad = IsGamepadAvailable(i);
    uint8_t keys[4];
    const uint8_t x = axisValue(isActionPressed(map, ActionLeftStickLeft), isActionPressed(map, ActionLeftStickRight));
    const uint8_t y = axisValue(isActionPressed(map, ActionLeftStickUp), isActionPressed(map, ActionLeftStickDown));
    if (isActionPressed(map, ActionRightStickModifier))
    {
        keys[0] = x;
        keys[1] = y;
        keys[2] = keys[3] = kPadStickCenter;
    }
    else
    {
        keys[0] = axisValue(isActionPressed(map, ActionRightStickLeft), isActionPressed(map, ActionRightStickRight));
        keys[1] = axisValue(isActionPressed(map, ActionRightStickUp), isActionPressed(map, ActionRightStickDown));
        keys[2] = x;
        keys[3] = y;
    }
    for (int i = 0; i < 4; ++i)
        if (!gamepad || keys[i] != kPadStickCenter)
            sticks[i] = keys[i];
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

    // GOW-Port: the keyboard works next to a gamepad too (buttons are active-low, so both sources simply add up). Before, any
    // listed gamepad switched it off, and a device GLFW lists without ever sending input (a dongle, a virtual Steam pad)
    // left the game with no input at all (issue #12).
    if (keyMap().enabled)
    {
        // GOW-Port: configurable keyboard (launcher).
        const KeyMap &map = keyMap();
        const bool moving = isLeftStickMoving(map);
        for (size_t action = 0; action <= ActionFury; ++action)
            if (isButtonPressed(map, action, moving))
                clearBit(kActionBits[action]);
        uint8_t sticks[4] = {data[4], data[5], data[6], data[7]};
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

    // GOW-Port: GOW_PAD_DIAG=1: what this read hands to the game, whenever the source or the buttons change (buttons are
    // active-low: ffff = nothing pressed). source "keyboard" means no gamepad was available.
    if (padDiagEnabled())
    {
        static int s_diagSource = -3;
        static uint16_t s_diagButtons = 0;
        const int source = useGamepad ? kGamepad : (keyMap().enabled ? -2 : -1);
        if (source != s_diagSource || btns != s_diagButtons)
        {
            s_diagSource = source;
            s_diagButtons = btns;
            const std::string name = useGamepad ? "gamepad " + std::to_string(kGamepad) : (source == -2 ? "keyboard (PC_KEYS map)" : "keyboard (fixed keys)");
            std::fprintf(stderr, "[gow-paddiag] game read: source=%s buttons=%04x sticks(rx,ry,lx,ly)=%u,%u,%u,%u\n", name.c_str(),
                         static_cast<unsigned>(btns), data[4], data[5], data[6], data[7]);
        }
    }

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
