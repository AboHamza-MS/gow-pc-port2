// RetroAchievements integration for the God of War native runtime.
// The protocol and condition evaluation come from rcheevos 12.5.0 (MIT), vendored in
// third_party/rcheevos. No password is ever written to a file: the launcher reads it
// from the Windows Credential Manager and hands it to this process only.
// The HTTPS transport (WinHTTP) lives in gow_retro_achievements_http.cpp because
// windows.h collides with raylib.h.
#include "raylib.h"
#include "rc_client.h"
#include "rc_consoles.h"
#include "rc_error.h"

#include "gow_retro_achievements_http.h"

#include <algorithm>
#include <atomic>
#include <chrono>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <iostream>
#include <mutex>
#include <string>
#include <thread>

namespace
{
    constexpr uint32_t kPs2RamSize = 32u * 1024u * 1024u;

    struct Notice
    {
        std::string title;
        std::string text;
        std::chrono::steady_clock::time_point until{};
    };

    struct State
    {
        rc_client_t *client = nullptr;
        std::atomic<uint8_t *> memory{nullptr};
        std::string iso;
        std::string elf;
        bool notifications = true;
        std::mutex noticeMutex;
        Notice notice;
    };

    // The process ends together with the game; keeping the state for the whole process
    // lifetime avoids destroying the client while an HTTP response is still
    // returning to the rcheevos callback.
    State &state()
    {
        static State *out = new State();
        return *out;
    }

    bool envFlag(const char *name)
    {
        const char *value = std::getenv(name);
        return value && value[0] == '1';
    }

    std::string envText(const char *name)
    {
        const char *value = std::getenv(name);
        return value ? std::string(value) : std::string();
    }

    void clearPasswordEnv()
    {
#ifdef _WIN32
        _putenv_s("PC_RA_PASSWORD", ""); // an empty value removes the variable from the process environment
#else
        unsetenv("PC_RA_PASSWORD");
#endif
    }

    uint32_t RC_CCONV readMemory(uint32_t address, uint8_t *destination, uint32_t bytes, rc_client_t *client)
    {
        auto *ra = static_cast<State *>(rc_client_get_userdata(client));
        uint8_t *ram = ra ? ra->memory.load(std::memory_order_acquire) : nullptr;
        // EE virtual/physical segments map to the same 32 MiB of RDRAM.
        address &= 0x1fffffffu;
        if (!ram || address >= kPs2RamSize)
            return 0;
        const uint32_t available = kPs2RamSize - address;
        const uint32_t read = std::min(bytes, available);
        std::memcpy(destination, ram + address, read);
        return read;
    }

    void showNotice(const char *title, const char *text)
    {
        State &ra = state();
        if (!ra.notifications)
            return;
        std::lock_guard<std::mutex> lock(ra.noticeMutex);
        ra.notice.title = title ? title : "RETROACHIEVEMENTS";
        ra.notice.text = text ? text : "";
        ra.notice.until = std::chrono::steady_clock::now() + std::chrono::seconds(7);
    }

    void RC_CCONV callServer(const rc_api_request_t *request, rc_client_server_callback_t callback, void *userData, rc_client_t *)
    {
        const std::string url = request && request->url ? request->url : "";
        const std::string post = request && request->post_data ? request->post_data : "";
        const std::string type = request && request->content_type ? request->content_type : "";
        std::thread([url, post, type, callback, userData]
        {
            const GowRaHttpResponse response = gowRaHttpRequest(url, post, type);
            rc_api_server_response_t server{};
            server.body = response.body.c_str();
            server.body_length = response.body.size();
            server.http_status_code = response.status;
            callback(&server, userData);
        }).detach();
    }

    void RC_CCONV logRa(const char *message, const rc_client_t *)
    {
        if (message && *message)
            std::cerr << "[retroachievements] " << message << std::endl;
    }

    void RC_CCONV gameLoaded(int result, const char *error, rc_client_t *c, void *)
    {
        if (result == RC_OK && rc_client_is_game_loaded(c))
        {
            rc_client_user_game_summary_t summary{};
            rc_client_get_user_game_summary(c, &summary);
            const std::string text = std::to_string(summary.num_unlocked_achievements) + "/" +
                                      std::to_string(summary.num_core_achievements) + " achievements loaded";
            std::cerr << "[retroachievements] " << text << std::endl;
            showNotice("RETROACHIEVEMENTS CONNECTED", text.c_str());
        }
        else
        {
            std::cerr << "[retroachievements] could not load the game: "
                      << (error ? error : rc_error_str(result)) << std::endl;
            showNotice("RETROACHIEVEMENTS", error ? error : "The game does not match a recognized set.");
        }
    }

    // RetroAchievements' PS2 hash: MD5 of the boot executable name followed by the ELF bytes. The ISO is not
    // needed when the ELF in the game folder is the original; a modified ELF produces a different hash and is not recognized.
    bool elfHash(const std::string &elf, std::string &hash)
    {
        if (elf.empty())
            return false;
        const std::filesystem::path path = std::filesystem::u8path(elf);
        std::error_code ec;
        if (path.filename() != "SCUS_973.99" || !std::filesystem::is_regular_file(path, ec))
            return false;
        return gowRaMd5WithPrefix(path.filename().string(), path.wstring(), hash);
    }

