// GOW-Port: Windows gamepad rumble (raylib's GLFW backend has none): XInput pads, and Sony DualShock 4 / DualSense
// pads through their HID output report (they are not XInput devices unless Steam or DS4Windows wraps them). The DLLs
// are loaded at run time, so the runner needs no import libraries; without them (or without a pad) rumble is off.
// The devices are driven from a thread of their own (see ps2PadWindowsRumble): the game only records the motor state.
#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <xinput.h>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <condition_variable>
#include <cstring>
#include <mutex>
#include <string>
#include <thread>
#include <utility>
#include <vector>

namespace
{
bool xinputRumble(uint16_t lowFrequency, uint16_t highFrequency)
{
    using GetStateFn = DWORD(WINAPI *)(DWORD, XINPUT_STATE *);
    using SetStateFn = DWORD(WINAPI *)(DWORD, XINPUT_VIBRATION *);
    static const auto functions = [] {
        HMODULE module = LoadLibraryA("xinput1_4.dll");
        if (!module)
            module = LoadLibraryA("xinput9_1_0.dll");
        return std::pair<GetStateFn, SetStateFn>(
            module ? reinterpret_cast<GetStateFn>(GetProcAddress(module, "XInputGetState")) : nullptr,
            module ? reinterpret_cast<SetStateFn>(GetProcAddress(module, "XInputSetState")) : nullptr);
    }();
    if (!functions.first || !functions.second)
        return false;
    // raylib's gamepad index does not map to an XInput user index; every connected XInput pad gets the state.
    XINPUT_VIBRATION vibration{lowFrequency, highFrequency};
    bool any = false;
    for (DWORD user = 0; user < XUSER_MAX_COUNT; ++user)
    {
        XINPUT_STATE state{};
        if (functions.first(user, &state) == ERROR_SUCCESS)
            any = functions.second(user, &vibration) == ERROR_SUCCESS || any;
    }
    return any;
}

// ---- Sony pads over HID (report layouts as in Linux hid-sony / hid-playstation) ----

struct HidAttributes
{
    ULONG Size;
    USHORT VendorID, ProductID, VersionNumber;
};
struct HidCaps
{
    USHORT Usage, UsagePage, InputReportByteLength, OutputReportByteLength, FeatureReportByteLength, Reserved[17];
    USHORT counts[10];
};
struct DeviceInterfaceData
{
    DWORD cbSize;
    GUID InterfaceClassGuid;
    DWORD Flags;
    ULONG_PTR Reserved;
};

enum class SonyKind
{
    DualShock4,
    DualSense
};

struct SonyPad
{
    HANDLE handle;
    SonyKind kind;
    bool bluetooth;
    size_t reportLength; // the device's output report length (WriteFile needs exactly this size)
    uint8_t sequence;
};

uint32_t crc32(uint32_t crc, const uint8_t *data, size_t size)
{
    crc = ~crc;
    for (size_t i = 0; i < size; ++i)
    {
        crc ^= data[i];
        for (int bit = 0; bit < 8; ++bit)
            crc = (crc >> 1) ^ (0xEDB88320u & (0u - (crc & 1u)));
    }
    return ~crc;
}

// Bluetooth output reports end with a CRC-32 over the HID output header byte 0xA2 and the first 74 report bytes.
void sealBluetoothReport(uint8_t *report)
{
    const uint8_t header = 0xA2;
    const uint32_t crc = crc32(crc32(0, &header, 1), report, 74);
    std::memcpy(report + 74, &crc, 4);
}

std::vector<SonyPad> openSonyPads()
{
    std::vector<SonyPad> pads;
    HMODULE hid = LoadLibraryA("hid.dll"), setup = LoadLibraryA("setupapi.dll");
    if (!hid || !setup)
        return pads;
    using GetHidGuidFn = void(WINAPI *)(GUID *);
    using GetAttributesFn = BOOLEAN(WINAPI *)(HANDLE, HidAttributes *);
    using GetPreparsedFn = BOOLEAN(WINAPI *)(HANDLE, void **);
    using FreePreparsedFn = BOOLEAN(WINAPI *)(void *);
    using GetCapsFn = LONG(WINAPI *)(void *, HidCaps *);
    using GetClassDevsFn = HANDLE(WINAPI *)(const GUID *, const char *, HWND, DWORD);
    using EnumInterfacesFn = BOOL(WINAPI *)(HANDLE, void *, const GUID *, DWORD, DeviceInterfaceData *);
    using GetDetailFn = BOOL(WINAPI *)(HANDLE, DeviceInterfaceData *, void *, DWORD, DWORD *, void *);
    using DestroyListFn = BOOL(WINAPI *)(HANDLE);
    const auto getHidGuid = reinterpret_cast<GetHidGuidFn>(GetProcAddress(hid, "HidD_GetHidGuid"));
    const auto getAttributes = reinterpret_cast<GetAttributesFn>(GetProcAddress(hid, "HidD_GetAttributes"));
    const auto getPreparsed = reinterpret_cast<GetPreparsedFn>(GetProcAddress(hid, "HidD_GetPreparsedData"));
    const auto freePreparsed = reinterpret_cast<FreePreparsedFn>(GetProcAddress(hid, "HidD_FreePreparsedData"));
    const auto getCaps = reinterpret_cast<GetCapsFn>(GetProcAddress(hid, "HidP_GetCaps"));
    const auto getClassDevs = reinterpret_cast<GetClassDevsFn>(GetProcAddress(setup, "SetupDiGetClassDevsA"));
    const auto enumInterfaces = reinterpret_cast<EnumInterfacesFn>(GetProcAddress(setup, "SetupDiEnumDeviceInterfaces"));
    const auto getDetail = reinterpret_cast<GetDetailFn>(GetProcAddress(setup, "SetupDiGetDeviceInterfaceDetailA"));
    const auto destroyList = reinterpret_cast<DestroyListFn>(GetProcAddress(setup, "SetupDiDestroyDeviceInfoList"));
    if (!getHidGuid || !getAttributes || !getPreparsed || !freePreparsed || !getCaps || !getClassDevs || !enumInterfaces || !getDetail ||
        !destroyList)
        return pads;

    GUID hidGuid{};
    getHidGuid(&hidGuid);
    const HANDLE list = getClassDevs(&hidGuid, nullptr, nullptr, 0x2 /*DIGCF_PRESENT*/ | 0x10 /*DIGCF_DEVICEINTERFACE*/);
    if (list == INVALID_HANDLE_VALUE)
        return pads;
    for (DWORD index = 0;; ++index)
    {
        DeviceInterfaceData data{sizeof(DeviceInterfaceData)};
        if (!enumInterfaces(list, nullptr, &hidGuid, index, &data))
            break;
        DWORD size = 0;
        getDetail(list, &data, nullptr, 0, &size, nullptr);
        if (size < sizeof(DWORD) + 2)
            continue;
        std::vector<uint8_t> detail(size);
        *reinterpret_cast<DWORD *>(detail.data()) = sizeof(void *) == 8 ? 8 : 5; // SP_DEVICE_INTERFACE_DETAIL_DATA_A::cbSize
        if (!getDetail(list, &data, detail.data(), size, nullptr, nullptr))
            continue;
        const char *path = reinterpret_cast<const char *>(detail.data() + sizeof(DWORD));
        const HANDLE handle = CreateFileA(path, GENERIC_READ | GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_WRITE, nullptr, OPEN_EXISTING, 0, nullptr);
        if (handle == INVALID_HANDLE_VALUE)
            continue;
        HidAttributes attributes{sizeof(HidAttributes)};
        HidCaps caps{};
        void *preparsed = nullptr;
        bool keep = false;
        if (getAttributes(handle, &attributes) && attributes.VendorID == 0x054C && getPreparsed(handle, &preparsed))
        {
            getCaps(preparsed, &caps);
            freePreparsed(preparsed);
            const USHORT product = attributes.ProductID;
            const bool ds4 = product == 0x05C4 || product == 0x09CC || product == 0x0BA0;
            const bool dualSense = product == 0x0CE6 || product == 0x0DF2;
            // the game-pad collection (usage page 1, usage 5) carries the output report; USB reports are 32 / 48
            // bytes, Bluetooth ones 78 or more
            if ((ds4 || dualSense) && caps.UsagePage == 1 && caps.Usage == 5 && caps.OutputReportByteLength >= 32)
            {
                // Bluetooth: the first output report switches the pad to its extended input report, which DirectInput
                // (raylib's GLFW input) may not read any more, so it is opt-in (GOW_PAD_BT_RUMBLE=1)
                static const bool bluetoothRumble = std::getenv("GOW_PAD_BT_RUMBLE") != nullptr;
                const bool bluetooth = caps.OutputReportByteLength >= 78;
                std::fprintf(stderr, "[pad] rumble: %s over %s%s\n", ds4 ? "DualShock 4" : "DualSense", bluetooth ? "Bluetooth" : "USB",
                             bluetooth && !bluetoothRumble ? " (off: set GOW_PAD_BT_RUMBLE=1)" : "");
                if (!bluetooth || bluetoothRumble)
                {
                    pads.push_back({handle, ds4 ? SonyKind::DualShock4 : SonyKind::DualSense, bluetooth, caps.OutputReportByteLength, 0});
                    keep = true;
                }
            }
        }
        if (!keep)
            CloseHandle(handle);
    }
    destroyList(list);
    return pads;
}

// large = strong (left, low-frequency) motor 0-255; small = weak (right, high-frequency) motor 0-255
bool sendSonyRumble(SonyPad &pad, uint8_t large, uint8_t small)
{
    std::vector<uint8_t> report(pad.reportLength, 0);
    if (pad.kind == SonyKind::DualShock4)
    {
        if (!pad.bluetooth)
        {
            report[0] = 0x05;
            report[1] = 0x01; // flags: motors only (the light bar and its flash stay as they are)
            report[4] = small;
            report[5] = large;
        }
        else
        {
            report[0] = 0x11;
            report[1] = 0xC0; // HID report with CRC
            report[3] = 0x01;
            report[6] = small;
            report[7] = large;
            sealBluetoothReport(report.data());
        }
    }
    else
    {
        // the common part: valid_flag0 (compatible vibration + haptics select), valid_flag1, right motor, left motor;
        // valid_flag2 bit 2 is the vibration flag of newer firmware
        size_t common = 1;
        if (pad.bluetooth)
        {
            report[0] = 0x31;
            report[1] = static_cast<uint8_t>(pad.sequence << 4);
            pad.sequence = static_cast<uint8_t>((pad.sequence + 1u) & 0xFu);
            report[2] = 0x10;
            common = 3;
        }
        else
            report[0] = 0x02;
        report[common + 0] = 0x03;
        report[common + 2] = small;
        report[common + 3] = large;
        report[common + 38] = 0x04;
        if (pad.bluetooth)
            sealBluetoothReport(report.data());
    }
    DWORD written = 0;
    return WriteFile(pad.handle, report.data(), static_cast<DWORD>(report.size()), &written, nullptr) && written == report.size();
}

bool sonyRumble(uint8_t large, uint8_t small)
{
    static std::vector<SonyPad> pads;
    static ULONGLONG lastScan = 0;
    // (re)scan when there is no pad, at most every 2 s, so a pad plugged in later is found
    const ULONGLONG now = GetTickCount64();
    if (pads.empty() && (lastScan == 0 || now - lastScan >= 2000))
    {
        lastScan = now;
        pads = openSonyPads();
    }
    bool any = false;
    for (size_t i = 0; i < pads.size();)
        if (sendSonyRumble(pads[i], large, small))
        {
            any = true;
            ++i;
        }
        else
        {
            CloseHandle(pads[i].handle); // unplugged
            pads.erase(pads.begin() + static_cast<std::ptrdiff_t>(i));
        }
    return any;
}
} // namespace

