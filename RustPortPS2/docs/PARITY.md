# Parity gate

The Rust recompiler is accepted only when it produces **the same bytes** as the C++ one for the retail game. This
document defines the gate; `ps2-parity` (the comparison tool, `crates/ps2-parity`) implements it.

## What is compared

1. Every file of the generated tree: relative path, size and SHA-256.
2. The generated headers, which `install.ps1` copies into `native/ps2xRuntime/include/`.
3. The counters the reporter prints: functions translated, stubs, skips, unresolved jumps.

The comparison is **per operating system**. The C++ writer uses `std::ofstream` in text mode, so the Windows tree has
CRLF and the Linux tree LF; a Windows tree can never match a Linux tree.

## Why a manifest and not the generated files

The generated C++ is derived from the game's executable and must never be committed (see the "Legal" section of
`AGENTS.md`). The gate therefore commits only a manifest:

```json
{
  "generator": "ps2_recomp",
  "generator_version": "scus-97399-cpp-baseline",
  "operating_system": "windows",
  "elf_sha256": "…",
  "files": [
    { "path": "func_00100008.cpp", "size": 12034, "sha256": "…" }
  ],
  "counts": { "functions": 6414, "stubs": 78, "skips": 0, "unresolved": 0 }
}
```

Manifests live in `RustPortPS2/tests/parity/manifests/<elf-sha256>-<os>.json`.

## Status

| Gate | Result |
|---|---|
| Retail ELF with the Ghidra map (`RustPortPS2/games/scus-97399/funcmap.csv`), Windows | **identical**: 6,418 of 6,418 files, same report (`ec9397d2...-windows.json`) |
| Retail ELF without the map (heuristic function discovery), Windows | **identical**: 6,426 of 6,426 files (run locally, not committed) |
| Synthetic fixture, Windows | **identical** (`fixture-windows.json`, runs in CI) |
| Linux (retail ELF and fixture) | **pending**: needs a C++ baseline built on Linux (`fixture-linux.json`); the fixture test skips an OS with no baseline |
| The game booting from a Rust-generated tree | **pending**: needs someone who has the game |

## `ps2-parity`

```text
ps2-parity compare A B [--show N]                   two trees, first differing line of the first N differing files
ps2-parity extract A B <path>                       the first difference of one file
ps2-parity manifest TREE --elf FILE --version TEXT --out FILE [--log recomp.log]
ps2-parity verify TREE MANIFEST                     a tree against a committed manifest
ps2-parity fixture --out DIR                        writes fixture.elf and recomp.toml (output in DIR/out/)
```

It has no dependencies: SHA-256 and the manifest JSON are implemented in the crate. `compare` exits 1 when the trees
differ.

## Local procedure (needs the user's game)

1. Build the Rust `ps2_recomp` and `ps2-parity` (`cargo build --release -p ps2-recomp-cli -p ps2-parity` in
   `RustPortPS2/`). The reference C++ recompiler (PS2Recomp's `ps2xRecomp`) is not part of this repository: build it
   from PS2Recomp upstream. In day-to-day use, the manifest committed in step 4 replaces it.
2. Fill in the placeholders of `RustPortPS2/games/scus-97399/gow-port.toml` (`@ELF@`, `@MAP@`, `@OUT@`) to make a
   temporary `recomp.toml`, exactly as `install.ps1` does. Run the C++ recompiler with it into `A/`, then run the Rust
   one with the same file (but a different `@OUT@`) into `B/`.
3. `ps2-parity compare A B` must print `different: 0`. Compare the two logs too: they must match except for the order of
   the `control-flow` warnings.
4. `ps2-parity manifest A --elf SCUS_973.99 --version scus-97399-cpp-baseline --out
   tests/parity/manifests/<elf-sha256>-<os>.json --log recomp.log` records the baseline;
   `ps2-parity verify B tests/parity/manifests/<file>` checks any later tree against it without the C++ build.

A failure is triaged one function at a time: `ps2-parity extract A B <path>` prints the first differing line, and the
function's address is in the file's header comment.

## CI procedure (no game)

`crates/ps2-parity/src/fixture.rs` builds a synthetic ELF in memory and `tests/fixture.rs` runs the whole Rust
pipeline on it, then verifies the tree against `tests/parity/manifests/fixture-<os>.json`. The fixture is not derived
from the game, so its baseline can be committed; it holds one function per instruction family, swept over every
sub-opcode and four operand patterns (including `$zero` forms), the VU0 macro block over four destination masks, a jump
table, a GIF DMA kick, calls, a loop, a likely branch, a syscall, a function that falls off its end, and a config with a
stub, a syscall stub, a skipped function, an entry hint, a patch and an MMIO mark. It found three translator bugs that
the retail game does not reach.

To regenerate a baseline after changing the fixture: `ps2-parity fixture --out DIR`, run the C++ `ps2_recomp
DIR/recomp.toml`, then `ps2-parity manifest DIR/out --elf DIR/fixture.elf --version fixture-cpp-baseline --out
tests/parity/manifests/fixture-<os>.json --log <the C++ log>`.

## What the gate does not prove

- That the runtime executes the code correctly. That is the game's smoke test (`last.log` without new `[recomp]` errors,
  the first level playable), run by someone who has the game.
- That the ELF is the retail one. The manifest records `elf_sha256`, so a different dump is simply a different baseline.
