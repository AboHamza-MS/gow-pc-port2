// GOW-Port: sampling profiler for the process on Windows.
//   GOW_SAMPLING=<file>          enables sampling and is the output file
//   GOW_SAMPLING_FROM=<s>       seconds after startup before sampling begins (default 0)
//   GOW_SAMPLING_SEC=<s>         sampling duration (default 20)
//   GOW_SAMPLING_US=<µs>         interval between samples (default 2000)
// NOTE: env var names and the output file format below are a contract with
// ps2-dev sampling-win and must be changed together.
// Each interval briefly suspends every thread in the process, reads its instruction pointer and
// resumes it. Nothing is allocated or locked while a thread is suspended (it could hold the heap lock).
// On completion it writes the file read by ps2-dev sampling-win together with
// gow-2005.map (CMake option PS2X_MAP).
#include <atomic>
#include <chrono>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <map>
#include <string>
#include <thread>
#include <utility>

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <psapi.h>
#include <tlhelp32.h>
#include <mmsystem.h>

namespace
{
    struct SampledThread
    {
        HANDLE handle = nullptr;
        uint64_t samples = 0;
        uint64_t startCpuMs = 0; // CPU ms at the start
        std::string name;
    };

    uint64_t cpuMs(HANDLE h)
    {
        FILETIME creationTime, exitTime, kernelTime, userTime;
        if (!GetThreadTimes(h, &creationTime, &exitTime, &kernelTime, &userTime))
            return 0;
        const auto a = [](const FILETIME &f) { return (static_cast<uint64_t>(f.dwHighDateTime) << 32) | f.dwLowDateTime; };
        return (a(kernelTime) + a(userTime)) / 10000u; // 100 ns units
    }

    std::string threadName(HANDLE h)
    {
        using Fn = HRESULT(WINAPI *)(HANDLE, PWSTR *);
        static const Fn getDescription = reinterpret_cast<Fn>(GetProcAddress(GetModuleHandleA("kernel32.dll"), "GetThreadDescription"));
        if (!getDescription)
            return {};
        PWSTR text = nullptr;
        std::string result;
        if (SUCCEEDED(getDescription(h, &text)) && text)
        {
            char buffer[128] = {};
            WideCharToMultiByte(CP_UTF8, 0, text, -1, buffer, sizeof(buffer) - 1, nullptr, nullptr);
            result = buffer;
            LocalFree(text);
        }
        return result;
    }

