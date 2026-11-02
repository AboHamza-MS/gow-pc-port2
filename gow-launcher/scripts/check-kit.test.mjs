// @vitest-environment node
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import {
  checkKit, cmakePaths, installerParameters, passedParameters, powershellPaths, shellPaths,
} from './check-kit.mjs';
import { forbiddenFiles, kitFiles, stage } from './stage-kit.mjs';

describe('what the installers reference', () => {
  it('reads $Root and $Native paths from install.ps1, ignoring comments and cutting dynamic parts', () => {
    const paths = powershellPaths(String.raw`
# Join-Path $Root 'commented\out.py'
$a = Join-Path $Root 'tools\vu1\generate_vu1.cpp'
$b = Join-Path $Root "RustPortPS2\games\scus-97399\gow-port.toml"
$c = Join-Path $Root "RustPortPS2\games\$f"
$d = Join-Path $Native 'ps2xIOP\src\native'
$e = "$Root\vu1n_heads.txt"
`);
    expect(paths).toEqual(
      expect.arrayContaining([
        'tools/vu1/generate_vu1.cpp', 'RustPortPS2/games/scus-97399/gow-port.toml', 'RustPortPS2/games', 'RustPortPS2/native/ps2xIOP/src/native', 'vu1n_heads.txt',
      ]),
    );
    expect(paths).not.toContain('commented/out.py');
  });

  it('reads $ROOT and $NATIVE paths from install.sh', () => {
    const paths = shellPaths(`
"$TOOLS" iop-gen "$GEN/iop_mods" "$NATIVE/ps2xIOP/src/native"
cp "$ROOT/tools/vu1/vu1_manifest.txt" x
`);
    expect(paths).toEqual(
      expect.arrayContaining(['tools/vu1/vu1_manifest.txt', 'RustPortPS2/native/ps2xIOP/src/native']),
    );
  });

  it('resolves CMake references that leave the native tree', () => {
    const paths = cmakePaths(
      `
# "\${CMAKE_CURRENT_SOURCE_DIR}/../ignored.py"
get_filename_component(GEN "\${CMAKE_CURRENT_SOURCE_DIR}/../../../tools/vu1/generate_vu1.cpp" ABSOLUTE)
set(OUTSIDE "\${CMAKE_CURRENT_SOURCE_DIR}/../../../../not-in-the-repository.py")
set(SRC "\${CMAKE_CURRENT_SOURCE_DIR}/../../../tools/render/replay_vif_chain.cpp")
file(GLOB X "\${CMAKE_CURRENT_SOURCE_DIR}/src/lib/*.cpp")
`,
      'RustPortPS2/native/ps2xRuntime/CMakeLists.txt',
    );
    expect(paths).toContain('tools/vu1/generate_vu1.cpp');
    expect(paths).toContain('tools/render/replay_vif_chain.cpp');
    expect(paths).toContain('RustPortPS2/native/ps2xRuntime/src/lib');
    expect(paths.some((p) => p.includes('ignored'))).toBe(false);
    expect(paths.some((p) => p.includes('not-in-the-repository'))).toBe(false); // leaves the repository: not ours to ship
  });

  it('compares the parameters the launcher passes with the param() block', () => {
    const accepted = installerParameters(`
param(
    [string]$Iso = '',
    [switch]$InstallTools,
    [int]$Jobs = 0,
    [string]$Destination = ''
)
`);
    expect([...accepted].sort()).toEqual(['destination', 'installtools', 'iso', 'jobs']);
    expect(passedParameters('let a = vec!["-Iso".to_string(), x, "-NoPause".to_string()];')).toEqual(['Iso', 'NoPause']);
  });
});

// Staging copies about a thousand files: slow runners need more than the default 5 s.
const exe = process.platform === 'win32' ? '.exe' : '';

