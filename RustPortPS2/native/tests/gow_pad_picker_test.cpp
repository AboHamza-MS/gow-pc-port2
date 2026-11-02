// Drives the real pad code (ps2xRuntime/src/lib/ps2_pad.cpp) against fake GLFW and raylib functions: the controller picker (the
// device list file, the chosen device, the fallbacks) and PC_PAD_MAP through PSPadBackend::readState. No window, GPU, pad or game
// files are needed. It includes ps2_pad.cpp itself, so it only needs raylib.h for the types and constants (a raylib checkout or the
// `_deps/raylib-src/src` folder of a runtime build); nothing from raylib is linked.
//
//   MSVC:  cl /nologo /std:c++20 /EHsc /I ..\ps2xRuntime\include /I ..\ps2xRuntime\src\lib /I <raylib>\src gow_pad_picker_test.cpp
//   run it once per scenario (the pad code reads its environment once, the first time):
//          gow_pad_picker_test.exe none | chosen | absent | far | map | badmap
//
// Not registered in CMake on purpose (a CMake edit recompiles the whole runtime).
#include "../ps2xRuntime/src/lib/ps2_pad.cpp"

#include <fstream>
#include <sstream>

using namespace gowpad; // the raylib button ids (BtnRightFaceDown...)

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

// ------------------------------------------------------------------------------------------------ the fake world

struct FakeJoystick
{
    bool present = false;
    std::string name, guid;
    bool mapped = false;
    int buttons = 0, hats = 0, axes = 0;
    unsigned char rawButtons[32]{};
    unsigned char rawHats[8]{};
    float rawAxes[12]{};
    bool down[32]{};    // raylib button ids held
    bool pressed[32]{}; // raylib button ids that went down since the last read
};
static FakeJoystick g_joy[16];
static double g_time = 0.0;
static std::vector<std::string> g_mappings; // what the pad code handed to glfwUpdateGamepadMappings
static bool g_acceptMappings = true;

extern "C"
{
    int glfwJoystickPresent(int jid) { return g_joy[jid].present; }
    int glfwJoystickIsGamepad(int jid) { return g_joy[jid].mapped; }
    const char *glfwGetJoystickName(int jid) { return g_joy[jid].name.c_str(); }
    const char *glfwGetJoystickGUID(int jid) { return g_joy[jid].guid.c_str(); }
    const char *glfwGetGamepadName(int jid) { return g_joy[jid].name.c_str(); }
    const float *glfwGetJoystickAxes(int jid, int *count)
    {
        *count = g_joy[jid].axes;
        return g_joy[jid].rawAxes;
    }
    const unsigned char *glfwGetJoystickButtons(int jid, int *count)
    {
        *count = g_joy[jid].buttons;
        return g_joy[jid].rawButtons;
    }
    const unsigned char *glfwGetJoystickHats(int jid, int *count)
    {
        *count = g_joy[jid].hats;
        return g_joy[jid].rawHats;
    }
    int glfwGetGamepadState(int, GLFWgamepadstate *) { return 0; }
    int glfwUpdateGamepadMappings(const char *mapping)
    {
        g_mappings.push_back(mapping);
        return g_acceptMappings ? 1 : 0;
    }

    bool IsKeyDown(int) { return false; }
    bool IsMouseButtonDown(int) { return false; }
    bool IsGamepadAvailable(int gamepad) { return gamepad >= 0 && gamepad < 4 && g_joy[gamepad].present; }
    bool IsGamepadButtonPressed(int gamepad, int button) { return g_joy[gamepad].pressed[button]; }
    bool IsGamepadButtonDown(int gamepad, int button) { return g_joy[gamepad].down[button]; }
    const char *GetGamepadName(int gamepad) { return g_joy[gamepad].name.c_str(); }
    float GetGamepadAxisMovement(int, int) { return 0.0f; }
    void SetGamepadVibration(int, float, float, float) {}
    bool IsWindowReady(void) { return false; }
    double GetTime(void) { return g_time; }
}
#ifdef _WIN32
bool ps2PadWindowsRumble(uint8_t, bool) { return true; }
#endif