// GOW-Port: the game asks for rumble on every hit given or taken (scePad2 SetActParam from 0x17C370). Applying it there,
// on the EE thread, cost ~100 ms per hit without a pad (XInputGetState on the four empty slots and a new HID device scan
// for a Sony pad, both slow), and the picture froze on each hit. The latest state is now handed to a thread that applies
// it, so the game never waits for a device; a burst of changes collapses into the last one.
bool ps2PadWindowsRumble(uint8_t large, bool small)
{
    struct Worker
    {
        std::mutex mutex;
        std::condition_variable wake;
        uint8_t large = 0;
        bool small = false;
        bool pending = false;
    };
    static Worker *const worker = [] {
        auto *w = new Worker; // lives for the process, like the thread
        std::thread([w] {
            for (;;)
            {
                uint8_t wantLarge;
                bool wantSmall;
                {
                    std::unique_lock<std::mutex> lock(w->mutex);
                    w->wake.wait(lock, [w] { return w->pending; });
                    w->pending = false;
                    wantLarge = w->large;
                    wantSmall = w->small;
                }
                xinputRumble(static_cast<uint16_t>(wantLarge * 257u), wantSmall ? 0xFFFFu : 0u);
                sonyRumble(wantLarge, wantSmall ? 0xFFu : 0u);
            }
        }).detach();
        return w;
    }();
    {
        std::lock_guard<std::mutex> lock(worker->mutex);
        worker->large = large;
        worker->small = small;
        worker->pending = true;
    }
    worker->wake.notify_one();
    return true;
}
#elif defined(__linux__)
// GOW-Port: Linux gamepad rumble through kernel evdev force-feedback (FF_RUMBLE on /dev/input/event*).
// Covers Xbox (xpad), Sony DualShock 4 / DualSense (hid-playstation / hid-sony), Nintendo, 8BitDo and Steam Virtual
// Gamepads without external libraries.
#include <chrono>
#include <cctype>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <dirent.h>
#include <fcntl.h>
#include <linux/input.h>
#include <string>
#include <sys/ioctl.h>
#include <unistd.h>
#include <vector>

