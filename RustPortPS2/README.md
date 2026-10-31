# RustPortPS2

Everything in the port except the game-specific C++: the PlayStation 2 static recompiler
([PS2Recomp](https://github.com/ran-j/PS2Recomp)'s behavior, implemented in Rust), the tools the installers use, the
developer tools, and `native/`, the C++ runtime that executes the recompiled game.

The installers run `ps2_recomp` (`bin/ps2_recomp` in the launcher's kit, `-RecompExe` / `--recomp-exe`, or a cargo
build). `ps2-tools` does the installers' mechanical work (disc extraction, ELF identity check, VU1 and IOP generators).

## Status

The recompiler reproduces the C++ recompiler's output **byte for byte** on `SCUS_973.99` (verified on Windows): 6,418 of
6,418 generated files with the Ghidra function map and 6,426 of 6,426 without it (heuristic function discovery), with
the same report. The reference tree is not committed; the parity gate keeps manifests instead. See
[`docs/PARITY.md`](docs/PARITY.md).

## Requirements

Rust (stable) and a C compiler: `third_party/rabbitizer` is the C decoder (vendored, tag `1.14.3`, see its `VENDORED.md`),
compiled by `cc`. On Windows, load the MSVC environment (`vcvars64.bat`), as for the rest of the port.

## Commands (from this folder)

```sh
cargo test --workspace          # all crates, no game files needed (the parity fixture is built in memory)
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt -p ps2-config -p ps2-elf -p ps2-r5900 -p ps2-recomp -p ps2-recomp-cli -p ps2-parity -p ps2-tools -p ps2-devtools -- --check

cargo build --release -p ps2-recomp-cli   # target/release/ps2_recomp <config.toml>
cargo build --release -p ps2-tools        # target/release/ps2-tools (iso-extract, verify-elf, vu1-*, iop-gen, vu1n-gen)
cargo build --release -p ps2-devtools     # target/release/ps2-dev (validators, profile and capture tools)
cargo build --release -p ps2-parity       # target/release/ps2-parity (see docs/PARITY.md)
```

Never run `cargo fmt --all`: it also formats local path dependencies, so it rewrites the vendored Rabbitizer. Add every
new crate to that list (CI does the same).

## Rules

- Everything here is new code, so no `// GOW-Port:` marker is needed (the marker is for changes to C++ that came from upstream).
- Never commit the game's ELF, generated C++ or generated Rust. The parity gate commits manifests (hashes) only.
- Output must stay byte-identical to the C++ recompiler: no `HashMap` on the emission path, no unordered iteration.
- GPL-3.0-only, same as the repository root.

## Layout

| Crate | Role |
|---|---|
| `ps2-config` | TOML configuration (`gow-port.toml`): stubs, skip, MMIO, instruction patches, entry points; Ghidra map input |
| `ps2-elf` | ELF32/MIPS loader: sections, symbols, relocations, `readWord`, function map, function extraction |
| `ps2-r5900` | R5900 fields, semantic decode and Rabbitizer disassembly |
| `ps2-recomp` | Translators (generic, `SPECIAL`, `REGIMM`, `COP0`, `COP1`, `COP2`/VU0 macros, `MMI`), control-flow analysis and emitter, function emitters, GIF-DMA-kick analysis, reporter, heuristic function scan |
| `ps2-recomp-cli` | The `ps2_recomp` binary |
| `ps2-parity` | Tree comparison, manifests and the synthetic fixture of the byte-identical gate |
| `ps2-devtools` | `ps2-dev`: config/test validators, `GOW_PERF_DIAG`/`GOW_SAMPLING` summaries, GIF/VIF capture inspectors, call-list generator, drivers of the C++ test executables |
| `ps2-tools` | Installer helpers: ISO9660 extractor, ELF identity check, VU1 manifest and programs, IOP AOT generator, native VU1 block generator (`vu1-manifest extend` derives manifest metadata from local captures without copying game code) |

`native/` is not a crate: it is the C++ runtime and IOP (`ps2xRuntime`, `ps2xIOP`, `ps2xTest`) built with CMake.
`PS2X_TOOLS_EXE` points its CMake at `ps2-tools` for the VU1 blocks.
