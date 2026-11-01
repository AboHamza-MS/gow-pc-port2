# gow-launcher

Launcher and installer for the God of War (2005) native port, built with Rust (`gow-core`), Tauri 2 and React. What it
does for the player (tabs, options, wizard, paths and logs) is covered in [`docs/LAUNCHER.md`](../docs/LAUNCHER.md).

## Requirements

Rust (stable), Node 22+, and on Windows the Visual Studio Build Tools (the same ones the port needs). On Linux the Tauri
libraries: `libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libsoup-3.0-dev patchelf`.

## Everyday commands (from this folder)

```sh
npm ci
npm run dev                       # the interface in a browser, with a mock backend: http://localhost:1420
npm run dev -- --open /?state=missing   # same, with no game installed (opens the wizard)
npm test                          # interface and kit tests (Vitest)
npm run check-kit                 # stage the kit and verify the installers' references are all in it
cargo test --workspace            # core, CLI and end-to-end tests
npx tauri dev                     # the real app
npx tauri build --bundles nsis    # installer (stages the kit first); on Linux: --bundles deb,appimage
cargo run -p gow-cli -- env       # the environment the game would get with the saved settings
cargo run -p gow-cli -- elf-report <elf> [funcmap.csv]   # entry, sections and map coverage (Rust port parser)
```

A release must be **self-contained**: `scripts/stage-kit.mjs` builds `ps2_recomp` and `ps2-tools` with cargo (or takes
`PS2_RECOMP_BIN`/`PS2_TOOLS_BIN` from a CI that reuses artifacts), and `scripts/check-kit.mjs` fails when the kit has no
`bin/`. Without those binaries, `install.ps1` builds the tools with cargo on the *user's* PC, and a user who downloads
the launcher has no Rust toolchain.

The Windows NSIS release also bundles Microsoft's **offline WebView2 installer**. This makes it about 214 MiB by design:
it opens on a clean Windows 10/11 x64 PC with no existing WebView2 runtime and no download when the launcher is
installed. `npm run check-windows-installer` enforces this release contract in CI. The game wizard still needs Internet
access to download Visual Studio Build Tools and portable Git when they are missing, and Windows asks for administrator
approval for the Build Tools.

Do not close `gow-2005` by name while testing (see `AGENTS.md`); the tests start their own throwaway child process.

## Where things go

| Change | Where |
|---|---|
| Anything that is not drawing: settings, environment of the game, checks, installer, optimizer | `crates/gow-core` (with tests) |
| A new command or event for the interface | `src-tauri/src/lib.rs` (thin), the type in `src/lib/types.ts`, the call in `src/lib/backend.ts` (and its mock) |
| Appearance (parchment theme) | tokens and nine-slice classes in `src/styles.css`; the plates they draw are in `src/assets/theme/` |
| Texts | `src/i18n/en.json` and `pt-BR.json`; `src/lib/i18n.test.ts` fails if a used key is missing |
| A variable passed to the game | `crates/gow-core/src/launch.rs`, the golden test next to it, and `docs/LAUNCHER.md` |
| Self-update of the launcher | decisions in `crates/gow-core/src/update.rs`, the plugin and events in `src-tauri/src/updater.rs`, the notice in `src/components/Shell.tsx`; setup and release steps in [`docs/dev/UPDATER.md`](../docs/dev/UPDATER.md) |

## Notes

- If `install.ps1`, `install.sh` or a CMake file starts reading a new file, add its folder to `INCLUDE` in
  `scripts/stage-kit.mjs`; `npm run check-kit` (and CI) fails until the kit includes it.
- `scripts/stage-kit.mjs` stages the installation kit from **tracked files only** into `src-tauri/kit/` (git-ignored) and
  refuses anything that looks like game data. Never add game files, generated code or captured VU1 microcode.
- If a locally built NSIS installer reports `Error writing temporary file`, do not release that copy: it was most likely
  run from a directory with a low Windows integrity label. Copy or build the artifact in a normal-integrity directory.
  CI release artifacts are produced at normal integrity.
- `GOW_LAUNCHER_DATA_DIR=<folder>` runs the app with its own data folder, so tests do not touch the real one.
- `GOW_LAUNCHER_NO_UPDATE_CHECK=1` turns off the check for a newer launcher (it asks GitHub when the app opens).
  `npm run dev` with `?update=automatic`, `?update=portable` or `?update=manual` shows the update notice with the mock.
- A release tag `launcher-vX.Y.Z` must match the version in the workspace `Cargo.toml`, `src-tauri/tauri.conf.json` and
  `package.json` (the release job checks it): the launcher compares its own version with the tags to offer updates. The
  signing key, the public key in `tauri.conf.json` (still a placeholder) and the GitHub secrets for in-app updates are
  described in [`docs/dev/UPDATER.md`](../docs/dev/UPDATER.md); without them the launcher still builds and only offers a
  link to the release page.
- An executable run from a folder with a Low integrity label (the sandbox applies this label to this repository) runs
  with low integrity and cannot write to `%APPDATA%`; copy it to a normal folder first.