namespace
{
constexpr uint16_t kRumbleReplayLengthMs = 4000u;

struct LinuxPad
{
    int fd = -1;
    int effectId = -1;
};

bool testBit(int bit, const unsigned long *bits)
{
    constexpr size_t kBitsPerLong = sizeof(unsigned long) * 8u;
    return ((bits[static_cast<size_t>(bit) / kBitsPerLong] >> (static_cast<size_t>(bit) % kBitsPerLong)) & 1ul) != 0ul;
}

bool isEventDeviceName(const char *name)
{
    if (std::strncmp(name, "event", 5) != 0 || name[5] == '\0')
        return false;
    for (const char *p = name + 5; *p != '\0'; ++p)
        if (!std::isdigit(static_cast<unsigned char>(*p)))
            return false;
    return true;
}

std::vector<LinuxPad> openLinuxPads()
{
    std::vector<LinuxPad> pads;
    DIR *dir = opendir("/dev/input");
    if (!dir)
        return pads;

    while (const dirent *entry = readdir(dir))
    {
        if (!isEventDeviceName(entry->d_name))
            continue;
        const std::string path = std::string("/dev/input/") + entry->d_name;
        const int fd = open(path.c_str(), O_RDWR | O_CLOEXEC | O_NONBLOCK);
        if (fd < 0)
            continue;

        unsigned long ffBits[(FF_MAX + sizeof(unsigned long) * 8u) / (sizeof(unsigned long) * 8u)]{};
        if (ioctl(fd, EVIOCGBIT(EV_FF, sizeof(ffBits)), ffBits) < 0 || !testBit(FF_RUMBLE, ffBits))
        {
            close(fd);
            continue;
        }

        ff_effect effect{};
        effect.type = FF_RUMBLE;
        effect.id = -1;
        effect.replay.length = kRumbleReplayLengthMs;
        effect.replay.delay = 0;
        if (ioctl(fd, EVIOCSFF, &effect) < 0 || effect.id < 0)
        {
            close(fd);
            continue;
        }

        char name[128] = "Gamepad";
        if (ioctl(fd, EVIOCGNAME(sizeof(name)), name) < 0 || name[0] == '\0')
            std::snprintf(name, sizeof(name), "Gamepad");
        std::fprintf(stderr, "[pad] rumble: %s (%s)\n", name, path.c_str());
        pads.push_back({fd, effect.id});
    }
    closedir(dir);
    return pads;
}

bool sendLinuxRumble(LinuxPad &pad, uint16_t strong, uint16_t weak, uint16_t lengthMs = kRumbleReplayLengthMs)
{
    const bool active = strong != 0u || weak != 0u;
    if (active)
    {
        ff_effect effect{};
        effect.type = FF_RUMBLE;
        effect.id = static_cast<int16_t>(pad.effectId);
        effect.u.rumble.strong_magnitude = strong;
        effect.u.rumble.weak_magnitude = weak;
        effect.replay.length = lengthMs;
        effect.replay.delay = 0;
        if (ioctl(pad.fd, EVIOCSFF, &effect) < 0)
            return false;
    }

    input_event play{};
    play.type = EV_FF;
    play.code = static_cast<uint16_t>(pad.effectId);
    play.value = active ? 1 : 0;
    return write(pad.fd, &play, sizeof(play)) == static_cast<ssize_t>(sizeof(play));
}
} // namespace

