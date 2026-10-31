# Environment variables

The runtime is configured through environment variables. **Everything here is off by default**: an absent variable
means the shipped behavior. The launcher sets the ones a player needs; the rest exist for development and testing.

Conventions:

- A variable ending in `_DIAG` writes a `[...]`-tagged line to the log (stdout/stderr, i.e. `logs/last.log`).
- A variable ending in `_CAPTURE`, `_DUMP`, `_RECORD` writes a file. **Those files contain game data** (RAM, VRAM,
  microcode) and must never be committed, attached or uploaded.
- Times are in seconds unless the name says otherwise.

## Set by the launcher

The launcher keeps your environment and adds or removes these. Set them yourself to reproduce a player's session
manually:

```
PS2X_GS_WORKER=1 GOW_GS_FINISH_ASYNC=1 GOW_AUDIO_DEVICE=default
GOW_PERF_DIAG=1 GOW_PERF_FRAME_PC=0x001837B8
```

plus, from the settings: `PC_SCREEN`, `PC_WIDTH`, `PC_HEIGHT`, `PC_TITLE`, `PC_PANEL=0`, `PC_ICON`, `PC_ASPECT`,
`PC_WIDESCREEN`, `PC_FILTER`, `GOW_SHARPEN`, `PC_FXAA`, `GOW_PROGRESSIVE`, `GOW_SCALER`,
`PS2X_GS_HW_SCALE`, `PC_GAME_FPS=60`, `PC_WINDOW_FPS`, `PC_VSYNC`, `PC_SHOW_FPS`, `GOW_SKIP_FMV`,
`GOW_FAST_BOOT`, `PC_DMC5_CONTROLS`, `PC_PAD_FACE_MAP`, `PC_KEYS`, `PC_CHEATS`, `PC_CHEATS_SHORTCUT`,
`PC_LANGUAGE`, `PC_ESC_QUITS`, `GOW_CANCEL`, `GOW_ISO`, `GOW_VU1_CAPTURE`, `PS2X_GS_SHADER_CACHE_DIR`, and the
`PC_RA_*` group when RetroAchievements is enabled.

Note: the launcher also exports `PC_SHARPNESS` and `PS2X_GS_SCALE`, but the runtime does not read them. Sharpening
arrives as `GOW_SHARPEN` and the render scale as `PS2X_GS_HW_SCALE`.

## Presentation