// The dongle of issue #12: three interfaces of the dongle, then the real pad ("Generic X-Box pad").
static const char *kPadGuid = "0300dddd5e040000a102000000010000";
static void connectDongle(int padSlot)
{
    const char *names[4] = {"2.4G Consumer Control", "2.4G Keyboard", "2.4G Mouse", "Generic X-Box pad"};
    const char *guids[4] = {"0300aaaa5e040000a102000000010000", "0300bbbb5e040000a102000000010000", "0300cccc5e040000a102000000010000", kPadGuid};
    for (int i = 0; i < 4; ++i)
    {
        const int slot = i == 3 ? padSlot : i;
        g_joy[slot].present = true;
        g_joy[slot].name = names[i];
        g_joy[slot].guid = guids[i];
        g_joy[slot].mapped = i != 3;
        g_joy[slot].buttons = i == 3 ? 15 : 1;
        g_joy[slot].hats = i == 3 ? 1 : 0;
        g_joy[slot].axes = i == 3 ? 6 : 0;
    }
}

static std::string readFile(const std::filesystem::path &path)
{
    std::ifstream in(path, std::ios::binary);
    std::stringstream text;
    text << in.rdbuf();
    return text.str();
}

// One read of the pad as the game does it; returns the 16-bit active-low word.
static uint16_t readWord(PSPadBackend &backend, int *activeOut = nullptr)
{
    uint8_t data[32];
    CHECK(backend.readState(0, 0, data, sizeof(data)));
    for (FakeJoystick &joy : g_joy)
        std::memset(joy.pressed, 0, sizeof(joy.pressed));
    if (activeOut)
        *activeOut = g_activeJoystick.load();
    return static_cast<uint16_t>(data[2] | data[3] << 8);
}

static void frames(int count)
{
    for (int i = 0; i < count; ++i)
    {
        g_time += 1.0 / 60.0;
        gowPadDiagFrame();
    }
}

#ifdef _WIN32
static void setEnv(const char *name, const char *value) { _putenv_s(name, value); }
#else
static void setEnv(const char *name, const char *value) { setenv(name, value, 1); }
#endif

