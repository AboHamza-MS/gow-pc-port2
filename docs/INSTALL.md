# Installing

There is no prebuilt binary of the game. Its code is **recompiled on your computer** from your own disc: this is a
static recompilation, not an emulator, so the disc is compiled into a native program instead of running in a virtual
PS2. On Windows a launcher does all of it for you, and two installer scripts cover people who prefer a terminal.

> [!IMPORTANT]
> This repository contains **no game files** (ISO, executable, IRX modules, `.PAK` data) and no code generated from
> them. You need your own legal copy of *God of War* for the PlayStation 2: the **USA** version, `SCUS-97399`.

> [!WARNING]
> **Linux support is experimental.** The Linux code path and installer exist in the repository and the game builds and
> runs there, but it is not validated to the same level as Windows: no CI job builds or tests the runtime on any
> platform, recent Linux builds have not been measured, vibration is not implemented, and the launcher's setup wizard,
> which drives `install.sh` on Linux, has never run on a real Linux machine. Treat Linux as prepared, not supported; see
> [`STATUS.md`](STATUS.md).

## What you need

| | |
|---|---|
| **Game** | Your own *God of War* (USA, `SCUS-97399`) disc image (`.iso`, about 8 GB). Rip it from your own disc with any DVD tool. Please don't pirate it |
| **Disk** | About **30 GB** free (the extracted disc is about 8 GB, the build a few more) |
| **CPU** | 64-bit x86, 4 cores or more recommended. AVX2 is **not** required by the shipped build |
| **RAM** | 8 GB (16 GB recommended while building) |
| **GPU** | OpenGL 4.6 or newer: any recent NVIDIA/AMD card, or the integrated graphics of a recent Intel/AMD CPU |
| **OS** | Windows 10/11 x64, or Linux: Arch/CachyOS, Ubuntu 24.04+, Fedora 40+ or similar (**experimental**, see above) |
| **Time** | The first build takes **30–90 minutes** depending on the CPU. Later updates only rebuild what changed |

The installation folder must not contain accents or special characters (the launcher enforces this; the Windows
installer works around it by moving the generated code to `%SystemDrive%\PCGen`).

## Windows: the launcher (recommended)

1. Download `God Of War - PC Launcher_<version>_x64-setup.exe` from the Releases page and run it. It installs for your
   user only (no administrator permission needed) and bundles WebView2, so it works on a clean Windows 10/11.
   The installer is not code-signed yet, so Windows SmartScreen shows "Windows protected your PC" and an "unknown
   publisher": click **More info**, then **Run anyway**. Code signing needs a paid certificate (or Azure Trusted
   Signing), which the project doesn't have yet.
2. Open the launcher. The **setup wizard** starts by itself:
   1. **ISO**: it looks for your ISO and verifies it. If it can't find the file, drop it on the window or use
      **Browse**.
   2. **Location**: where the game goes (`C:\Users\<you>\GodOfWar-PC` by default), ASCII path, 30 GB free.
   3. **Components**: only if something is missing. Visual Studio 2022 Build Tools (C++, about 6 GB, asks for
      administrator permission, 15–30 minutes) and portable Git (about 50 MB, stays inside the install folder). Build
      Tools are also offered when you have an older Visual Studio (2017, 2019) and no CMake 3.21 or newer on the PC: the
      new ones are installed next to the old ones, which are not removed. If a usable CMake exists, nothing is offered.
   4. **Install**: a progress bar and a cancel button. Finished steps are kept, so you can cancel and resume later.
3. Press **PLAY**. Check **Files** once: the executable, the runner and the memory card should all be detected.

The launcher's own documentation (tabs, options, paths, logs) is in [`LAUNCHER.md`](LAUNCHER.md).

