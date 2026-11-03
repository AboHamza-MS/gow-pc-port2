# GOW-Port: rebuild the already translated runtime without repeating the ELF -> C++ step.
param([int]$Jobs = 4, [string]$ToolsExe = '')
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (-not (Test-Path -LiteralPath $vswhere)) { throw 'Visual Studio Build Tools was not found.' }
$vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vs) { throw 'MSVC x64 was not found.' }
$vcvars = Join-Path $vs 'VC\Auxiliary\Build\vcvars64.bat'
$vcvarsCmd = Join-Path $env:TEMP 'gow_runtime_vcvars.cmd'
Set-Content -LiteralPath $vcvarsCmd -Value "@call `"$vcvars`" >nul`r`n@set" -Encoding ASCII
& $env:ComSpec /c $vcvarsCmd | ForEach-Object {
    if ($_ -match '^([^=]+)=(.*)$') { Set-Item -Path "env:$($matches[1])" -Value $matches[2] }
}
function Slashes([string]$path) { return ($path -replace '\\', '/') }
$recomp = Join-Path $root 'RustPortPS2\native'
$ToolsExe = if ($ToolsExe) { $ToolsExe } else { Join-Path $root 'build-rust\release\ps2-tools.exe' }
if (-not (Test-Path -LiteralPath $ToolsExe)) {
    & cargo build --release --locked -p ps2-tools --manifest-path (Join-Path $root 'RustPortPS2\Cargo.toml') --target-dir (Join-Path $root 'build-rust')
    if ($LASTEXITCODE -ne 0) { throw 'cargo build of ps2-tools failed.' }
}
$build = Join-Path $recomp 'build-dev'
$snapshot = Join-Path $root 'build-gen\vu1-snapshot'
if (-not (Test-Path -LiteralPath $snapshot -PathType Container)) { throw 'build-gen/vu1-snapshot is missing.' }
$cmakeArgs = @('-S', $recomp, '-B', $build, '-G', 'Ninja', '-DCMAKE_BUILD_TYPE=RelWithDebInfo',
    '-DPS2X_ENABLE_RUNTIME_LOGS=ON', '-DPS2X_ENABLE_AGRESSIVE_LOGS=OFF', '-DPS2X_ENABLE_IOP_RPC_TRACE=OFF',
    '-DPS2X_BUILD_TEST=OFF',
    '-DPS2X_SHOW_WINDOWS_CONSOLE=ON', '-DPS2X_RUNNER_UNITY_BUILD_BATCH_SIZE=8',
    "-DPS2X_VU1_MICROCODE_DIR=$(Slashes $snapshot)",
    "-DPS2X_VU1_GENERATOR_SOURCE=$(Slashes (Join-Path $recomp 'tools\vu1\generate_vu1.cpp'))",
    "-DPS2X_TOOLS_EXE=$(Slashes $ToolsExe)")
& cmake @cmakeArgs
if ($LASTEXITCODE -ne 0) { throw 'The runtime CMake configuration failed.' }
& ninja -C $build "-j$Jobs" ps2EntryRunner
if ($LASTEXITCODE -ne 0) { throw 'The runtime build failed.' }
Write-Host (Join-Path $build 'ps2xRuntime\gow-2005.exe')
