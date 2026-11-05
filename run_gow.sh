#!/usr/bin/env bash
# PC Port: opens the game without the launcher (Linux). The settings are those of the launcher's default window.
#   ./run_gow.sh            with the disc extracted by ./install.sh into game/
#   PS2X_GS_HW_SCALE=1 ./run_gow.sh, GOW_SKIP_FMV=1 ./run_gow.sh, ...   (see docs/INSTALL.md)
ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT/RustPortPS2/native"
# Game settings of Waffle565's runtime: VIF1/VU1/GIF on the GS thread and asynchronous FINISH.
export PS2X_GS_WORKER=${PS2X_GS_WORKER:-1}
export GOW_GS_FINISH_ASYNC=${GOW_GS_FINISH_ASYNC:-1}
export GOW_AUDIO_DEVICE=${GOW_AUDIO_DEVICE:-default}
export PS2X_GS_HW_SCALE=${PS2X_GS_HW_SCALE:-2}
export PC_DMC5_CONTROLS=${PC_DMC5_CONTROLS:-0}
export GOW_SKIP_FMV=${GOW_SKIP_FMV:-0}
# GOW-Port: the launcher's widescreen switch (PCSX2's Hor+ patch); PC_WIDESCREEN=4:3 or unset with the switch off gives
# the game's original 4:3 picture.
export PC_WIDESCREEN=${PC_WIDESCREEN:-16:9}
# Game: game/ (install.sh) or, in a development tree, the root ELF with the ISO next to it.
if [ -f "$ROOT/game/SCUS_973.99" ]; then
    ELF="$ROOT/game/SCUS_973.99"
else
    ELF="$ROOT/SCUS_973.99"
fi
if [ ! -f "$(dirname "$ELF")/.disc_layout" ] && [ -z "${GOW_ISO:-}" ]; then
    for c in "$(dirname "$ELF")"/*.iso; do [ -f "$c" ] && export GOW_ISO="$c" && break; done
fi
if [ -z "${PS2X_RUNNER:-}" ]; then
    if [ -x "$ROOT/build/ps2xRuntime/gow-2005" ]; then
        PS2X_RUNNER="$ROOT/build/ps2xRuntime/gow-2005"
    else
        PS2X_RUNNER="./build/ps2xRuntime/gow-2005"
    fi
fi
"$PS2X_RUNNER" "$ELF" "$@"
