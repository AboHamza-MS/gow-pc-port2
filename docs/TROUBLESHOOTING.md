# Troubleshooting

Most problems come down to one of three things: the wrong disc, a missing build tool, or a laptop that isn't giving
the VU1 thread enough CPU. This page is organized by symptom. If you end up opening an issue, the last section says
what to attach.

## Where the logs are

| Log | Where |
|---|---|
| Last game run (stdout + stderr) | Launcher: **Other → Last game log**. File: `%APPDATA%\GOW_PC\Launcher\logs\last.log` (`~/.local/share/GOW_PC/Launcher/logs/last.log`), previous run in `previous.log` |
| Installation | `logs/install.log` in the launcher's data folder |
| Optimization | `logs/optimize.log` in the launcher's data folder |
| Recompilation | `build-gen/recomp.log` |
| Optimization when run by an installer | `build-gen/optimize.log` |

The game log is plain text. Lines starting with `[gow-`, `[gs`, `[vu1` and `[iop` are the useful ones.

## Installation

| Symptom | Cause / fix |
|---|---|
| "God of War ISO not found" | Put the ISO in `game/` or pass the path (`install.cmd -Iso ...`, `./install.sh "/path/file.iso"`). Only the **USA** disc (`SCUS-97399`) works |
| The wizard rejects the ISO | It must be the **original** USA image, not a translation, patch or other region. The hash is checked on purpose |
| "Visual Studio is missing" | Tick the Build Tools component in the wizard, or run `install.cmd -InstallTools`. It needs administrator permission and 15–30 minutes |
| `cmake ended with code 1` and `The source directory ".../build" does not exist` | A CMake older than 3.21 was picked up, usually the one bundled with Visual Studio 2017 or early 2019. Tick the Build Tools component in the wizard (it installs 2022 next to the old one), or uninstall the old Visual Studio. Run the installer again and read the `Visual Studio:` line and the `cl`, `cmake` and `ninja` lines under it, near the top of the log: they show which tools were used |
| The build fails near the end with `RC1107` on `gow.rc.res` | The CMake in `PATH` comes from another toolchain and lacks `cmcldeps.exe`. The installer now prefers Visual Studio's CMake, which has it; if you still see this, make sure Visual Studio has the "C++ CMake tools for Windows" component, or put an official CMake 3.21+ first in `PATH` |
| ``cmake: .../usr/lib/libcrypto.so.3: version `OPENSSL_3.x.0' not found (required by /usr/lib/libssl.so.3)`` (Linux, AppImage) | The AppImage's bundled libraries leaked into the tools the wizard runs, so the system `cmake` mixed the AppImage's older `libcrypto` with your newer `libssl`. Fixed: the launcher now removes its own library path from everything it starts (the installer, `cmake`, the game). Update the launcher; as a workaround on an older one, start it with `env -u LD_LIBRARY_PATH ./<file>.AppImage` or run `./install.sh` from a checkout |
| "The path cannot contain accents" | Choose an ASCII path (the launcher enforces it; `install.ps1` alone falls back to `%SystemDrive%\PCGen` for the generated code) |
| Not enough disk space | About 30 GB: ~8 GB extracted disc + build. The wizard checks before starting |
| The build takes hours | Normal on the first run (6,400 C++ files). Reduce `-Jobs`/`--jobs` if the machine starts swapping; the linker step alone can take a long time with the MSVC toolchain |
| "CMake does not match the source" | A build folder was configured from another checkout; the installers detect it and reconfigure. If you build by hand, delete `build/CMakeCache.txt` |
| The VU1 step warned "could not apply the manifest" | Your ELF is not the retail one. The installer falls back to capturing the microcode by playing, which is slower to converge |
| Installation cancelled | Everything finished is kept; run it again and it resumes |

## Starting the game

| Symptom | Cause / fix |
|---|---|
| Launcher says the game files are missing | **Files → Find files automatically**, or set `SCUS_973.99` and `gow-2005` by hand |
| "ELF does not match this build" | The runner was built for another ELF (or the file changed). Rebuild, or set the runner the build produced |
| Window opens and closes instantly | Read `logs/last.log`; usually a missing renderer feature (below) or a missing game data file |
| Black screen, crash or "could not create the GL context" on an old GPU | The renderer needs **OpenGL 4.6**. Update the driver; on Linux make sure the proprietary/Mesa driver for your card is actually in use |
| Windows SmartScreen warning | The launcher is not code-signed yet: "More info → Run anyway". Verify you downloaded it from the project's Releases page |
| Third-party antivirus blocks the build | The build runs `cl.exe`, `ninja` and the recompiler; add the installation folder to its exclusions |

## Performance and audio

| Symptom | Cause / fix |
|---|---|
| Runs in slow motion | Check `speed=` in the log (`GOW_DELTA_DIAG=1`). Usually the CPU: close background builds, use the performance power profile, and run the optimization pass |
| Slow in combat only | VU1-bound. Run `-Optimize`/`--optimize` again after playing new areas, or let the launcher's background optimization finish |
| Slow at 3× and above | GPU-bound, as expected. Go back to 2×. The launcher's *fluency fallback* is what keeps heavy scenes at 60 |
| Audio crackles or cuts | Usually the GS running inline with the EE: the launcher sets `PS2X_GS_WORKER=1`; if you start the game by hand, export it. Also check that the CPU is not throttling |
| No audio device | `GOW_AUDIO_DEVICE=default` picks the system default; set another device name if you have several |
| Window stutters while the FPS number is fine | Presentation-bound: lower the FPS mode, or disable VSync if your display does not need it |

## Saves and the memory card

| Symptom | Cause / fix |
|---|---|
| "No MEMORY CARD found" | The card image is missing: `Mcd001.ps2` next to `SCUS_973.99` (in `game/`). Copy a PCSX2 card there, or let the game create one |
| My PCSX2 saves are not listed | The file must be a PCSX2 card image named `Mcd001.ps2` next to the executable, or point `GOW_MC0` at it |
| The save does not appear in PCSX2 | Copy the whole `Mcd001.ps2` out of `game/`; do not copy parts of it |
| **The game hangs right after saving** | A known open problem on Windows. Note the area and what you did, then attach `logs/last.log`; its last lines show where it stopped |
| Save does not survive a reinstall | `game/` is deleted on a clean reinstall. Back up `Mcd001.ps2` |

## Controls and features

| Symptom | Cause / fix |
|---|---|
| A key does nothing | It may be bound to two actions, or be one of the reserved keys (F1/F2/F11). **Controls** marks conflicts |
| Ctrl casts magic instead of dodging | Ctrl is R2 while standing still; hold a movement key to dodge |
| F2 does not open the cheat menu | The game window must have focus. With a pad, only the captured button/combo opens it |
| No rumble | Vibration is implemented on **Windows only** |
| Gamepad not detected | Only controller port 1 is connected. Reconnect the pad before starting the game |
| Target lock does nothing | Alt must be pressed and released alone, with the window focused, and there must be an enemy within range |
| Custom textures do nothing | **Textures → Custom textures** must be on, the files must be in `textures/replacements`, and the game must be restarted |
| Achievements not recognized | The ISO must be the original (or an unmodified `SCUS_973.99`). Hardcore mode turns cheats off by design |
| Achievements password not saved on Linux | Not implemented there; the credential store is Windows-only |

## Reset and start over

- **Launcher settings**: **Other → Restore defaults** (graphics, performance, controls and cheats; paths are kept).
- **Settings file**: delete `settings.json` in the launcher data folder.
- **Game files and build**: delete `game/`, `build/`, `build-gen/` and `vu1_captures/` (back up `Mcd001.ps2` first) and
  run the installer again.
- **Recompile only**: run the installer again without deleting anything; finished steps are skipped.
- **Play without the launcher** (Linux): `./run_gow.sh`, which uses the same environment as the launcher's defaults.

## What to include in a bug report

1. **Platform and version**: OS, GPU and driver, launcher version (About) or commit hash.
2. **How it was built**: the launcher wizard, `install.cmd`/`install.sh`, and whether `-FastDev` was used.
3. **What you did**: the area, whether it was a save, a movie, a menu; if it is performance, the scene and the settings.
4. **The log**: `logs/last.log` (or `build-gen/recomp.log` for build problems).
5. **The game-speed line**: `speed=` from the log, if the complaint is performance.

Do **not** attach game files, RAM/VRAM dumps or captured VU1 microcode: they are game data and must stay on your
machine.
