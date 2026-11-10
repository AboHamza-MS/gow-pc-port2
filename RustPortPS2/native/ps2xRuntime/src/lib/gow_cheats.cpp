// GOW-Port: cheat menu (trainer) for God of War (SCUS-97399, NTSC-U). F2 or
// PC_CHEATS_SHORTCUT="7"/"9+11" opens it during gameplay;
// PC_CHEATS="god_mode,one_hit_kill,..." gives the initial state and PC_LANGUAGE (en/pt-BR) the language.
//
// Addresses taken from the executable (not from third-party codes):
//   0x302CC0  Kratos's state (what goes into the save game):
//     +0x5C float magic, +0x60 float Rage of the Gods meter (0..100, AddRage caps it at 100),
//     +0x68 u16 red orbs (SetOrbs, 0x1DB250, caps them at 60000),
//     +0x76 u8 health level (0..4), +0x77 u8 magic level (0..5) (0x1D8200: Gorgon Eyes / Phoenix Feathers),
//     +0x78..+0x7C u8 levels of Zeus's Fury, Medusa's Gaze, Poseidon's Rage, Army of Hades and Artemis's
//     Blade (0..2), +0x7D u8 Blades of Chaos level (0..4); 0x1D8CB8 treats those figures as complete,
//     +0x82 u16 items (0x04 Zeus, 0x08 Medusa, 0x10 Poseidon, 0x20 Hades, 0x40 Artemis, 0x100 trident).
//   0x3028E0  player settings: +0x00..+0x10 max health per level, +0x14..+0x28 max magic per level.
//   0x29CD58  player controller; its first field is Kratos (health at +0x178, max at +0x17C).
//   0x29CB94  UI variables (Costumes_Unlocked +0x1AC, HardMode_Unlocked +0x1B0).
//   0x2A12C8  completed playthroughs (bits 0-2 easy/normal/hard, 3 God Mode, 4 Challenge of the Gods): this
//     drives the extras, costumes and God Mode difficulty (0x1BA9B8, 0x22F4B0). Saved to the memory card.
//   0x1E0A80  sets a creature's health (a0, f12; clamps it between 0 and max). 0x1E0A98 subtracts damage (a0, f12)
//     and signals the thresholds it crosses (boss phases); ends up calling 0x1E0A80.
#include "ps2_runtime.h"
#include "ps2_runtime_macros.h"
#include "raylib.h"

#include <algorithm>
#include <array>
#include <atomic>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <iostream>
#include <string>
#include <vector>

namespace
{
    enum Cheat : int
    {
        Immortal,
        OneHitKills,
        InfiniteMagic,
        InfiniteRage,
        MaxOrbs,
        AllUpgrades,
        AllPowers,
        MaxMeters,
        Extras,
        InfiniteJump,
        kNumCheats
    };
    // NOTE: these id strings are a public contract (PC_CHEATS env var value, gow-launcher/src/pages/CheatsPage.tsx's
    // `CHEATS` list, documented in docs/dev/GAMEPLAY-HOOKS.md) and must not change.
    constexpr const char *kIds[kNumCheats] = {"god_mode", "one_hit_kill", "infinite_magic", "infinite_rage", "max_orbs",
                                              "all_upgrades", "all_powers", "max_meters", "unlock_extras", "infinite_jump"};

    constexpr uint32_t kMask = 0x01FFFFFFu;
    constexpr uint32_t kState = 0x00302CC0u;
    constexpr uint32_t kSettings = 0x003028E0u;
    constexpr uint32_t kPlayerControl = 0x0029CD58u;
    constexpr uint32_t kUiVariables = 0x0029CB94u;
    constexpr uint32_t kCompletedPlaythroughs = 0x002A12C8u;
    constexpr uint32_t kSetHealth = 0x001E0A80u;
    constexpr uint32_t kSubtractHealth = 0x001E0A98u;

    std::array<std::atomic<bool>, kNumCheats> g_active{};
    std::atomic<bool> g_installed{false};
    std::atomic<bool> g_menuOpen{false};
    std::atomic<bool> g_blockAfterClose{false};
    struct CheatTriggerPart
    {
        int gamepad = 0;
    };
    struct CheatTrigger
    {
        CheatTriggerPart first;
        CheatTriggerPart second;
        bool latched = false;
        bool wasDown = false;
    };
    CheatTrigger g_trigger;

    PS2Runtime::RecompiledFunction g_setHealth = nullptr;
    PS2Runtime::RecompiledFunction g_subtractHealth = nullptr;

    bool triggerPartDown(const CheatTriggerPart &part)
    {
        return part.gamepad > 0 && IsGamepadAvailable(0) && IsGamepadButtonDown(0, static_cast<GamepadButton>(part.gamepad));
    }