| Variable | Effect |
|---|---|
| `PC_SCREEN=fullscreen\|window`, `PC_WIDTH`, `PC_HEIGHT` | Initial mode and window size |
| `PC_ASPECT=auto\|stretch\|4:3\|16:9\|16:10\|21:9` | Shape of the presented picture; `stretch` fills the window |
| `PC_WIDESCREEN=<aspect>` | Hor+ widescreen patch at that aspect (absent = the game's 4:3) |
| `PC_FILTER=fsr\|bilinear\|pixel`, `PC_FXAA=1` | Filter *intent*; the runtime itself reads `GOW_SCALER` and `GOW_SHARPEN` |
| `GOW_SCALER=sharp\|bilinear`, `GOW_SHARPEN` (0–1) | The scaler that actually runs: bicubic + contrast-adaptive sharpening, or a linear tap |
| `PS2X_GS_HW_SCALE=<n>` | Internal render scale (float, clamped to ≥ 1). This is the one the runtime reads |
| `PS2X_GS_DIRECT_PRESENT=0` | Forces the RAM (readback + upload) presentation; shared GL textures are the default |
| `GOW_PRESENT_BLOCKING`, `GOW_PRESENT_FIFO`, `GOW_PRESENT_MIN_INTERVAL_US` | Presentation pacing |
| `PC_GAME_FPS`, `PC_WINDOW_FPS`, `PC_VSYNC`, `PC_SHOW_FPS` | Limits, vsync, counter |
| `PC_PROGRESSIVE` / `GOW_PROGRESSIVE` | 480p progressive mode |
| `PC_TITLE`, `PC_ICON`, `PC_PANEL=0` | Window title, icon, hidden developer panel (F1 opens it) |
| `PC_ESC_QUITS=1\|0` | Escape closes the game |

## Threads and pacing

| Variable | Effect |
|---|---|
| `PS2X_GS_WORKER=1` | VIF1/VU1/GIF on the GS worker thread. **Required for real-time audio and speed** |
| `GOW_GS_FINISH_ASYNC=1` | GS FINISH does not block the EE |
| `GOW_EE_WAIT=1` | The EE sleeps while waiting for the frame (changes game pacing) |
| `PS2X_IOP_NATIVE`, `PS2X_IOP_NATIVE_STATS` | Native IOP module use and its hit statistics |
| `PS2X_THREAD_PRIORITY` | Thread priority policy |

## Input and features

| Variable | Effect |
|---|---|
| `PC_KEYS="action=code,code;..."` | Keyboard/mouse map (`1000+n` = mouse buttons) |
| `PC_DMC5_CONTROLS`, `PC_PAD_FACE_MAP` | Pad preset and the A/B/X/Y swap |
| `PC_CHEATS=god_mode,...`, `PC_CHEATS_SHORTCUT`, `PC_CHEATS_DIAG`, `PC_CHEATS_OPEN_IN` | Cheats at startup, menu shortcut, diagnostics |
| `PC_LANGUAGE=en\|pt-BR`, `PC_MENU_FONT=<ttf>` | Cheat-menu language and font |
| `GOW_TARGETING=0`, `GOW_TARGETING_DIAG`, `GOW_TARGETING_TEST`, `GOW_TARGETING_STICK`, `GOW_TARGETING_DUMP` | Target lock |
| `GOW_CANCEL=0`, `GOW_CANCEL_EVASION`, `GOW_CANCEL_COMBO`, `GOW_CANCEL_BUFFER`, `GOW_CANCEL_DIAG`, `GOW_CANCEL_SEGMENT`, `GOW_CANCEL_AXIS`, `GOW_CANCEL_DUMP`, `GOW_CANCEL_WATCH` | Agile combat |
| `PC_RA_ENABLED`, `PC_RA_USERNAME`, `PC_RA_PASSWORD`, `PC_RA_HARDCORE`, `PC_RA_NOTIFICATIONS` | RetroAchievements |

## Content and paths

| Variable | Effect |
|---|---|
| `GOW_ISO=<file>` | Disc image to read from (otherwise the extracted files are used) |
| `GOW_MC0`, `GOW_MC1` | Memory-card images; `GOW_MC0=0` empties slot 1 |
| `GOW_SKIP_FMV=1` | Skips the game's movies (for load-time investigation) |
| `GOW_FAST_BOOT=1` | Skips the entry animation |
| `GOW_TEXTURE_REPLACE`, `GOW_TEXTURE_REPLACE_SYNC`, `GOW_TEXTURE_REPLACE_{CPU,GPU}_MB`, `GOW_TEXTURE_REPLACE_UPLOAD_MB`, `GOW_TEXTURE_REPLACE_DIAG` | Custom texture packs |
| `GOW_PAD_TEST`, `GOW_PAD_TEST_NO_CAPTURE`, `GOW_PAD_SCRIPT`, `GOW_PAD_CAPTURES`, `GOW_PAD_LOG` | Automated pad input and GS frame captures |
| `GOW_HOST_SHOT`, `GOW_HOST_SHOT_ON_CHANGE` | Window screenshots at a frame number |

## Instrumentation

| Variable | What it prints |
|---|---|
| `GOW_PERF_DIAG=1`, `GOW_PERF_FRAME_PC=<pc>` | `[gow-perf]` per-second split: EE, VU1, GS, upload, flip rate |
| `GOW_DELTA_DIAG=1` | `[gow-speed]` game time vs wall time (see [`../PERFORMANCE.md`](../PERFORMANCE.md)) |
| `GOW_SAMPLING=1` | Periodic subsystem sampling (`ps2-dev perf-summary` parses it) |
| `GOW_VU1_STATS=1`, `GOW_VU1N_DIAG=1`, `GOW_VU1_BUDGET_DIAG=1` | VU1 native/interpreted coverage, native block hits |
| `GOW_GS_HW_STATS=1`, `GOW_GS_HW_PROFILE=1`, `GOW_GS_QUEUE_DIAG`, `GOW_GS_PRESENT_PHASE_DIAG` | GS backend counters and queue timing |
| `GOW_IO_HIST=<s>`, `GOW_IOP_IO_HIST=<s>` | Which hardware registers the game still touches, within `<s>` seconds |
| `GOW_VIF_DIAG=1`, `GOW_XGKICK_IMMEDIATE=1`, `GOW_VU1F_*` | VIF/GIF packet diagnostics |
| `GOW_AUDIO_STATS=1`, `GOW_IOP_DRIFT=1`, `GOW_SND_REGLOG=1`, `GOW_SND_DIAG=1` | Sound: frame counts, IOP drift, register writes |
| `GOW_MPEG_DIAG=1`, `GOW_MPEG_PACE`, `GOW_MOVIE_OVERRIDE` | Movie playback and A/V sync |
| `GOW_RENDER_DIAG=1`, `GOW_PATH_DIAG=1`, `GOW_ANM_DIAG=1`, `GOW_SKEL_DIAG=1`, `GOW_MODEL_DIAG=1` | Renderer, node search, animation, skeleton |
| `GOW_GS_REPLAY_TRACE`, `GOW_GS_REPLAY_AFTER`, `GOW_GS_REPLAY_SECONDS`, `GOW_GS_REPLAY_TEXFLUSH` | Record a GS command stretch. The recording works, but the consumer (`replay_gs`) does not build in this tree; use `GOW_VIF_CAPTURE` plus `replay_vif_chain` for VIF1/VU1 work |
| `PS2X_EE_DIFF*`, `PS2X_EE_WATCH*` | Differential EE runs against PCSX2's interpreter |
| `GOW_VU1_PCSX2=1` | Run VU1 on PCSX2's interpreter (A/B reference) |

There are many more single-purpose switches (GS dumps, VU1 recording, EE traces). The complete, authoritative list is
one command away:

```bash
grep -rhoE 'getenv\("([A-Z0-9_]+)"\)' RustPortPS2/native --include=*.cpp --include=*.h --include=*.inl \
  | sed -E 's/.*"([A-Z0-9_]+)".*/\1/' | sort -u
```

Read the code that uses a variable before relying on it: most dump files are game data and must stay local.