bool ps2PadLinuxRumble(uint8_t large, bool small)
{
    static std::vector<LinuxPad> pads;
    static std::chrono::steady_clock::time_point lastScan{};
    static std::chrono::steady_clock::time_point lastPulseStart{};
    static uint16_t lastStrong = 0u, lastWeak = 0u;
    static bool scannedOnce = false;

    const auto now = std::chrono::steady_clock::now();
    uint16_t strong = large ? static_cast<uint16_t>(16384u + large * 192u) : (small ? 24576u : 0u);
    uint16_t weak = (small || large > 0u) ? 0xFFFFu : 0u;
    uint16_t lengthMs = kRumbleReplayLengthMs;

    if (strong != 0u || weak != 0u)
    {
        if (lastStrong == 0u && lastWeak == 0u)
            lastPulseStart = now;
        lastStrong = strong;
        lastWeak = weak;
    }
    else if ((lastStrong != 0u || lastWeak != 0u) && now - lastPulseStart < std::chrono::milliseconds(90))
    {
        // Let 1-4 frame pulses finish spinning (~90 ms) before the kernel stops the motor.
        strong = lastStrong;
        weak = lastWeak;
        lengthMs = 80u;
        lastStrong = lastWeak = 0u;
    }
    else
    {
        lastStrong = lastWeak = 0u;
    }

    if (pads.empty() && (strong != 0u || weak != 0u))
    {
        if (!scannedOnce || now - lastScan >= std::chrono::seconds(2))
        {
            scannedOnce = true;
            lastScan = now;
            pads = openLinuxPads();
        }
    }

    bool any = false;
    for (size_t i = 0; i < pads.size();)
    {
        if (sendLinuxRumble(pads[i], strong, weak, lengthMs))
        {
            any = true;
            ++i;
        }
        else
        {
            close(pads[i].fd); // unplugged
            pads.erase(pads.begin() + static_cast<std::ptrdiff_t>(i));
        }
    }
    return any;
}
#endif