    bool cheatTriggerDown()
    {
        if (!g_trigger.first.gamepad)
            return false;
        if (!triggerPartDown(g_trigger.first))
            return false;
        return !g_trigger.second.gamepad || triggerPartDown(g_trigger.second);
    }

    bool cheatTriggerPressed()
    {
        const bool down = cheatTriggerDown();
        const bool pressed = down && !g_trigger.wasDown;
        g_trigger.wasDown = down;
        return pressed && !g_trigger.latched;
    }

    CheatTriggerPart parseTriggerPart(const std::string &part)
    {
        CheatTriggerPart result;
        try
        {
            result.gamepad = std::clamp(std::stoi(part), 1, static_cast<int>(GAMEPAD_BUTTON_RIGHT_THUMB));
        }
        catch (...)
        {
            result = {};
        }
        return result;
    }

    void configureCheatTrigger()
    {
        g_trigger = {};
        const char *value = std::getenv("PC_CHEATS_SHORTCUT");
        if (!value || !*value)
            return;
        std::string text(value);
        const size_t plus = text.find('+');
        try
        {
            g_trigger.first = parseTriggerPart(text.substr(0, plus));
            if (plus != std::string::npos)
                g_trigger.second = parseTriggerPart(text.substr(plus + 1));
            if (g_trigger.second.gamepad == g_trigger.first.gamepad)
                g_trigger.second = {};
        }
        catch (...)
        {
            g_trigger = {};
        }
        if (g_trigger.first.gamepad)
            std::cerr << "[cheats] gamepad shortcut configured: "
                      << std::to_string(g_trigger.first.gamepad)
                      << (g_trigger.second.gamepad ? "+" : "")
                      << (g_trigger.second.gamepad ? std::to_string(g_trigger.second.gamepad) : "")
                      << std::endl;
    }

    bool active(Cheat cheat) { return g_active[cheat].load(std::memory_order_relaxed); }

    bool diagActive()
    {
        static const bool diag = []
        {
            const char *value = std::getenv("PC_CHEATS_DIAG");
            return value && std::strcmp(value, "1") == 0;
        }();
        return diag;
    }

    // PC_CHEATS_DIAG=1: the first calls to the health functions (creature, before, requested, after).
    void diagHealth(const char *function, uint32_t creature, uint32_t player, float before, float requested, float applied)
    {
        static std::atomic<unsigned> messages{0};
        if (!diagActive() || messages.fetch_add(1, std::memory_order_relaxed) >= 400u)
            return;
        std::fprintf(stderr, "[cheats:health] %s creature=%06x%s health=%.1f requested=%.1f -> %.1f\n", function, creature,
                     creature == player ? " (Kratos)" : "", before, requested, applied);
    }

    bool validAddress(uint32_t address, uint32_t bytes = 4u)
    {
        return address >= 0x00100000u && address + bytes <= PS2_RAM_SIZE && (address & 3u) == 0u;
    }

    template <typename T>
    T read(const uint8_t *rdram, uint32_t address)
    {
        T value{};
        std::memcpy(&value, rdram + address, sizeof(T));
        return value;
    }

    template <typename T>
    void write(uint8_t *rdram, uint32_t address, T value)
    {
        std::memcpy(rdram + address, &value, sizeof(T));
    }

    float maxHealthOfLevel(const uint8_t *rdram, unsigned level)
    {
        return read<float>(rdram, kSettings + 4u * std::min(level, 4u));
    }

    // Kratos: first field of the player controller. 0 if the pointers are not in the game's RAM.
    uint32_t kratos(const uint8_t *rdram)
    {
        const uint32_t control = read<uint32_t>(rdram, kPlayerControl) & kMask;
        if (!validAddress(control))
            return 0u;
        const uint32_t creature = read<uint32_t>(rdram, control) & kMask;
        return validAddress(creature, 0x180u) ? creature : 0u;
    }

    // Before writing to Kratos (outside his own code), his max health must be one from the level table:
    // this avoids touching an object already freed during a load.
    bool kratosPlausible(const uint8_t *rdram, uint32_t creature)
    {
        const float max = read<float>(rdram, creature + 0x17Cu);
        for (unsigned level = 0; level <= 4u; ++level)
            if (max > 0.0f && max == maxHealthOfLevel(rdram, level))
                return true;
        return false;
    }

    // This function executes inside the game loop to periodically update cheat modifiers
    void handleGameplayCheats(uint8_t *rdram)
    {
        if (active(InfiniteJump))
        {
            uint32_t player = kratos(rdram);
            if (player != 0u && kratosPlausible(rdram, player))
            {
                // Resets double jump counter so player can jump infinitely in mid-air
                write<uint32_t>(rdram, player + 0x14Cu, 0u);
            }
        }
    }
}
