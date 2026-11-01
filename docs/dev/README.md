# Developer documentation

Everything a contributor needs, in the order most people need it. Player documentation is one level up, in
[`../`](../README.md); project rules for agents and contributors are in [`../../AGENTS.md`](../../AGENTS.md).

## Five-minute orientation

1. The game's MIPS code is **recompiled to C++** once, at install time (`ps2_recomp`, Rust), and then compiled as a
   normal program (`gow-2005`). At runtime nothing interprets the game's own code.
2. That generated C++ runs on a runtime that is also C++ (`ps2xRuntime`, `ps2xIOP`) and implements the PS2 hardware
   natively: GS, VU1, IOP, DMA, SIF, sound, pad, memory card.
3. The launcher (`gow-launcher/`, Rust + Tauri + React) installs the game, stores settings and starts the executable
   with environment variables.
4. Rust never runs inside the game process. C++ never performs an installation step.

```
RustPortPS2/
  crates/            ps2-config · ps2-elf · ps2-r5900 · ps2-recomp · ps2-recomp-cli · ps2-parity · ps2-tools · ps2-devtools
  native/            ps2xRuntime · ps2xIOP · ps2xTest · tools · tests  (CMake)
  games/scus-97399/  recompiler config, function map, VU1 manifests
  third_party/       vendored Rabbitizer
  docs/              PARITY.md
gow-launcher/        gow-core · gow-cli · fake-runner · src-tauri · src (React)
docs/                player docs   ·   docs/dev/ this folder
install.sh · install.ps1 · install.cmd · run_gow.sh
scripts/             PowerShell build helpers, CI script checker
```

## Where to read next

| You want to… | Read |
|---|---|
| Understand how the pieces fit | [`ARCHITECTURE.md`](ARCHITECTURE.md) |
| Build the runtime, tune CMake, run the tests | [`BUILD.md`](BUILD.md) |
| Know what an environment variable does | [`ENVIRONMENT.md`](ENVIRONMENT.md) |
| Measure or debug performance | [`PERFORMANCE.md`](PERFORMANCE.md) |
| Work on the graphics layer | [`RENDERING.md`](RENDERING.md) |
| Work on sound or movies | [`AUDIO-VIDEO.md`](AUDIO-VIDEO.md) |
| Work on saves | [`MEMORY-CARD.md`](MEMORY-CARD.md) |
| Work on cheats, lock-on, agile combat, input | [`GAMEPLAY-HOOKS.md`](GAMEPLAY-HOOKS.md) |
| Understand the byte-identical recompiler gate | [`../../RustPortPS2/docs/PARITY.md`](../../RustPortPS2/docs/PARITY.md) |
| Work on the launcher | [`../../gow-launcher/README.md`](../../gow-launcher/README.md) |
| Release the launcher so that installed copies can update themselves (keys, secrets, `latest.json`) | [`UPDATER.md`](UPDATER.md) |

## The commands you will actually use

```bash
# recompiler, tools and their tests (no game files needed)
cd RustPortPS2
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt -p ps2-config -p ps2-elf -p ps2-r5900 -p ps2-recomp -p ps2-recomp-cli -p ps2-parity -p ps2-tools -p ps2-devtools -- --check

# the game's gate
cargo run --release -p ps2-devtools -- validate-config

# build the runtime (after an install has generated the code)
ninja -C native/build ps2EntryRunner

# launcher
cd ../gow-launcher && npm ci && npm test && cargo test --workspace
```

`ps2-dev` with no arguments lists every developer command (validators, `perf-summary`, `sampling`,
`gif-inspect`, `vif-extract`, `cancel-line`, `gen-call-list`, the C++ test drivers).

## House rules that bite

- **Never edit generated code**: `ps2xRuntime/src/runner/*.cpp`, `ps2xIOP/src/native/iopn_*`,
  `build/*/vu1*_generated`. The installer and the launcher's optimizer regenerate them.
- **Header changes are expensive**: anything included by the generated units (`ps2_runtime.h`, `ps2_memory.h`,
  `ps2_perf.h`, the access macros) rebuilds ~6,400 files. Batch CMake edits too: a `CMakeLists.txt` change regenerates
  the precompiled header.
- **Mark upstream code**: every change to code that came from PS2Recomp / the native port / the SotC GS backend gets a
  `// GOW-Port:` comment.
- **Overrides call the original** for everything they do not replace, and defer to it when resuming from a checkpoint
  (`ctx->pc` different from the entry).
- **Diagnostics are opt-in** through an environment variable and log with a `[gow-…]` tag; see
  [`ENVIRONMENT.md`](ENVIRONMENT.md).
- **English everywhere**: code, comments, identifiers, file names, log messages, commits. The only translations are
  the player README, the launcher and the in-game cheat menu.
- **No game data**: never commit the ELF, the extracted disc, generated C++, RAM/VRAM dumps or captured VU1 microcode.

## Documentation rules

`docs/` is for players; `docs/dev/` is for contributors. One subject per file. No dated session logs, no personal
paths. When behavior changes, update the page that documents it: if code and a page disagree, the code wins and the
page is fixed in the same change.