int main(int argc, char **argv)
{
    const std::string scenario = argc > 1 ? argv[1] : "chosen";
    const std::filesystem::path list = std::filesystem::temp_directory_path() / "gow_pad_picker_test" / "pads" / "devices.json";
    std::filesystem::remove_all(list.parent_path().parent_path());
    PSPadBackend backend;

    if (scenario == "none")
    {
        // Without the two variables the picker does nothing: no file, the game's own choice (first available slot, then the last
        // pressed).
        connectDongle(3);
        frames(65);
        CHECK(!std::filesystem::exists(list));
        int active = -2;
        readWord(backend, &active);
        CHECK(active == 0);
        g_joy[3].pressed[BtnRightFaceDown] = true;
        g_joy[3].down[BtnRightFaceDown] = true;
        readWord(backend, &active);
        CHECK(active == 3);
    }
    else if (scenario == "chosen")
    {
        setEnv("GOW_PAD_LIST_FILE", list.string().c_str());
        setEnv("GOW_PAD_DEVICE", "0300DDDD5E040000A102000000010000"); // any case
        connectDongle(3);
        frames(1);
        // The list is written at once, with every joystick, whether it has an SDL mapping, and the pad found.
        const std::string text = readFile(list);
        CHECK(text.find("\"choice\":\"found\"") != std::string::npos);
        CHECK(text.find("\"name\":\"Generic X-Box pad\",\"guid\":\"0300dddd5e040000a102000000010000\",\"mapped\":false,\"buttons\":11,\"hats\":1,\"axes\":6") != std::string::npos); // GLFW says 15 (11 + 4 for the hat): 11 are real
        CHECK(text.find("\"name\":\"2.4G Keyboard\"") != std::string::npos);
        CHECK(text.find("\"preferred\":\"0300DDDD5E040000A102000000010000\"") != std::string::npos);
        // Pressing a button on another device does not take over; the chosen one is read from the first read on.
        g_joy[0].pressed[BtnRightFaceLeft] = true;
        g_joy[0].down[BtnRightFaceLeft] = true; // X on the keyboard interface: ignored
        g_joy[3].down[BtnRightFaceDown] = true; // A on the chosen pad: cross
        int active = -2;
        const uint16_t word = readWord(backend, &active);
        CHECK(active == 3);
        CHECK(word == static_cast<uint16_t>(0xFFFF & ~0x4000));
        // The next refresh says which device the game reads.
        frames(30);
        CHECK(readFile(list).find("\"active\":3") != std::string::npos);
        // Unplugged: the list changes, and the game falls back to the automatic choice without crashing.
        g_joy[3] = FakeJoystick{};
        frames(30);
        const std::string after = readFile(list);
        CHECK(after.find("Generic X-Box pad") == std::string::npos);
        CHECK(after.find("\"choice\":\"absent\"") != std::string::npos);
        readWord(backend, &active);
        CHECK(active == 0);
        // Plugged in again on another slot: found again.
        connectDongle(2 /* the pad takes joystick 2 now */);
        frames(30);
        CHECK(readFile(list).find("\"choice\":\"found\"") != std::string::npos);
        g_joy[2].down[BtnRightFaceRight] = true;
        readWord(backend, &active);
        CHECK(active == 2);
    }
    else if (scenario == "absent")
    {
        setEnv("GOW_PAD_LIST_FILE", list.string().c_str());
        setEnv("GOW_PAD_DEVICE", "0300eeee5e040000a102000000010000");
        connectDongle(3);
        frames(1);
        CHECK(readFile(list).find("\"choice\":\"absent\"") != std::string::npos);
        // The automatic choice is untouched: slot 0 until a button goes down elsewhere.
        int active = -2;
        readWord(backend, &active);
        CHECK(active == 0);
        g_joy[3].pressed[BtnRightFaceDown] = true;
        readWord(backend, &active);
        CHECK(active == 3);
    }
    else if (scenario == "far")
    {
        setEnv("GOW_PAD_LIST_FILE", list.string().c_str());
        setEnv("GOW_PAD_DEVICE", kPadGuid);
        // Five joysticks: the pad is joystick 5, which raylib's four slots do not reach.
        connectDongle(5);
        g_joy[3].present = true;
        g_joy[3].name = "Other pad";
        g_joy[3].guid = "0300ffff5e040000a102000000010000";
        g_joy[3].mapped = true;
        frames(1);
        const std::string text = readFile(list);
        CHECK(text.find("\"choice\":\"outOfRange\"") != std::string::npos);
        CHECK(text.find("\"index\":5") != std::string::npos);
        int active = -2;
        readWord(backend, &active);
        CHECK(active == 0); // the automatic choice
    }
    else if (scenario == "mapping" || scenario == "rejected" || scenario == "badmapping")
    {
        // The launcher's hand-made mapping for the chosen device: handed to GLFW once, with the platform added by the runtime.
        const std::string fields = "a:b0,b:b1,start:b7,dpup:h0.1,leftx:a0,lefttrigger:+a2";
        const std::string mapping = std::string(kPadGuid) + ",Generic X-Box pad," + fields;
        setEnv("GOW_PAD_SDL_MAPPING", scenario == "badmapping" ? "not a mapping" : mapping.c_str());
        g_acceptMappings = scenario != "rejected";
        connectDongle(3);
        frames(90);
        if (scenario == "badmapping")
            CHECK(g_mappings.empty());
        else
        {
            CHECK(g_mappings.size() == 1); // once, however many frames pass
#ifdef _WIN32
            CHECK(!g_mappings.empty() && g_mappings[0] == mapping + ",platform:Windows,\n");
#else
            CHECK(!g_mappings.empty() && g_mappings[0] == mapping + ",platform:Linux,\n");
#endif
        }
    }
    else if (scenario == "live")
    {
        // The raw input feed for the mapping dialog: only while a fresh `probe` file is next to the device list.
        setEnv("GOW_PAD_LIST_FILE", list.string().c_str());
        const std::filesystem::path probe = list.parent_path() / "probe";
        const std::filesystem::path live = list.parent_path() / "live.json";
        connectDongle(3);
        g_joy[3].rawAxes[2] = -1.0f; // a trigger that rests at -1, as on many pads
        frames(65);
        CHECK(!std::filesystem::exists(live)); // nobody asked
        std::ofstream(probe) << "x";
        frames(30);
        std::string text = readFile(live);
        CHECK(text.find("\"schema\":1") != std::string::npos);
        CHECK(text.find("\"index\":3") != std::string::npos);
        CHECK(text.find("\"guid\":\"" + std::string(kPadGuid) + "\",\"buttons\":[],\"hats\":[],\"axes\":[]") != std::string::npos);
        // Raw button 2, hat 0 up and axis 2 (a trigger resting at -1) pressed on the real pad: reported with GLFW's own numbers.
        g_joy[3].rawButtons[2] = 1;
        g_joy[3].rawButtons[11] = 1; // what GLFW adds for hat 0 up (real buttons 0 to 10, then four per hat): never a real button
        g_joy[3].rawHats[0] = 1;
        g_joy[3].rawAxes[2] = 1.0f;
        frames(10);
        text = readFile(live);
        const std::string pad = text.substr(text.find("\"index\":3"));
        CHECK(pad.find("\"buttons\":[2]") != std::string::npos); // not [2,11]: a mapping naming b11 is dropped whole by GLFW
        CHECK(pad.find("\"hats\":[[0,1]]") != std::string::npos);
        CHECK(pad.find("[2,200]") != std::string::npos); // -1 -> +1: a distance of 2.00 from where the trigger rested
        // Released: back to nothing.
        g_joy[3].rawButtons[2] = 0;
        g_joy[3].rawButtons[11] = 0;
        g_joy[3].rawHats[0] = 0;
        g_joy[3].rawAxes[2] = -1.0f;
        frames(10);
        CHECK(readFile(live).substr(readFile(live).find("\"index\":3")).find("\"buttons\":[]") != std::string::npos);
        // The beat keeps changing while quiet (the launcher tells a quiet pad from a game that stopped).
        const std::string quiet = readFile(live);
        frames(60);
        CHECK(readFile(live) != quiet);
        // A probe file that nobody touches for ten seconds is stale and ignored: the feed stops.
        std::error_code error;
        std::filesystem::last_write_time(probe, std::filesystem::file_time_type::clock::now() - std::chrono::seconds(30), error);
        frames(35);
        const std::string stopped = readFile(live);
        g_joy[3].rawButtons[5] = 1;
        frames(60);
        CHECK(readFile(live) == stopped);
    }
    else if (scenario == "map" || scenario == "badmap")
    {
        setEnv("PC_PAD_MAP", scenario == "map" ? "a=circle;b=cross;lt=start;x=none;start=start" : "a=circle;b=cross");
        setEnv("PC_PAD_FACE_MAP", "cross,triangle,circle,square");
        g_joy[0].present = true;
        g_joy[0].name = "Pad";
        g_joy[0].guid = "0300aaaa5e040000a102000000010000";
        g_joy[0].mapped = true;
        int active = -2;
        g_joy[0].down[BtnRightFaceDown] = true; // A
        const uint16_t a = readWord(backend, &active);
        g_joy[0].down[BtnRightFaceDown] = false;
        g_joy[0].down[BtnRightFaceLeft] = true; // X
        const uint16_t x = readWord(backend, &active);
        g_joy[0].down[BtnRightFaceLeft] = false;
        g_joy[0].down[BtnLeftTrigger2] = true; // LT
        const uint16_t lt = readWord(backend, &active);
        g_joy[0].down[BtnLeftTrigger2] = false;
        g_joy[0].down[BtnRightFaceUp] = true; // Y
        const uint16_t y = readWord(backend, &active);
        if (scenario == "map")
        {
            CHECK(a == static_cast<uint16_t>(0xFFFF & ~0x2000)); // a=circle, not the face map's cross
            CHECK(x == 0xFFFF);                                   // x=none
            CHECK(lt == static_cast<uint16_t>(0xFFFF & ~0x0008)); // lt=start
            CHECK(y == 0xFFFF);                                   // not listed: nothing
        }
        else
        {
            // Invalid (no start): ignored, so the face map "cross,triangle,circle,square" is in force: A=cross, X=circle, Y=square.
            CHECK(a == static_cast<uint16_t>(0xFFFF & ~0x4000));
            CHECK(x == static_cast<uint16_t>(0xFFFF & ~0x2000));
            CHECK(lt == static_cast<uint16_t>(0xFFFF & ~0x0100)); // classic shoulders: LT is L2
            CHECK(y == static_cast<uint16_t>(0xFFFF & ~0x8000));
        }
    }
    else
    {
        std::printf("unknown scenario %s\n", scenario.c_str());
        return 2;
    }
    if (g_failures == 0)
        std::printf("PASS: %s\n", scenario.c_str());
    return g_failures == 0 ? 0 : 1;
}
