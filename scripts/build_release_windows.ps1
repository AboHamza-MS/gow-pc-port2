# GOW-Port: builds the runtime (gow-2005.exe, CMake target ps2EntryRunner) in Release, without LTCG, with the same configuration the installer uses.
# Usage: build_release_windows.ps1 [-Jobs 10] [-Data C:\GowTeste2] [-ConfigureOnly] [-ToolsExe ps2-tools.exe]
#   -Data: folder with vu1_captures\ (VU1 microcode) and, optionally, build\_deps (already downloaded dependencies).
#           The build goes to RustPortPS2\native\build (ignored by git).
#   -ToolsExe: a built ps2-tools (generates the native VU1 blocks); by default build-rust\release\ps2-tools.exe, built with cargo.
param([int]$Jobs = 10, [string]$Data = 'C:\GowTeste2', [switch]$ConfigureOnly, [string]$ToolsExe = '')
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (-not (Test-Path -LiteralPath $vswhere)) { throw 'Visual Studio Build Tools was not found.' }
$vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vs) { throw 'MSVC x64 was not found.' }
$cmake = Join-Path $vs 'Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe'
$ninja = Join-Path $vs 'Common7\IDE\CommonExtensions\Microsoft\CMake\Ninja\ninja.exe'
$vcvarsCmd = Join-Path $env:TEMP 'gow_release_vcvars.cmd'
Set-Content -LiteralPath $vcvarsCmd -Value "@call `"$(Join-Path $vs 'VC\Auxiliary\Build\vcvars64.bat')`" >nul`r`n@set" -Encoding ASCII
cmd /c $vcvarsCmd | ForEach-Object { if ($_ -match '^([^=]+)=(.*)$') { Set-Item -Path "env:$($matches[1])" -Value $matches[2] } }
function Slashes([string]$path) { return ($path -replace '\\', '/') }

$recomp = Join-Path $root 'RustPortPS2\native'
$build = Join-Path $recomp 'build'
$vu1 = Join-Path $Data 'vu1_captures'
if (-not (Test-Path -LiteralPath $vu1 -PathType Container)) { throw "VU1 microcode is missing: $vu1" }
# vu1n_heads.txt is read next to the microcode folder (CMake: ${PS2X_VU1_MICROCODE_DIR}/../vu1n_heads.txt)
Copy-Item -LiteralPath (Join-Path $root 'RustPortPS2\games\scus-97399\vu1-heads.list') -Destination (Join-Path $Data 'vu1n_heads.txt') -Force
$ToolsExe = if ($ToolsExe) { $ToolsExe } else { Join-Path $root 'build-rust\release\ps2-tools.exe' }
if (-not (Test-Path -LiteralPath $ToolsExe)) {
    & cargo build --release --locked -p ps2-tools --manifest-path (Join-Path $root 'RustPortPS2\Cargo.toml') --target-dir (Join-Path $root 'build-rust')
    if ($LASTEXITCODE -ne 0) { throw 'cargo build of ps2-tools failed.' }
}
$cmakeArgs = @('-S', $recomp, '-B', $build, '-G', 'Ninja', "-DCMAKE_MAKE_PROGRAM=$ninja", '-DCMAKE_BUILD_TYPE=Release',
    '-DPS2X_ENABLE_RUNTIME_LOGS=OFF', '-DPS2X_ENABLE_AGRESSIVE_LOGS=OFF', '-DPS2X_ENABLE_IOP_RPC_TRACE=OFF',
    '-DPS2X_ENABLE_LTCG=OFF', '-DPS2X_VU1_AVX2=OFF', '-DPS2X_MAP=ON',
    '-DPS2X_BUILD_TEST=OFF', '-DPS2X_SHOW_WINDOWS_CONSOLE=OFF',
    '-DPS2X_RUNNER_UNITY_BUILD_BATCH_SIZE=8',
    "-DPS2X_VU1_MICROCODE_DIR=$(Slashes $vu1)",
    "-DPS2X_VU1_GENERATOR_SOURCE=$(Slashes (Join-Path $recomp 'tools\vu1\generate_vu1.cpp'))",
    "-DPS2X_TOOLS_EXE=$(Slashes $ToolsExe)")
# Dependencies already downloaded (same versions, no network)
$deps = Join-Path $Data 'build\_deps'
foreach ($n in 'imgui', 'raylib', 'rlimgui') {
    $src = Join-Path $deps "$n-src"
    if (Test-Path -LiteralPath $src -PathType Container) { $cmakeArgs += "-DFETCHCONTENT_SOURCE_DIR_$($n.ToUpper())=$(Slashes $src)" }
}
& $cmake @cmakeArgs
if ($LASTEXITCODE -ne 0) { throw 'The runtime CMake configuration failed.' }
if ($ConfigureOnly) { Write-Host 'Configured.'; return }
& $ninja -C $build "-j$Jobs" ps2EntryRunner
if ($LASTEXITCODE -ne 0) { throw 'The runtime build failed.' }
Write-Host (Join-Path $build 'ps2xRuntime\gow-2005.exe')