**Updating the launcher.** When a newer launcher is published, a notice with an **Update** button appears in the title bar;
the button downloads and installs it and restarts the launcher (it waits while the game, an installation or an
optimization is running). **Other, Automatic updates** does the same by itself at start-up; it is off by default. Your
settings and the game are kept. Details: [`LAUNCHER.md`](LAUNCHER.md#updating-the-launcher).

## Windows: the portable zip

For anyone who prefers not to run an installer, for example because SmartScreen warns about an unsigned one. It is the
same launcher and the same wizard; only the installation of the launcher itself is skipped.

1. Download `God Of War - PC Launcher_<version>_x64-portable.zip` from the Releases page. **Unblock it before
   unzipping**: right-click the zip, Properties, tick **Unblock**, OK. A zip downloaded by a browser carries the "from the
   internet" mark and Explorer hands it to every file inside, so without this step the unsigned `.exe` triggers the same
   SmartScreen warning as the installer (More info, Run anyway also works).
2. Unzip the whole folder to a path without accents or special characters (for example `C:\GodOfWarPC`) and open
   `God Of War - PC Launcher.exe`. Keep the `kit` folder next to it: the launcher copies the installation scripts from
   there on its first run. The wizard then works as described above.
3. What is different from the installer:
   - **No Start-menu entry and no uninstaller.** To remove it, delete the folder, `%APPDATA%\GOW_PC\Launcher` and the
     game folder you chose in the wizard.
   - **WebView2 is not included.** Windows 11 and up-to-date Windows 10 already have the Microsoft Edge WebView2
     Runtime; if the launcher does not open, install its Evergreen Standalone Installer from Microsoft.
   - **Settings stay in `%APPDATA%\GOW_PC\Launcher`**, not in the folder, so a new version can be unzipped over the old one
     (or next to it): settings carry over and the kit refreshes itself.
   - **It does not update itself.** It still shows the "new version available" notice, but the button opens the release
     page: download the new zip there and unzip it over the old folder.

Every release has `SHA256SUMS-windows.txt` and `SHA256SUMS-linux.txt` with the SHA-256 of the installers and the zip, built
by the same GitHub Actions run. Compare with `Get-FileHash <file>` (PowerShell) or `sha256sum -c` (Linux). The zip also
contains a `README.txt` with these steps, in English and Portuguese.

## Windows: the installer script

From a checkout of the repository, PowerShell 5.1 or 7:

```powershell
install.cmd -Iso "D:\God of War.iso"                 # extracts, recompiles and builds
install.cmd -Iso "D:\God of War.iso" -InstallTools   # first time: installs the C++ build tools too
install.cmd -Destination "D:\GodOfWar"               # keep the game data and the build out of the repository
```

| Option | Meaning |
|---|---|
| `-Iso <file>` | The disc image. Without it, the first `.iso` in `game\` and then in the repository root is used |
| `-Destination <folder>` | Where `game\`, `build\`, `build-gen\` and `vu1_captures\` go. The generated C++ still lands in the repository |
| `-InstallTools` | Downloads Microsoft's Build Tools installer (signature checked) and installs the C++ workload, when Visual Studio is missing or no usable CMake exists (see below) |
| `-NoOptimize` / `-Optimize` | Skip / force the optimization pass (see below) |
| `-Seconds <n>` | Duration of the optimization run (default 240 s) |
| `-Jobs <n>` | Parallel compiler processes (default: cores, at most one per 2 GB of RAM) |
| `-Vu1MemoryLimitGB <4-8>` | RAM cap for the VU1 generator (default 4 GB, `0` = unlimited) |
| `-Vu1BuildDir <folder>` | Use an existing VU1 microcode folder instead of `vu1_captures\` |
| `-FastDev` | `RelWithDebInfo`, logs on, console visible. For iterating; **not** for measuring performance |
| `-AllowModifiedElf` | Accept a modified `SCUS_973.99` (experimental builds) |
| `-RecompExe` / `-ToolsExe` | Use prebuilt `ps2_recomp` / `ps2-tools` instead of building them with cargo |

`install.cmd` is a thin wrapper around `install.ps1`; call the `.ps1` directly if your execution policy allows it.

## Linux (experimental)

1. Install the build dependencies. The easy way is to let the script do it: `./install.sh --install-deps` detects the
   distribution (Arch and relatives, Debian/Ubuntu and relatives, Fedora and relatives), lists what is missing and
   installs it through `sudo`. `./install.sh --check-deps` only reports. By hand:

   ```bash
   # Arch / CachyOS / Manjaro
   sudo pacman -S --needed base-devel cmake ninja git

   # Ubuntu / Debian
   sudo apt install build-essential cmake ninja-build git libgl-dev libx11-dev libxrandr-dev \
     libxinerama-dev libxcursor-dev libxi-dev libxkbcommon-dev libwayland-dev libasound2-dev

   # Fedora
   sudo dnf install gcc-c++ cmake ninja-build git mesa-libGL-devel libX11-devel libXrandr-devel \
     libXinerama-devel libXcursor-devel libXi-devel libxkbcommon-devel wayland-devel alsa-lib-devel
   ```

   The script looks the programs up in `PATH` and the libraries with `pkg-config` (GL, X11, Xrandr, Xinerama, Xcursor,
   Xi, xkbcommon, Wayland, ALSA). Rust is only needed if you do not have prebuilt tools in `bin/` (see below). Python
   is **not** needed.

   Before building, it also checks the free space of the destination (about 30 GB; only a warning once the disc is
   already extracted) and the RAM (4 GB at least, a warning under 8 GB). `GOW_SKIP_RESOURCE_CHECK=1` skips both.

2. Put your ISO in `game/God of War.iso` or pass its path.
3. Run:

   ```bash
   ./install.sh                          # or: ./install.sh "/path/to/God of War.iso"
   ```

| Option | Meaning |
|---|---|
| `<iso>` (positional) | The disc image; otherwise the first `.iso` in `game/` then in the repository root |
| `--destination DIR` | Where `game/`, `build/`, `build-gen/` and `vu1_captures/` go (default: next to the script) |
| `--install-deps` | Install the missing system packages through `sudo` (otherwise it asks in a terminal, or prints the command) |
| `--check-deps` | Only report what is missing (`[missing]`, `[packages]` and `[install-command]` lines, exit code 2 if anything is) |
| `--no-optimize` | Skip the optimization pass |
| `--optimize [seconds]` | Force it, optionally for a different duration (default 240 s) |
| `--jobs N` | Parallel compiler processes |
| `--recomp-exe FILE` | Use a prebuilt `ps2_recomp` |
| `--tools-exe FILE` | Use a prebuilt `ps2-tools` |

The Rust tools come from `--recomp-exe`/`--tools-exe`, from `bin/` next to the script (the launcher's kit) or from
`cargo build`, in that order.

4. Play with `./run_gow.sh`, or install the `.deb`/AppImage and point the launcher at the installation.

The launcher's setup wizard also runs on Linux: it starts `install.sh` with the ISO and the destination you chose and
shows the same eight stages. When packages are missing it lists them with the command for your distribution (it can't
run `sudo` for you), and you press **Check again** after installing them. This path has unit tests (the arguments, the
report of `--check-deps` and the wizard step) but has not been run on a real Linux machine, so if it fails, the script
above is the fallback. The launcher can also find and start an installation made by the script (**Files → Find files
automatically**).

## What the installation creates

| Folder / file | Contents |
|---|---|
| `game/` | The extracted disc: `SCUS_973.99`, the data files, `.disc_layout` (sector map), `IOP_MOD/` and, after your first save, `Mcd001.ps2` |
| `build-gen/` | Work folder of the recompiler: the filled-in `recomp.toml`, `recomp.log`, the generated C++ in `out/`, `optimize.log` and the captured IOP modules |
| `build/` | The CMake/Ninja tree and the game: `build/ps2xRuntime/gow-2005` (`gow-2005.exe` on Windows) |
| `vu1_captures/` | VU1 micro-memory images (16 KB each) that let the vector unit run native code |
| `build-rust/` | `ps2_recomp` and `ps2-tools` when they were built from source |
| `textures/replacements/` | Where a custom texture pack goes ([`TEXTURES.md`](TEXTURES.md)) |
| `toolchain/` | Portable Git (Windows only, only when Git was missing) |
| `RustPortPS2/native/ps2xRuntime/src/runner/` | The generated C++ of the game. **Always in the repository**, even with `-Destination` |

On Windows, `build\ps2xRuntime\game-elf.sha256` records which ELF the build belongs to; the launcher checks it before
starting the game.

## The optimization pass

The game's vector unit (VU1) and its I/O processor (IOP) can run either interpreted or as native code. The installer
already does the important half automatically:

- **VU1, always**: the microcode is extracted from the ELF itself using the committed manifest
  (`RustPortPS2/games/scus-97399/vu1.manifest`), before the first build. No game session is needed.
- **IOP and extra VU1, optionally**: `-Optimize` / `--optimize` starts the game with automatic button presses for 240
  seconds, records the code it executes, and rebuilds with it. Run it again after playing new areas to convert more of
  the game to native code; the port gets faster each time.

The launcher does the same thing in the background, after you close the game (**Performance → Background
optimization**), and shows a percentage. *Also while playing* is off by default because a rebuild with the game open
freezes the picture.

> While the optimization run is going, the game opens by itself and plays the beginning of the first level. **Do not
> touch the keyboard or the controller** until the window closes.

## Updating

```bash
git pull
./install.sh        # Windows: install.cmd
```

Only changed files are rebuilt. The extracted game, the recorded VU1/IOP code and your memory card are kept.

## Removing the installation

Nothing is installed outside the folders above. Delete the installation folder (or, in a checkout, `game/`,
`build/`, `build-gen/`, `vu1_captures/`, `build-rust/`) and, for the launcher, its data folder
(`%APPDATA%\GOW_PC\Launcher` or `~/.local/share/GOW_PC/Launcher`). **Copy `Mcd001.ps2` out before deleting `game/`**:
that file is your save.