    void startGame(rc_client_t *client)
    {
        State &ra = state();
        if (!ra.iso.empty())
        {
            std::cerr << "[retroachievements] identifying the game from the ISO" << std::endl;
            rc_client_begin_identify_and_load_game(client, RC_CONSOLE_PLAYSTATION_2, ra.iso.c_str(), nullptr, 0,
                                                   &gameLoaded, nullptr);
            return;
        }
        std::string hash;
        if (!elfHash(ra.elf, hash))
        {
            std::cerr << "[retroachievements] no ISO and no readable SCUS_973.99: cannot identify the game." << std::endl;
            showNotice("RETROACHIEVEMENTS", "Could not identify the game (no ISO or executable).");
            return;
        }
        std::cerr << "[retroachievements] identifying the game from the executable (hash " << hash << ")" << std::endl;
        rc_client_begin_load_game(client, hash.c_str(), &gameLoaded, nullptr);
    }

    void RC_CCONV loginDone(int result, const char *error, rc_client_t *client, void *)
    {
        if (result != RC_OK)
        {
            std::cerr << "[retroachievements] login failed: " << (error ? error : rc_error_str(result)) << std::endl;
            showNotice("RETROACHIEVEMENTS", error ? error : "Could not start the session.");
            return;
        }
        startGame(client);
    }

    void RC_CCONV raEvent(const rc_client_event_t *event, rc_client_t *)
    {
        if (!event)
            return;
        if (event->type == RC_CLIENT_EVENT_ACHIEVEMENT_TRIGGERED && event->achievement)
        {
            const std::string text = std::string(event->achievement->title ? event->achievement->title : "Achievement") +
                                      " \u00b7 " + std::to_string(event->achievement->points) + " points";
            showNotice("ACHIEVEMENT UNLOCKED", text.c_str());
        }
        else if (event->type == RC_CLIENT_EVENT_LEADERBOARD_STARTED && event->leaderboard)
            showNotice("LEADERBOARD STARTED", event->leaderboard->title);
        else if (event->type == RC_CLIENT_EVENT_LEADERBOARD_SUBMITTED && event->leaderboard)
            showNotice("LEADERBOARD SUBMITTED", event->leaderboard->title);
        else if (event->type == RC_CLIENT_EVENT_DISCONNECTED)
            showNotice("RETROACHIEVEMENTS OFFLINE", "Pending submissions will resume on reconnect.");
        else if (event->type == RC_CLIENT_EVENT_RECONNECTED)
            showNotice("RETROACHIEVEMENTS CONNECTED", "Pending submissions synced.");
    }
}

// Called after loading the ELF, once the launcher has already handed over ISO, user and password via the environment.
void gowRetroAchievementsInstall(const std::string &elfPath)
{
    if (!envFlag("PC_RA_ENABLED"))
        return;

    State &ra = state();
    if (ra.client)
        return;

    const std::string user = envText("PC_RA_USERNAME");
    std::string password = envText("PC_RA_PASSWORD");
    ra.iso = envText("GOW_ISO");
    ra.elf = elfPath;
    ra.notifications = envFlag("PC_RA_NOTIFICATIONS");
    clearPasswordEnv();
    if (user.empty() || password.empty())
    {
        std::cerr << "[retroachievements] integration enabled, but user or secure credential is missing." << std::endl;
        showNotice("RETROACHIEVEMENTS", "Enter the user and password in the launcher.");
        std::fill(password.begin(), password.end(), '\0');
        return;
    }

    ra.client = rc_client_create(&readMemory, &callServer);
    if (!ra.client)
    {
        std::cerr << "[retroachievements] could not create the client." << std::endl;
        std::fill(password.begin(), password.end(), '\0');
        return;
    }
    rc_client_set_userdata(ra.client, &ra);
    rc_client_set_allow_background_memory_reads(ra.client, 0);
    rc_client_set_hardcore_enabled(ra.client, envFlag("PC_RA_HARDCORE") ? 1 : 0);
    rc_client_set_event_handler(ra.client, &raEvent);
    rc_client_enable_logging(ra.client, RC_CLIENT_LOG_LEVEL_INFO, &logRa);
    rc_client_begin_login_with_password(ra.client, user.c_str(), password.c_str(), &loginDone, nullptr);
    std::fill(password.begin(), password.end(), '\0');
}

// Called on VBlank: exposes the current RDRAM and evaluates the achievement set's conditions once per frame.
void gowRetroAchievementsVBlank(uint8_t *rdram)
{
    State &ra = state();
    if (!ra.client)
        return;
    ra.memory.store(rdram, std::memory_order_release);
    rc_client_do_frame(ra.client);
}

// Draws a discreet notification inside the game window.
void gowRetroAchievementsDraw()
{
    State &ra = state();
    std::lock_guard<std::mutex> lock(ra.noticeMutex);
    if (ra.notice.until <= std::chrono::steady_clock::now())
        return;
    const float width = std::min(430.0f, static_cast<float>(GetScreenWidth()) - 32.0f);
    const float x = static_cast<float>(GetScreenWidth()) - width - 18.0f;
    const float y = 22.0f;
    DrawRectangleRounded(Rectangle{x, y, width, 68.0f}, 0.12f, 8, Color{12, 13, 15, 236});
    DrawRectangleRoundedLinesEx(Rectangle{x, y, width, 68.0f}, 0.12f, 8, 1.0f, Color{201, 167, 101, 230});
    DrawRectangle(static_cast<int>(x), static_cast<int>(y), 4, 68, Color{201, 167, 101, 255});
    DrawText(ra.notice.title.c_str(), static_cast<int>(x + 16), static_cast<int>(y + 12), 15, Color{234, 213, 165, 255});
    DrawText(ra.notice.text.c_str(), static_cast<int>(x + 16), static_cast<int>(y + 37), 16, Color{240, 230, 218, 255});
}