    // NOTE: output file format tokens ('base', 'duration', 'interval_us', 'module', 'thread', 'cpu_ms',
    // 'samples', 's') are parsed by ps2-dev sampling-win and must be changed together.
    void body(std::string path, double delay, double seconds, int intervalUs)
    {
        using Clock = std::chrono::steady_clock;
        std::this_thread::sleep_for(std::chrono::duration<double>(delay));
        timeBeginPeriod(1);
        const DWORD self = GetCurrentThreadId();
        const DWORD pid = GetCurrentProcessId();
        std::map<DWORD, SampledThread> threads;
        std::map<std::pair<DWORD, uint64_t>, uint64_t> counts;

        const auto refresh = [&]
        {
            const HANDLE snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if (snapshot == INVALID_HANDLE_VALUE)
                return;
            THREADENTRY32 te{};
            te.dwSize = sizeof(te);
            for (BOOL ok = Thread32First(snapshot, &te); ok; ok = Thread32Next(snapshot, &te))
            {
                if (te.th32OwnerProcessID != pid || te.th32ThreadID == self || threads.count(te.th32ThreadID))
                    continue;
                const HANDLE h = OpenThread(THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_QUERY_INFORMATION, FALSE, te.th32ThreadID);
                if (!h)
                    continue;
                SampledThread &thread = threads[te.th32ThreadID];
                thread.handle = h;
                thread.startCpuMs = cpuMs(h);
                thread.name = threadName(h);
            }
            CloseHandle(snapshot);
        };

        refresh();
        const auto start = Clock::now();
        const auto end = start + std::chrono::duration_cast<Clock::duration>(std::chrono::duration<double>(seconds));
        auto next = start;
        auto lastRefresh = start;
        while (Clock::now() < end)
        {
            for (auto &entry : threads)
            {
                SampledThread &thread = entry.second;
                if (SuspendThread(thread.handle) == static_cast<DWORD>(-1))
                    continue;
                CONTEXT context{};
                context.ContextFlags = CONTEXT_CONTROL;
                const bool valid = GetThreadContext(thread.handle, &context) != 0;
                const uint64_t rip = valid ? context.Rip : 0;
                ResumeThread(thread.handle);
                if (valid) // outside the suspension: safe to allocate memory here
                {
                    ++counts[{entry.first, rip}];
                    ++thread.samples;
                }
            }
            if (Clock::now() - lastRefresh > std::chrono::milliseconds(500))
            {
                refresh();
                lastRefresh = Clock::now();
            }
            next += std::chrono::microseconds(intervalUs);
            std::this_thread::sleep_until(next);
        }
        timeEndPeriod(1);

        std::FILE *f = std::fopen(path.c_str(), "w");
        if (!f)
            return;
        std::fprintf(f, "base %llx\n", static_cast<unsigned long long>(reinterpret_cast<uintptr_t>(GetModuleHandleA(nullptr))));
        std::fprintf(f, "duration %.3f interval_us %d\n", seconds, intervalUs);
        HMODULE modules[1024];
        DWORD neededBytes = 0;
        if (EnumProcessModules(GetCurrentProcess(), modules, sizeof(modules), &neededBytes))
            for (DWORD i = 0; i < neededBytes / sizeof(HMODULE) && i < 1024; ++i)
            {
                MODULEINFO info{};
                char moduleName[MAX_PATH] = {};
                if (GetModuleInformation(GetCurrentProcess(), modules[i], &info, sizeof(info)) && GetModuleFileNameExA(GetCurrentProcess(), modules[i], moduleName, MAX_PATH))
                    std::fprintf(f, "module %llx %lx %s\n", static_cast<unsigned long long>(reinterpret_cast<uintptr_t>(info.lpBaseOfDll)),
                                 static_cast<unsigned long>(info.SizeOfImage), moduleName);
            }
        for (auto &entry : threads)
        {
            const uint64_t cpu = cpuMs(entry.second.handle) - entry.second.startCpuMs;
            std::fprintf(f, "thread %lu cpu_ms %llu samples %llu %s\n", static_cast<unsigned long>(entry.first), static_cast<unsigned long long>(cpu),
                         static_cast<unsigned long long>(entry.second.samples), entry.second.name.c_str());
        }
        for (const auto &c : counts)
            std::fprintf(f, "s %lu %llx %llu\n", static_cast<unsigned long>(c.first.first), static_cast<unsigned long long>(c.first.second),
                         static_cast<unsigned long long>(c.second));
        std::fclose(f);
        std::fprintf(stderr, "[sampling] %s written\n", path.c_str());
        for (auto &entry : threads)
            CloseHandle(entry.second.handle);
    }
}

#ifdef _WIN32
void gowSamplingBurst(const char *path, double seconds, int intervalUs)
{
    // GOW-Port: synchronous sampling (called by the hang watchdog of gow_integration.cpp)
    body(std::string(path), 0.0, seconds, intervalUs < 200 ? 200 : intervalUs);
}
#endif

void gowSamplingStart()
{
    const char *path = std::getenv("GOW_SAMPLING");
    if (!path || !path[0])
        return;
    static std::atomic<bool> started{false};
    if (started.exchange(true))
        return;
    const auto number = [](const char *name, double fallback)
    {
        const char *v = std::getenv(name);
        return v && v[0] ? std::atof(v) : fallback;
    };
    const double delay = number("GOW_SAMPLING_FROM", 0.0);
    const double seconds = number("GOW_SAMPLING_SEC", 20.0);
    const int us = static_cast<int>(number("GOW_SAMPLING_US", 2000.0));
    std::thread(body, std::string(path), delay, seconds, us < 200 ? 200 : us).detach();
}
#else
void gowSamplingStart() {}
void gowSamplingBurst(const char *, double, int) {}
#endif
