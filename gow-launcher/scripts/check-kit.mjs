// Verifies that the installation kit is complete: everything `install.ps1`, `install.sh` and the CMake files read from
// the repository is inside the kit, and every `install.ps1` parameter the launcher passes still exists.
//
// A file the installers need but the kit left out makes the wizard fail only on a user's machine, hours in. This turns
// it into a failure in CI.
//
//   node scripts/check-kit.mjs            (after `node scripts/stage-kit.mjs`)

import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { join, posix, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { defaultKitDir, repositoryRoot, trackedFiles } from './stage-kit.mjs';

const slashes = (text) => text.replaceAll('\\', '/');

/** Folders the installers write into (or that hold the user's data); only a placeholder of them is tracked. */
const OUTPUT_ROOTS = new Set(['game', 'build', 'build-gen', 'vu1_captures', 'toolchain', 'logs', 'active', 'RustPortPS2/native/build']);

/**
 * Sources the installers only use to build the Rust tools from a repository checkout (cargo, with the vendored
 * `rabbitizer`). The kit carries the prebuilt `bin/ps2_recomp` and `bin/ps2-tools` instead, so these are not required.
 */
const DEVELOPER_ONLY = ['RustPortPS2/Cargo.toml', 'RustPortPS2/crates', 'RustPortPS2/third_party', 'RustPortPS2/target'];
const isDeveloperOnly = (path) => DEVELOPER_ONLY.some((root) => path === root || path.startsWith(`${root}/`));

/** Cuts a path at its first dynamic part (`$var` or a glob) and drops what follows a `>` of `$<...>` expressions. */
function cutDynamic(raw) {
  const path = raw.replace(/>.*$/, '');
  const at = path.search(/[$*<]/);
  return at < 0 ? path : path.slice(0, Math.max(0, path.lastIndexOf('/', at)));
}

/** A path as written in a script, made repository-relative. A dynamic tail (`src\$f`) is cut to its directory. */
function clean(raw) {
  return cutDynamic(slashes(raw).replace(/^\.?\/+/, '')).replace(/\/+$/, '');
}

const stripPowershellComments = (text) => text.replace(/(^|\s)#.*$/gm, '$1');
const stripShellComments = stripPowershellComments;
const stripCMakeComments = (text) => text.replace(/#.*$/gm, '');

/** Repository paths `install.ps1` reads: `$Root`-relative and `$Native`-relative. */
export function powershellPaths(text) {
  const code = stripPowershellComments(text);
  const found = new Set();
  const add = (raw, base = '') => {
    const path = clean(raw);
    if (path) found.add(base + path);
  };
  for (const match of code.matchAll(/Join-Path\s+\$Root\s+(['"])([^'"]+)\1/g)) add(match[2]);
  for (const match of code.matchAll(/\$Root[\\/]([\w.\-\\/$]+)/g)) add(match[1]);
  for (const match of code.matchAll(/Join-Path\s+\$Native\s+(['"])([^'"]+)\1/g)) add(match[2], 'RustPortPS2/native/');
  return [...found];
}

/** Repository paths `install.sh` reads. */
export function shellPaths(text) {
  const code = stripShellComments(text);
  const found = new Set();
  for (const match of code.matchAll(/\$\{?ROOT\}?"?\/([\w.\-/$]+)/g)) {
    const path = clean(match[1]);
    if (path) found.add(path);
  }
  for (const match of code.matchAll(/\$\{?NATIVE\}?"?\/([\w.\-/$]+)/g)) {
    const path = clean(match[1]);
    if (path) found.add(`RustPortPS2/native/${path}`);
  }
  return [...found];
}

/** Repository paths a CMake file (at `file`, repository-relative) refers to by `${CMAKE_*_SOURCE_DIR}/...` or `../`. */
export function cmakePaths(text, file) {
  const code = stripCMakeComments(text);
  const base = posix.dirname(file);
  const found = new Set();
  const add = (directory, relative) => {
    const resolved = posix.normalize(posix.join(directory, cutDynamic(relative)));
    if (resolved && !resolved.startsWith('..') && resolved !== '.') found.add(resolved.replace(/\/+$/, ''));
  };
  for (const match of code.matchAll(/\$\{CMAKE_CURRENT_SOURCE_DIR\}\/([^\s"')]+)/g)) add(base, match[1]);
  for (const match of code.matchAll(/\$\{CMAKE_SOURCE_DIR\}\/([^\s"')]+)/g)) add('RustPortPS2/native', match[1]);
  for (const match of code.matchAll(/(?:^|[\s"'(])((?:\.\.\/)+[^\s"')]+)/g)) add(base, match[1]);
  return [...found];
}

/** Names of the parameters in the `param(...)` block of install.ps1. */
export function installerParameters(text) {
  const block = /\bparam\s*\(([\s\S]*?)\n\)/i.exec(text)?.[1] ?? '';
  return new Set([...block.matchAll(/\[(?:switch|string|int|double|hashtable)\]\s*\$(\w+)/gi)].map((m) => m[1].toLowerCase()));
}

/** `-Name` arguments the launcher's Rust code passes to install.ps1. */
export function passedParameters(rustText) {
  return [...rustText.matchAll(/"-(\w+)"\s*\.to_string\(\)/g)].map((m) => m[1]);
}

function walk(directory, base = directory, out = new Set()) {
  if (!existsSync(directory)) return out;
  for (const name of readdirSync(directory)) {
    const full = join(directory, name);
    if (statSync(full).isDirectory()) walk(full, base, out);
    else out.add(slashes(full.slice(base.length + 1)));
  }
  return out;
}

/**
 * Checks `kitDir` against what the installers reference. Returns `{ errors, notes, checked }`: an error is a tracked
 * file or folder the installers read that is not in the kit; a note is a reference to something not tracked at all
 * (usually a generated or output path).
 */
export function checkKit({ repository = repositoryRoot, kitDir = defaultKitDir, tracked = trackedFiles(repository) } = {}) {
  const inKit = walk(kitDir);
  const errors = [];
  const notes = [];
  const references = new Map(); // path -> who asked for it
  const ask = (paths, who) => paths.forEach((path) => references.has(path) || references.set(path, who));

  // The kit must carry the prebuilt Rust tools. Without them install.ps1 falls back to `cargo build` on the *user's* PC,
  // which nobody who downloads the launcher has: the wizard would fail after hours of build.
  const exe = process.platform === 'win32' ? '.exe' : '';
  for (const name of ['bin/ps2_recomp', 'bin/ps2-tools']) {
    if (!inKit.has(`${name}${exe}`)) {
      errors.push(
        `the kit has no ${name}${exe}: run \`node scripts/stage-kit.mjs\` (it builds the tools) so the install does not ` +
          "need cargo on the user's PC",
      );
    }
  }

  const read = (file) => readFileSync(join(repository, file), 'utf8');
  if (existsSync(join(repository, 'install.ps1'))) ask(powershellPaths(read('install.ps1')), 'install.ps1');
  if (existsSync(join(repository, 'install.sh'))) ask(shellPaths(read('install.sh')), 'install.sh');
  for (const file of tracked.filter((f) => f.startsWith('RustPortPS2/native/') && /(?:^|\/)(?:CMakeLists\.txt|[^/]+\.cmake)$/.test(f))) {
    ask(cmakePaths(read(file), file), file);
  }

  for (const [path, who] of references) {
    if (OUTPUT_ROOTS.has(path) || isDeveloperOnly(path)) continue;
    const isTracked = tracked.some((file) => file === path || file.startsWith(`${path}/`));
    if (!isTracked) {
      notes.push(`${who}: ${path} is not tracked (generated or output path?)`);
      continue;
    }
    const present = inKit.has(path) || [...inKit].some((file) => file.startsWith(`${path}/`));
    if (!present) errors.push(`${who} reads ${path}, which is tracked but missing from the kit`);
  }

  // Parameters the launcher passes to install.ps1 must still exist.
  if (existsSync(join(repository, 'install.ps1'))) {
    const accepted = installerParameters(read('install.ps1'));
    for (const file of ['installer.rs', 'optimizer.rs']) {
      const source = join(repository, 'gow-launcher', 'crates', 'gow-core', 'src', file);
      if (!existsSync(source)) continue;
      for (const name of passedParameters(readFileSync(source, 'utf8'))) {
        if (!accepted.has(name.toLowerCase())) errors.push(`${file} passes -${name}, which install.ps1 does not accept`);
      }
    }
  }
  return { errors, notes, checked: references.size };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  if (!existsSync(defaultKitDir)) {
    console.error(`No kit at ${defaultKitDir}: run \`node scripts/stage-kit.mjs\` first.`);
    process.exit(2);
  }
  const { errors, notes, checked } = checkKit();
  notes.forEach((note) => console.log(`note: ${note}`));
  if (errors.length > 0) {
    errors.forEach((error) => console.error(`error: ${error}`));
    process.exit(1);
  }
  console.log(`kit complete: ${checked} references checked`);
}
