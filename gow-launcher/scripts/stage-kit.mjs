// Stages the installation kit that travels inside the launcher's installer (see crates/gow-core/src/kit.rs).
//
// Copies the tracked files the installers need from the repository root into src-tauri/kit/, which Tauri bundles as
// a resource. Only files tracked by git are taken, so nothing generated from the game (or any local scratch file) can
// end up in a release. `scripts/check-kit.mjs` verifies that nothing the installers reference was left out.
//
//   node scripts/stage-kit.mjs

import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { cpSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { basename, dirname, extname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
export const repositoryRoot = resolve(here, '..', '..');
export const defaultKitDir = resolve(here, '..', 'src-tauri', 'kit');

// What install.ps1 / install.sh read from their own folder.
export const INCLUDE = ['install.ps1', 'install.cmd', 'install.sh', 'RustPortPS2/native/', 'RustPortPS2/games/'];
// Tracked, but not needed to install (tests, docs...) or not for end users.
export const EXCLUDE = [/^RustPortPS2\/native\/(?:.*\/)?tests?\//, /^RustPortPS2\/native\/tools\/profiling\//];

/** Every file tracked by git, as repository-relative paths with forward slashes. */
export function trackedFiles(repository = repositoryRoot) {
  return execFileSync('git', ['ls-files', '-z'], { cwd: repository, maxBuffer: 1 << 28 })
    .toString('utf8')
    .split('\0')
    .filter(Boolean);
}

/** The tracked files that belong in the kit. */
export function kitFiles(tracked) {
  return tracked.filter(
    (file) =>
      INCLUDE.some((prefix) => (prefix.endsWith('/') ? file.startsWith(prefix) : file === prefix)) &&
      !EXCLUDE.some((rule) => rule.test(file)),
  );
}

/** Files that look like game data: they must never be staged. */
export function forbiddenFiles(files) {
  return files.filter((file) => /\.(iso|pak|irx|elf|bin)$/i.test(file) || /^SCUS_/i.test(file.split('/').pop() ?? ''));
}

/**
 * Where the prebuilt Rust executables go in the kit: `ps2_recomp` (the recompiler) and `ps2-tools` (ISO extraction, ELF
 * check, VU1/IOP generators), both from RustPortPS2/. The installers use `bin/<name>[.exe]` when it is there and build
 * it with cargo when it is not (a repository checkout).
 */
export const RECOMP_BINARY_DIR = 'bin';

/**
 * Builds the two Rust executables the installers run, so a release never depends on binaries that happened to be lying
 * around on the build machine: without them the kit would fall back to `cargo build` on the *user's* PC (install.ps1
 * asks for Rust), which nobody who downloads the launcher has.
 *
 * `PS2_RECOMP_BIN`/`PS2_TOOLS_BIN` still win, for a CI that reuses build artifacts. Both come out of one cargo build.
 */
export function buildRustTools({ repository = repositoryRoot, cargo = process.env.CARGO ?? 'cargo' } = {}) {
  const manifest = join(repository, 'RustPortPS2', 'Cargo.toml');
  console.log('kit: building ps2_recomp and ps2-tools with cargo (release)...');
  try {
    execFileSync(
      cargo,
      ['build', '--release', '--locked', '-p', 'ps2-recomp-cli', '-p', 'ps2-tools', '--manifest-path', manifest],
      { cwd: repository, stdio: 'inherit' },
    );
  } catch (error) {
    throw new Error(
      'could not build ps2_recomp/ps2-tools with cargo ' +
        `(${error.message}). A release must ship them: install Rust, or set PS2_RECOMP_BIN and PS2_TOOLS_BIN to prebuilt ` +
        'executables.',
    );
  }
  const dir = join(repository, 'RustPortPS2', 'target', 'release');
  const exe = process.platform === 'win32' ? '.exe' : '';
  return { recomp: join(dir, `ps2_recomp${exe}`), tools: join(dir, `ps2-tools${exe}`) };
}

/** In the kit: the stamp of the Rust tools' sources. See `TOOLS_STAMP` in crates/gow-core/src/rebuild.rs. */
export const TOOLS_STAMP = 'kit-tools.stamp';

/**
 * A hash of what the two prebuilt Rust tools are made of: the content of every tracked file under `RustPortPS2/crates/`
 * plus the workspace manifest and lock. The launcher compares kits by this instead of by the executables' bytes: compiling the
 * same sources twice gives different bytes (timestamps, debug identifiers), which would make every release ask every player
 * to rebuild their game. The stamp changes only when the recompiler or the tools really changed.
 */
export function toolsStamp(repository = repositoryRoot, tracked = trackedFiles(repository)) {
  const inputs = tracked
    .filter((file) => file.startsWith('RustPortPS2/crates/') || file === 'RustPortPS2/Cargo.toml' || file === 'RustPortPS2/Cargo.lock')
    .sort();
  const outer = createHash('sha256');
  for (const file of inputs) {
    const path = join(repository, file);
    if (!existsSync(path)) continue; // tracked but deleted in the working tree
    // Line endings are normalized: a Windows checkout with autocrlf and a Linux one hold the same sources.
    const text = readFileSync(path).toString('latin1').replace(/\r\n/g, '\n');
    outer.update(`${file}\0${createHash('sha256').update(text, 'latin1').digest('hex')}\n`);
  }
  return outer.digest('hex');
}

/** Copies the kit into `target` (replacing it). Returns the staged files. */
export function stage({
  repository = repositoryRoot,
  target = defaultKitDir,
  recompBinary = process.env.PS2_RECOMP_BIN,
  toolsBinary = process.env.PS2_TOOLS_BIN,
  buildTools = buildRustTools,
} = {}) {
  const files = kitFiles(trackedFiles(repository));
  const forbidden = forbiddenFiles(files);
  if (forbidden.length > 0) {
    throw new Error('Refusing to stage files that look like game data:\n  ' + forbidden.join('\n  '));
  }
  rmSync(target, { recursive: true, force: true });
  for (const file of files) {
    const from = join(repository, file);
    if (!existsSync(from)) continue; // tracked but deleted in the working tree
    const to = join(target, file);
    mkdirSync(dirname(to), { recursive: true });
    cpSync(from, to, { preserveTimestamps: true });
  }
  // Both or neither: a release ships prebuilt tools (CI artifacts) or builds them here. It must never end up without them.
  for (const [variable, binary] of [
    ['PS2_RECOMP_BIN', recompBinary],
    ['PS2_TOOLS_BIN', toolsBinary],
  ]) {
    if (binary && !existsSync(binary)) throw new Error(`${variable} does not exist: ${binary}`);
  }
  const binaries = recompBinary && toolsBinary ? { recomp: recompBinary, tools: toolsBinary } : buildTools({ repository });
  for (const [variable, binary, stem] of [
    ['PS2_RECOMP_BIN', binaries.recomp, 'ps2_recomp'],
    ['PS2_TOOLS_BIN', binaries.tools, 'ps2-tools'],
  ]) {
    if (!binary) throw new Error(`${variable} is missing and the cargo build did not produce it`);
    if (!existsSync(binary)) throw new Error(`${variable} does not exist: ${binary}`);
    const name = `${stem}${extname(basename(binary)).toLowerCase() === '.exe' ? '.exe' : ''}`;
    mkdirSync(join(target, RECOMP_BINARY_DIR), { recursive: true });
    cpSync(binary, join(target, RECOMP_BINARY_DIR, name));
    files.push(`${RECOMP_BINARY_DIR}/${name}`);
  }
  writeFileSync(join(target, TOOLS_STAMP), `${toolsStamp(repository)}\n`);
  files.push(TOOLS_STAMP);
  return files;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    const files = stage();
    console.log(`kit: ${files.length} files staged into ${defaultKitDir}`);
  } catch (error) {
    console.error(error.message);
    process.exit(1);
  }
}