describe('the staged kit', { timeout: 60_000 }, () => {
  let target;
  beforeAll(() => {
    target = mkdtempSync(join(tmpdir(), 'gow-kit-'));
  });
  afterAll(() => rmSync(target, { recursive: true, force: true }));

  /** Stand-ins for the two executables `stage` builds/ships, so the tests never run cargo. */
  const fakeTools = () => {
    const dir = mkdtempSync(join(tmpdir(), 'gow-tools-'));
    const recomp = join(dir, `ps2_recomp${exe}`);
    const tools = join(dir, `ps2-tools${exe}`);
    writeFileSync(recomp, 'stand-in for the real executable');
    writeFileSync(tools, 'stand-in for the real executable');
    return { recomp, tools };
  };

  it('is complete for the installers as they are in this repository', () => {
    const files = stage({ target, buildTools: fakeTools });
    expect(files.length).toBeGreaterThan(100);
    const { errors, checked } = checkKit({ kitDir: target });
    expect(errors).toEqual([]);
    expect(checked).toBeGreaterThan(20);
  });

  it('stamps the kit with the sources of the Rust tools, not with the bytes of the executables', () => {
    const files = stage({ target, buildTools: fakeTools });
    expect(files).toContain('kit-tools.stamp');
    const first = readFileSync(join(target, 'kit-tools.stamp'), 'utf8').trim();
    expect(first).toMatch(/^[0-9a-f]{64}$/);
    // Two builds of the same sources give different executables, and the same stamp.
    const other = (() => {
      const result = fakeTools();
      writeFileSync(result.recomp, 'a second build of the same sources');
      writeFileSync(result.tools, 'a second build of the same sources');
      return result;
    })();
    stage({ target, buildTools: () => other });
    expect(readFileSync(join(target, 'kit-tools.stamp'), 'utf8').trim()).toBe(first);
  });

  it('builds the Rust tools when no prebuilt ones are given', () => {
    let builds = 0;
    const files = stage({ target, buildTools: () => (builds += 1, fakeTools()) });
    expect(builds).toBe(1);
    expect(files).toContain(`bin/ps2_recomp${exe}`);
    expect(files).toContain(`bin/ps2-tools${exe}`);
  });

  it('puts the prebuilt Rust executables under bin/ when they are given', () => {
    const scratch = mkdtempSync(join(tmpdir(), 'gow-kit-bin-'));
    try {
      const recomp = join(scratch, `ps2_recomp${exe}`);
      const tools = join(scratch, `ps2-tools${exe}`);
      writeFileSync(recomp, 'stand-in for the real executable');
      writeFileSync(tools, 'stand-in for the real executable');
      const files = stage({ target: join(scratch, 'kit'), recompBinary: recomp, toolsBinary: tools });
      expect(files).toContain(`bin/ps2_recomp${exe}`);
      expect(files).toContain(`bin/ps2-tools${exe}`);
      expect(existsSync(join(scratch, 'kit', 'bin', `ps2_recomp${exe}`))).toBe(true);
      expect(existsSync(join(scratch, 'kit', 'bin', `ps2-tools${exe}`))).toBe(true);
      expect(() => stage({ target: join(scratch, 'kit2'), recompBinary: join(scratch, 'missing.exe') })).toThrow(/PS2_RECOMP_BIN/);
      expect(() => stage({ target: join(scratch, 'kit3'), toolsBinary: join(scratch, 'missing.exe') })).toThrow(/PS2_TOOLS_BIN/);
    } finally {
      rmSync(scratch, { recursive: true, force: true });
    }
  });

  it('never contains anything that looks like game data', () => {
    const files = stage({ target, buildTools: fakeTools });
    expect(forbiddenFiles(files)).toEqual([]);
    expect(forbiddenFiles(['game/SCUS_973.99', 'x/a.ISO', 'y/PART1.PAK'])).toHaveLength(3);
  });

  it('fails when a file the installers read is left out', () => {
    // An empty kit: everything install.ps1 reads that is tracked is reported.
    const emptyKit = mkdtempSync(join(tmpdir(), 'gow-kit-empty-'));
    try {
      const { errors } = checkKit({ kitDir: emptyKit });
      expect(errors.length).toBeGreaterThan(5);
      expect(errors.some((e) => e.includes('tools/vu1/generate_vu1.cpp'))).toBe(true);
    } finally {
      rmSync(emptyKit, { recursive: true, force: true });
    }
  });

  it('does not require the Rust crates: the kit carries the prebuilt binary', () => {
    const emptyKit = mkdtempSync(join(tmpdir(), 'gow-kit-rust-'));
    try {
      const { errors } = checkKit({ kitDir: emptyKit });
      expect(errors.filter((e) => e.includes('RustPortPS2/crates') || e.includes('RustPortPS2/third_party') || e.includes('RustPortPS2/Cargo.toml'))).toEqual([]);
    } finally {
      rmSync(emptyKit, { recursive: true, force: true });
    }
  });

  it('excludes tests, docs and CI helpers', () => {
    const files = kitFiles([
      'install.ps1', 'docs/INSTALL.md', 'scripts/ci/analyze_ps1.ps1', 'RustPortPS2/native/tests/a.cpp',
      'RustPortPS2/native/tools/profiling/sample.cpp', 'RustPortPS2/native/tools/vu1/generate_vu1.cpp',
      'RustPortPS2/native/ps2xRuntime/tests/a.cpp',
    ]);
    expect(files).toEqual(['install.ps1', 'RustPortPS2/native/tools/vu1/generate_vu1.cpp']);
  });
});
