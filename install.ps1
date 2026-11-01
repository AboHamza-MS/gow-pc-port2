# PC Port - Windows installer (PowerShell 5.1 or 7).
#
#   install.cmd [-Iso "C:\path\God of War.iso"] [-Destination "D:\GodOfWar"] [-InstallTools] [-NoOptimize] [-Optimize] [-Seconds 240]
#
#   -Destination   folder for the extracted disc, the generated code and the build (by default, the repository).
#
#   -RecompExe     a built ps2_recomp.exe (the Rust recompiler, RustPortPS2/crates/ps2-recomp-cli). Without it the script uses
#                  bin\ps2_recomp.exe next to itself (the launcher's kit carries it) and otherwise builds it with cargo (needs
#                  Rust, a C compiler and the MSVC environment this script already loads).
#   -ToolsExe      same for ps2-tools.exe (RustPortPS2/crates/ps2-tools: ISO extraction, ELF check, VU1/IOP generators).
#
#   -InstallTools  first installs Visual Studio 2022 Build Tools (C++, CMake and Ninja) if missing: downloads the official installer
#                  from Microsoft and asks for administrator permission. Python is not used any more; Git is not needed either:
#                  if it is missing, a portable one is downloaded into <destination>\toolchain.
#
# Steps (each one is skipped if already done):
#   1. Extracts the disc from the ISO into game\ (with its sector map, .disc_layout) and prepares game\IOP_MOD.
#   2. Builds the Rust tools (ps2_recomp, ps2-tools) and translates SCUS_973.99 to C++ with RustPortPS2\games\scus-97399\gow-port.toml.
#   3. Builds the game (gow-2005.exe, MSVC + Ninja). The launcher is a separate download (GitHub Releases): its
#      setup wizard runs this script and finds the result.
#   4. Optimization: runs the game for a few minutes with automatic key presses, saves the VU1 microcode and the IOP
#      modules it uses and rebuilds with them as native code. Only the first time; -Optimize repeats it,
#      -NoOptimize skips it.
#
# Nothing generated from the game (disc, C++ code, microcode) leaves this computer.
param(
    [string]$Iso = '',
    [switch]$InstallTools,
    [switch]$NoOptimize,
    [switch]$Optimize,
    [int]$Seconds = 240,
    [int]$Jobs = 0,
    # GOW-Port: protects normal PCs from the VU1 AOT generator retaining every capture in a long session.
    # Zero preserves an explicit developer override; the installer and launcher default to 4 GB.
    [ValidateRange(0, 64)]
    [int]$Vu1MemoryLimitGB = 4,
    # GOW-Port: automated execution (the launcher's installation wizard).
    [switch]$NoPause,
    [switch]$AllowModifiedElf,
    # GOW-Port: iteration without /GL or /LTCG; the full Release can take hours to link.
    [switch]$FastDev,
    [string]$Vu1BuildDir = '',
    # GOW-Port: folder where everything generated from the game goes (game\, build-gen\, build\, vu1_captures\).
    # Empty: inside the repository, as always. Used by the launcher's installation wizard.
    [string]$Destination = '',
    # GOW-Port: a prebuilt ps2_recomp to use instead of the kit's or the cargo build (see the header).
    [string]$RecompExe = '',
    # GOW-Port: a prebuilt ps2-tools, same rule as -RecompExe.
    [string]$ToolsExe = ''
)
$ErrorActionPreference = 'Stop'
$Root = $PSScriptRoot
$Native = Join-Path $Root 'RustPortPS2\native'
$Profile = Join-Path $Root 'RustPortPS2\games\scus-97399' # the game's recompiler configuration, function map and VU1 data
$DataRoot = $Root
if ($Destination) {
    New-Item -ItemType Directory -Force -Path $Destination | Out-Null
    $DataRoot = (Resolve-Path -LiteralPath $Destination).Path
}
$Build = if ($Destination) { Join-Path $DataRoot 'build' } else { Join-Path $Native $(if ($FastDev) { 'build-dev' } else { 'build' }) }
$Game = Join-Path $DataRoot 'game'
$Vu1 = Join-Path $DataRoot 'vu1_captures'
# GOW-Port: freeze the development AOT set; new captures do not invalidate the executable on every test.
$snapshotVu1 = Join-Path $DataRoot 'build-gen\vu1-snapshot'
$Vu1ForBuild = if ($Vu1BuildDir) { $Vu1BuildDir } elseif ($FastDev -and (Test-Path $snapshotVu1)) { $snapshotVu1 } else { $Vu1 }
if ($Vu1BuildDir -and -not (Test-Path -LiteralPath $Vu1BuildDir -PathType Container)) {
    throw "The VU1 directory does not exist: $Vu1BuildDir"
}
if ($Vu1BuildDir) { $Vu1ForBuild = (Resolve-Path -LiteralPath $Vu1BuildDir).Path }
# Build processes: one per core, but at most one per 2 GB of memory (each unit of ~8 generated
# files uses 1-2 GB with /O2 /GL). -Jobs N sets it.
if ($Jobs -le 0) {
    $memGb = [math]::Floor((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory / 1GB)
    $Jobs = [math]::Max(1, [math]::Min([Environment]::ProcessorCount, [math]::Floor($memGb / 2)))
}
function Step([string]$t) { Write-Host ''; Write-Host "==> $t" -ForegroundColor Red }
function Warn([string]$t) { Write-Host "[warning] $t" -ForegroundColor Yellow }
function Fail([string]$t) {
    Write-Host "[error] $t" -ForegroundColor Red
    if (-not $NoPause) { Read-Host 'Press Enter to exit' }
    exit 1
}
function Run([string]$exe, [string[]]$arguments) {
    & $exe @arguments
    if ($LASTEXITCODE -ne 0) { Fail "$exe ended with code $LASTEXITCODE" }
}
function Slashes([string]$p) { return ($p -replace '\\', '/') }

# --- 0. Tools -----------------------------------------------------------------------------------------------------
# GOW-Port: the installer is self-sufficient. Git, and CMake and Ninja when the system has no usable ones, are downloaded in portable
# form into toolchain\ (without installing anything on Windows or asking for administrator). Only Visual Studio Build Tools (C++) is installed, with -InstallTools,
# because Microsoft does not allow redistributing it: its official installer is downloaded and its signature is checked.
$Toolchain = Join-Path $DataRoot 'toolchain'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

function Download([string]$url, [string]$target) {
    New-Item -ItemType Directory -Force -Path (Split-Path $target) | Out-Null
    try { Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $target }
    catch { Fail "Could not download $url ($($_.Exception.Message)). Check the Internet connection." }
}
function FindVs {
    $vw = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path $vw)) { return $null }
    return (& $vw -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath)
}
function CheckSignature([string]$file, [string]$signer) {
    $f = Get-AuthenticodeSignature -LiteralPath $file
    if ($f.Status -ne 'Valid' -or $f.SignerCertificate.Subject -notmatch [regex]::Escape($signer)) {
        Fail "$(Split-Path $file -Leaf) does not have a valid signature from $signer (status: $($f.Status))."
    }
}
function GetPortableGit {
    $dir = Join-Path $Toolchain 'mingit'
    if (-not (Test-Path (Join-Path $dir 'cmd\git.exe'))) {
        $v = '2.47.1'
        $zip = Join-Path $Toolchain "MinGit-$v-64-bit.zip"
        Step "Downloading portable Git $v (about 50 MB)"
        Download "https://github.com/git-for-windows/git/releases/download/v$v.windows.1/MinGit-$v-64-bit.zip" $zip
        Expand-Archive -LiteralPath $zip -DestinationPath $dir -Force
        Remove-Item -LiteralPath $zip -Force
    }
    return (Join-Path $dir 'cmd')
}

function InstallVsBuildTools {
    Step 'Installing Visual Studio 2022 Build Tools (C++): asks for administrator permission; 15-30 min'
    $bootstrap = Join-Path $Toolchain 'vs_BuildTools.exe'
    Download 'https://aka.ms/vs/17/release/vs_BuildTools.exe' $bootstrap
    CheckSignature $bootstrap 'Microsoft Corporation'
    try {
        $p = Start-Process -FilePath $bootstrap -Verb RunAs -Wait -PassThru -ArgumentList @(
            '--wait', '--passive', '--norestart', '--add', 'Microsoft.VisualStudio.Workload.VCTools', '--includeRecommended')
    }
    catch { Fail 'Could not start the Visual Studio installer: the administrator permission must be accepted.' }
    # 3010 = installed, but Windows asks for a restart; the C++ tools can already be used.
    if ($p.ExitCode -ne 0 -and $p.ExitCode -ne 3010) { Fail "The Visual Studio installer ended with code $($p.ExitCode)." }
}
if ($InstallTools -and -not (FindVs)) { InstallVsBuildTools }

Step 'Checking tools'
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (-not (Test-Path $vswhere)) { Fail 'Visual Studio (Build Tools) is not installed. Run: install.cmd -InstallTools' }
$vs = $null
# MSVC environment (cl, link, cmake and ninja from Visual Studio) in this PowerShell session. A temporary .cmd avoids the
# quoting problems of "cmd /c" with paths containing spaces.
# vcvars64.bat looks vswhere.exe up by name, and outside a Developer Prompt its folder is not in PATH: it printed "'vswhere.exe'
# is not recognized" (in the console's code page, which showed up garbled in the launcher's log). The folder goes into PATH
# first, and stderr is discarded too: a real failure is caught by the checks for cl, cmake and ninja right below.
# GOW-Port: a function, because installing Visual Studio 2022 next to an old one (see the cmake check) reloads it.
function LoadVsEnvironment {
    $script:vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if (-not $script:vs) { Fail 'Visual Studio does not have the C++ compiler (the "Desktop development with C++" workload). Run: install.cmd -InstallTools' }
    $vcvars = Join-Path $script:vs 'VC\Auxiliary\Build\vcvars64.bat'
    $vcvarsCmd = Join-Path $env:TEMP 'PC_vcvars.cmd'
    $vsInstaller = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer'
    Set-Content -LiteralPath $vcvarsCmd -Value "@set `"PATH=$vsInstaller;%PATH%`"`r`n@call `"$vcvars`" >nul 2>&1`r`n@set" -Encoding ASCII
    cmd /c $vcvarsCmd | ForEach-Object {
        if ($_ -match '^([^=]+)=(.*)$') { Set-Item -Path "env:$($matches[1])" -Value $matches[2] }
    }
    # GOW-Port: only the compiler is required here. The newest Visual Studio may lack the "C++ CMake tools" component (a Visual
    # Studio 18 Community without it, issue #9): cmake and ninja are looked up in every installation, or installed, right below.
    if (-not (Get-Command cl -ErrorAction SilentlyContinue)) { Fail "cl not found after loading vcvars64.bat ($vcvars)." }
}
LoadVsEnvironment
# GOW-Port: the build needs a CMake 3.21 or newer with cmcldeps.exe next to it, and the first one in PATH may not be that:
# - an older one (an old Visual Studio's, Android SDK, another recompilation kit) does not understand "-S <src> -B <build>": it takes
#   the last path as the source and stops with "The source directory <build> does not exist";
# - one from another toolchain can be 3.21+ and lack cmcldeps.exe: the Ninja generator then writes the resource compiler rule (rc.exe)
#   with an empty helper and the build fails with RC1107 near its end.
# The CMake and Ninja bundled with Visual Studio are the pair Microsoft tests with the MSVC compiler used here, so they are preferred
# when usable; otherwise the first usable CMake in PATH is used, and only if there is none is something installed (-InstallTools).
function CmakeVersion([string]$cmake = 'cmake') {
    $m = [regex]::Match((& $cmake --version | Select-Object -First 1), '(\d+)\.(\d+)')
    if (-not $m.Success) { return [version]'0.0' }
    return [version]"$($m.Groups[1].Value).$($m.Groups[2].Value)"
}
function CmakeUsable([string]$cmake = 'cmake') {
    $source = (Get-Command $cmake).Source
    return ((CmakeVersion $source) -ge [version]'3.21') -and (Test-Path -LiteralPath (Join-Path (Split-Path -Parent $source) 'cmcldeps.exe'))
}
# Puts the chosen CMake first in PATH (with Visual Studio's Ninja when it is Visual Studio's CMake) and returns its path, or $null.
function UseUsableCmake {
    # GOW-Port: every Visual Studio installation, the newest first, not only the one whose vcvars64.bat was loaded: a newer one
    # without the CMake component must not hide the CMake of Visual Studio 2022 Build Tools installed next to it.
    $vsPaths = @(& $vswhere -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath | Where-Object { $_ })
    $bundledCmakes = @{}
    $candidates = @()
    foreach ($p in $vsPaths) {
        $vsCmake = Join-Path $p 'Common7\IDE\CommonExtensions\Microsoft\CMake'
        $bundled = Join-Path $vsCmake 'CMake\bin\cmake.exe'
        if (Test-Path -LiteralPath $bundled) { $candidates += $bundled; $bundledCmakes[$bundled] = $vsCmake }
    }
    $candidates += @(Get-Command cmake -All -ErrorAction SilentlyContinue | ForEach-Object { $_.Source })
    foreach ($candidate in $candidates) {
        if (CmakeUsable $candidate) {
            $env:Path = (Split-Path -Parent $candidate) + ';' + $env:Path
            if ($bundledCmakes.ContainsKey($candidate)) {
                $ninja = Join-Path $bundledCmakes[$candidate] 'Ninja'
                if (Test-Path -LiteralPath (Join-Path $ninja 'ninja.exe')) { $env:Path = $ninja + ';' + $env:Path }
            }
            if (-not (Get-Command ninja -ErrorAction SilentlyContinue)) {
                foreach ($p in $vsPaths) {
                    $ninja = Join-Path $p 'Common7\IDE\CommonExtensions\Microsoft\CMake\Ninja'
                    if (Test-Path -LiteralPath (Join-Path $ninja 'ninja.exe')) { $env:Path = $ninja + ';' + $env:Path; break }
                }
            }
            return $candidate
        }
    }
    return $null
}
$cmakeFirst = Get-Command cmake -ErrorAction SilentlyContinue
$cmakeBefore = if ($cmakeFirst) { $cmakeFirst.Source } else { 'none' }
$cmakeBeforeVersion = if ($cmakeFirst) { CmakeVersion } else { 'not installed' }
$cmakeWasUsable = [bool]$cmakeFirst -and (CmakeUsable)
$cmakeChosen = UseUsableCmake
# GOW-Port: portable CMake and Ninja (issue #11), like the portable Git: downloaded into toolchain\ from their official releases,
# SHA-256 checked, without installing anything on Windows or asking for administrator. They are used only when the system has
# no usable CMake (3.21+ with cmcldeps.exe) or no Ninja, so nobody has to add Visual Studio components or install a second
# Visual Studio just for them. The C++ compiler still comes from Visual Studio Build Tools, which Microsoft does not allow
# redistributing. A failed download is not fatal here: the other options below still apply.
function GetPortableTool([string]$name, [string]$url, [string]$sha256, [string]$zip, [string]$dir, [string]$exe) {
    if (Test-Path -LiteralPath $exe) { return $exe }
    try {
        New-Item -ItemType Directory -Force -Path $Toolchain | Out-Null
        Step "Downloading portable $name"
        Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $zip
        $actual = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash
        if ($actual -ne $sha256) { throw "SHA-256 mismatch for $name ($actual, expected $sha256)" }
        if (Test-Path -LiteralPath $dir) { Remove-Item -LiteralPath $dir -Recurse -Force }
        Expand-Archive -LiteralPath $zip -DestinationPath $dir -Force
        if (-not (Test-Path -LiteralPath $exe)) { throw "$exe is missing after extracting $name" }
        return $exe
    }
    catch {
        Warn "Could not set up portable $name ($($_.Exception.Message))."
        return $null
    }
    finally { if (Test-Path -LiteralPath $zip) { Remove-Item -LiteralPath $zip -Force -ErrorAction SilentlyContinue } }
}
function UsePortableCmake {
    $v = '3.31.6'
    $exe = GetPortableTool 'CMake' "https://github.com/Kitware/CMake/releases/download/v$v/cmake-$v-windows-x86_64.zip" `
        'd163cd3ab4959b0a53fa8988f2ddbd2e6c501658201e6a154386bad9dbe4f836' (Join-Path $Toolchain "cmake-$v.zip") `
        (Join-Path $Toolchain "cmake-$v") (Join-Path $Toolchain "cmake-$v\cmake-$v-windows-x86_64\bin\cmake.exe")
    if ($exe -and (CmakeUsable $exe)) {
        $env:Path = (Split-Path -Parent $exe) + ';' + $env:Path
        return $exe
    }
    return $null
}
function UsePortableNinja {
    $v = '1.12.1'
    $exe = GetPortableTool 'Ninja' "https://github.com/ninja-build/ninja/releases/download/v$v/ninja-win.zip" `
        'f550fec705b6d6ff58f2db3c374c2277a37691678d6aba463adcbb129108467a' (Join-Path $Toolchain "ninja-$v.zip") `
        (Join-Path $Toolchain "ninja-$v") (Join-Path $Toolchain "ninja-$v\ninja.exe")
    if ($exe) { $env:Path = (Split-Path -Parent $exe) + ';' + $env:Path }
}
if (-not $cmakeChosen) { $cmakeChosen = UsePortableCmake }
# An old Visual Studio (2017, 2019 before 16.10) bundles an old CMake. With -InstallTools (the launcher's wizard, which asked the
# user), Visual Studio 2022 is installed next to it: it becomes the newest, and the old one stays. Only if the portable CMake
# could not be downloaded.
if (-not $cmakeChosen -and $InstallTools) {
    Warn 'No usable CMake (3.21 or newer with cmcldeps.exe) was found: installing Visual Studio 2022 Build Tools next to the current Visual Studio.'
    InstallVsBuildTools
    LoadVsEnvironment
    $cmakeChosen = UseUsableCmake
}
if (-not $cmakeChosen) {
    Fail "A CMake 3.21 or newer with cmcldeps.exe next to it is required, but $cmakeBefore ($cmakeBeforeVersion) does not meet that and no other was found (Visual Studio: $vs). Visual Studio needs the ""C++ CMake tools for Windows"" component (Visual Studio Installer > Modify), an old Visual Studio (2017, 2019 before 16.10) bundles an old CMake: install Visual Studio 2022 Build Tools (install.cmd -InstallTools), or put an official CMake 3.21+ first in PATH."
}
if (-not (Get-Command ninja -ErrorAction SilentlyContinue)) { UsePortableNinja }
if (-not (Get-Command ninja -ErrorAction SilentlyContinue)) {
    Fail 'ninja was not found and the portable one could not be downloaded: check the Internet connection, add the "C++ CMake tools for Windows" component to Visual Studio (Visual Studio Installer > Modify), or put ninja in PATH.'
}
if ($cmakeChosen -ne $cmakeBefore) {
    $note = "CMake: using $cmakeChosen instead of $cmakeBefore ($cmakeBeforeVersion)."
    if ($cmakeWasUsable) { Write-Host $note } else { Warn "$note It is older than 3.21 or lacks cmcldeps.exe." }
}
# GOW-Port: the build environment in the log, to diagnose reports without asking for it.
Write-Host "Visual Studio: $vs ($((& $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property catalog_productDisplayVersion)))"
foreach ($c in @('cl', 'cmake', 'ninja')) {
    $cmd = Get-Command $c
    # cl prints its banner on stderr, which would abort under $ErrorActionPreference = 'Stop': its file version is used.
    $ver = if ($c -eq 'cl') { $cmd.Version } else { (& $c --version | Select-Object -First 1) }
    Write-Host "  ${c}: $($cmd.Source) ($ver)"
}
# GOW-Port: the first configuration downloads raylib, imgui and rlImGui with Git; with the dependencies already downloaded
# (or with Git installed) nothing is needed.
if (-not (Get-Command git -ErrorAction SilentlyContinue) -and -not (Test-Path (Join-Path $Build '_deps\raylib-src'))) {
    $env:Path = (GetPortableGit) + ';' + $env:Path
}

# GOW-Port: the Rust tools (RustPortPS2/). Order for each: its parameter, then bin\<tool>.exe (the launcher's kit), then cargo.
# Both come out of one cargo build when either is missing.
function ResolveTool([string]$given, [string]$parameter, [string]$name) {
    if ($given) {
        if (-not (Test-Path -LiteralPath $given)) { Fail "$parameter not found: $given" }
        return (Resolve-Path -LiteralPath $given).Path
    }
    $kit = Join-Path $Root "bin\$name.exe"
    if (Test-Path -LiteralPath $kit) { return $kit }
    return $null
}
$recompExe = ResolveTool $RecompExe '-RecompExe' 'ps2_recomp'
$toolsExe = ResolveTool $ToolsExe '-ToolsExe' 'ps2-tools'
if (-not $recompExe -or -not $toolsExe) {
    Step 'Building the Rust tools (ps2_recomp, ps2-tools)'
    $rustRoot = Join-Path $Root 'RustPortPS2'
    if (-not (Test-Path -LiteralPath (Join-Path $rustRoot 'third_party\rabbitizer\Cargo.toml'))) {
        Fail 'RustPortPS2\third_party\rabbitizer is missing: the repository checkout is incomplete.'
    }
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        Fail 'cargo was not found: install Rust (https://rustup.rs), or pass -RecompExe and -ToolsExe with built executables.'
    }
    $rustTarget = Join-Path $DataRoot 'build-rust'
    Run 'cargo' @('build', '--release', '--locked', '-p', 'ps2-recomp-cli', '-p', 'ps2-tools',
        '--manifest-path', (Join-Path $rustRoot 'Cargo.toml'), '--target-dir', $rustTarget)
    if (-not $recompExe) { $recompExe = Join-Path $rustTarget 'release\ps2_recomp.exe' }
    if (-not $toolsExe) { $toolsExe = Join-Path $rustTarget 'release\ps2-tools.exe' }
}
Write-Host "Using the recompiler: $recompExe"
Write-Host "Using ps2-tools: $toolsExe"

# --- 1. Game data -------------------------------------------------------------------------------------------------
Step 'Game data (game\)'
New-Item -ItemType Directory -Force -Path $Game | Out-Null
# GOW-Port: custom textures (launcher switch): PCSX2's layout, the pack goes inside textures\replacements.
New-Item -ItemType Directory -Force -Path (Join-Path $DataRoot 'textures\replacements') | Out-Null
if (-not (Test-Path (Join-Path $Game 'SCUS_973.99')) -or -not (Test-Path (Join-Path $Game '.disc_layout'))) {
    if (-not $Iso) {
        $c = Get-ChildItem -Path $Game, $Root -Filter *.iso -File -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($c) { $Iso = $c.FullName }
    }
    if (-not $Iso -or -not (Test-Path -LiteralPath $Iso)) {
        Fail 'God of War ISO (USA, SCUS-97399) not found. Put it in game\ or use: install.cmd -Iso "C:\path\God of War.iso"'
    }
    Write-Host "Extracting $Iso (about 8 GB; the game does not need the ISO afterwards)..."
    Run $toolsExe @('iso-extract', $Iso, $Game, '--layout')
}
$cnf = Join-Path $Game 'SYSTEM.CNF'
if (-not (Test-Path $cnf) -or -not (Select-String -Path $cnf -Pattern 'SCUS_973.99' -Quiet)) {
    Fail 'game\ is not God of War USA (SCUS-97399): SYSTEM.CNF does not boot SCUS_973.99.'
}
$iopMod = Join-Path $Game 'IOP_MOD'
# GOW-Port: a correct file name does not identify the executable's revision.
$verifyArgs = @('verify-elf', (Join-Path $Game 'SCUS_973.99'), '--map', (Join-Path $Profile 'funcmap.csv'))
if ($AllowModifiedElf) { $verifyArgs += '--allow-modified' }
Run $toolsExe $verifyArgs
New-Item -ItemType Directory -Force -Path $iopMod | Out-Null
Get-ChildItem -Path $Game -Filter *.IRX -File | ForEach-Object { Copy-Item $_.FullName (Join-Path $iopMod $_.Name) -Force }
Write-Host 'Done: IOP modules in game\IOP_MOD.'

# GOW-Port: the VU1 microcode is in the ELF itself. The manifest (games/scus-97399/vu1.manifest) says which piece of the
# ELF goes at each micro-memory address, and here the micro-memories are assembled without playing. That way the first build already
# carries native VU1 and the capture by playing (stage 4) is skipped. If it cannot be applied (another ELF), that capture and the
# launcher's progressive one remain.
$manifest = Join-Path $Profile 'vu1.manifest'
if ((Test-Path -LiteralPath $manifest) -and $Vu1ForBuild -eq $Vu1 -and
    -not (Get-ChildItem -Path $Vu1 -Filter *.bin -ErrorAction SilentlyContinue | Select-Object -First 1)) {
    Step 'VU1 microcode from the ELF (without playing)'
    New-Item -ItemType Directory -Force -Path $Vu1 | Out-Null
    & $toolsExe vu1-manifest apply (Join-Path $Game 'SCUS_973.99') $manifest $Vu1
    if ($LASTEXITCODE -ne 0) { Warn 'Could not apply the VU1 manifest to this ELF; the microcode will be captured by playing.' }
}
# GOW-Port: VU1 code that the ELF declares (.DVP.ovlytab) and that no capture has seen uploaded: vu1s_*.bin images
# (ps2-tools vu1-programs), whose positions the generator takes all as possible block entries. Idempotent.
if ((Test-Path -LiteralPath $manifest) -and $Vu1ForBuild -eq $Vu1 -and
    (Get-ChildItem -Path $Vu1 -Filter vu1_*.bin -ErrorAction SilentlyContinue | Select-Object -First 1)) {
    & $toolsExe vu1-programs apply (Join-Path $Game 'SCUS_973.99') $manifest $Vu1
}

# --- 2. Recompiler and game code ----------------------------------------------------------------------------------
function Configure {
    # GOW-Port: a build folder configured from another source tree (a checkout, an older kit) makes CMake stop with
    # "does not match the source". Its cache is dropped; the downloaded dependencies (_deps) are kept.
    $cache = Join-Path $Build 'CMakeCache.txt'
    if (Test-Path -LiteralPath $cache) {
        $cachedSource = Select-String -LiteralPath $cache -Pattern '^CMAKE_HOME_DIRECTORY:INTERNAL=(.*)$' | Select-Object -First 1
        if ($cachedSource -and ((Slashes $cachedSource.Matches[0].Groups[1].Value).TrimEnd('/') -ine (Slashes $Native).TrimEnd('/'))) {
            Warn "The build folder was configured from another source ($($cachedSource.Matches[0].Groups[1].Value)): reconfiguring."
            Remove-Item -LiteralPath $cache -Force
            Remove-Item -LiteralPath (Join-Path $Build 'CMakeFiles') -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
    $extra = @()
    # GOW-Port: RelWithDebInfo keeps /O2 but avoids MSVC's whole-program link optimization.
    $buildType = if ($FastDev) { 'RelWithDebInfo' } else { 'Release' }
    $runtimeLogs = if ($FastDev) { 'ON' } else { 'OFF' }
    $console = if ($FastDev) { 'ON' } else { 'OFF' }
    if (Get-ChildItem -Path $Vu1ForBuild -Filter *.bin -ErrorAction SilentlyContinue | Select-Object -First 1) {
        $extra += "-DPS2X_VU1_MICROCODE_DIR=$(Slashes $Vu1ForBuild)"
        $extra += "-DPS2X_VU1_GENERATOR_SOURCE=$(Slashes (Join-Path $Native 'tools\vu1\generate_vu1.cpp'))"
        $extra += "-DPS2X_TOOLS_EXE=$(Slashes $toolsExe)"
        $extra += "-DPS2X_VU1_MAX_MEMORY_MB=$($Vu1MemoryLimitGB * 1024)"
        # GOW-Port: CMake reads vu1n_heads.txt (dynamic VU1 entry points) next to the microcode folder; its source in the
        # repository is games/scus-97399/vu1-heads.list.
        # Without it those blocks are interpreted: ~7 million native block misses per 5 s in a fight.
        $headsSource = Join-Path $Profile 'vu1-heads.list'
        if (Test-Path -LiteralPath $headsSource) {
            Copy-Item -LiteralPath $headsSource -Destination (Join-Path (Split-Path -Parent $Vu1ForBuild) 'vu1n_heads.txt') -Force
        }
    }
    Run 'cmake' (@('-S', $Native, '-B', $Build, '-G', 'Ninja', "-DCMAKE_BUILD_TYPE=$buildType",
        "-DPS2X_ENABLE_RUNTIME_LOGS=$runtimeLogs", '-DPS2X_ENABLE_AGRESSIVE_LOGS=OFF', '-DPS2X_ENABLE_IOP_RPC_TRACE=OFF',
        '-DPS2X_BUILD_TEST=OFF', "-DPS2X_SHOW_WINDOWS_CONSOLE=$console",
        '-DPS2X_RUNNER_UNITY_BUILD_BATCH_SIZE=8') + $extra)
}
Step 'Configuring the build (the first time it downloads raylib, imgui and rlImGui)'
Configure
Step 'Translating SCUS_973.99 to C++'
# ps2_recomp does not open paths with non-ASCII characters: if the repository folder has them, work happens in C:\PCGen.
$Gen = Join-Path $DataRoot 'build-gen'
if ($Gen -match '[^\x20-\x7E]') { $Gen = Join-Path $env:SystemDrive 'PCGen' }
$out = Join-Path $Gen 'out'
New-Item -ItemType Directory -Force -Path $out | Out-Null
Copy-Item (Join-Path $Game 'SCUS_973.99') (Join-Path $Gen 'SCUS_973.99') -Force
Copy-Item (Join-Path $Profile 'funcmap.csv') (Join-Path $Gen 'funcmap.csv') -Force
(Get-Content -LiteralPath (Join-Path $Profile 'gow-port.toml') -Raw) `
    -replace '@ELF@', (Slashes (Join-Path $Gen 'SCUS_973.99')) `
    -replace '@MAP@', (Slashes (Join-Path $Gen 'funcmap.csv')) `
    -replace '@OUT@', ((Slashes $out) + '/') | Set-Content -LiteralPath (Join-Path $Gen 'recomp.toml') -Encoding ASCII
$recompLog = Join-Path $Gen 'recomp.log'
& $recompExe (Join-Path $Gen 'recomp.toml') *> $recompLog
if ($LASTEXITCODE -ne 0) { Get-Content $recompLog -Tail 20; Fail 'ps2_recomp failed (see recomp.log)' }
$runner = Join-Path $Native 'ps2xRuntime\src\runner'
New-Item -ItemType Directory -Force -Path $runner | Out-Null
function Get-Sha256([string]$path) {
    # GOW-Port: Windows PowerShell 5.1 may not expose Get-FileHash in restricted shells.
    $stream = [System.IO.File]::OpenRead($path)
    $hasher = [System.Security.Cryptography.SHA256]::Create()
    try {
        return ([System.BitConverter]::ToString($hasher.ComputeHash($stream))).Replace('-', '').ToLowerInvariant()
    }
    finally {
        $hasher.Dispose()
        $stream.Dispose()
    }
}

function CopyIfChanged([string]$source, [string]$target) {
    # Only what changed: recompilation stays incremental.
    if ((Test-Path -LiteralPath $target) -and
        (Get-Sha256 $source) -eq (Get-Sha256 $target)) { return }
    Copy-Item -LiteralPath $source -Destination $target -Force
}
Get-ChildItem -Path $out -Filter *.cpp -File | ForEach-Object { CopyIfChanged $_.FullName (Join-Path $runner $_.Name) }
Get-ChildItem -Path $out -Filter *.h -File | ForEach-Object { CopyIfChanged $_.FullName (Join-Path $Native "ps2xRuntime\include\$($_.Name)") }
# The overrides live in ps2xRuntime/src/game now (part of the CMake target); drop the copies older versions left here.
foreach ($f in @('gow_overrides.cpp', 'gow_pad2_packet.h', 'gow_gs_replay.h')) {
    Remove-Item -LiteralPath (Join-Path $runner $f) -Force -ErrorAction SilentlyContinue
}

# --- 3. Game ------------------------------------------------------------------------------------------
function BuildGame {
    Step "Building the game (the first time takes a while: ~6,400 files; VU1 RAM profile: $Vu1MemoryLimitGB GB)"
    Run 'ninja' @('-C', $Build, "-j$Jobs", 'ps2EntryRunner')
}
Configure
BuildGame
$GameExe = Join-Path $Build 'ps2xRuntime\gow-2005.exe'
# GOW-Port: bind the exact local ELF to the generated runner; the launcher checks this fingerprint.
(Get-Sha256 (Join-Path $Game 'SCUS_973.99')) |
    Set-Content -LiteralPath (Join-Path (Split-Path $GameExe) 'game-elf.sha256') -Encoding ASCII

# --- 4. Optimization (native VU1 and IOP) -------------------------------------------------------------------------
$hasVu1 = [bool](Get-ChildItem -Path $Vu1 -Filter *.bin -ErrorAction SilentlyContinue | Select-Object -First 1)
if ($hasVu1 -and -not $Optimize) { Write-Host 'VU1: micro-memories available (manifest or captures); there is no need to open the game to capture.' }
if ($Optimize -or (-not $NoOptimize -and -not $hasVu1)) {
    Step "Optimization: the game opens by itself for $Seconds s (do not touch the keyboard); the VU1 and IOP code is saved"
    $mods = Join-Path $Gen 'iop_mods'
    New-Item -ItemType Directory -Force -Path $Vu1, $mods | Out-Null
    $vars = @{
        GOW_VU1_CAPTURE = $Vu1; PS2X_IOP_DUMP_MODULES = $mods; GOW_SKIP_FMV = '1'; GOW_PAD_TEST = '1';
        GOW_PAD_TEST_NO_CAPTURE = '1'; GOW_MC0 = (Join-Path $Gen 'optimize.ps2'); PS2X_GS_WORKER = '1'; GOW_GS_FINISH_ASYNC = '1';
        GOW_PAD_SCRIPT = '5:start,12:x,20:x,28:x,40:square,41:square,42:square,44:triangle,48:lup*3,52:x,55:square,56:triangle,60:circle,64:r1,70:lleft*2,74:square,76:square,80:triangle,90:lup*4,96:x,100:square,104:l1,110:lright*2,120:square,125:triangle,140:lup*5,150:square,160:triangle,180:lup*5,200:square'
    }
    foreach ($k in $vars.Keys) { Set-Item -Path "env:$k" -Value $vars[$k] }
    $p = Start-Process -FilePath $GameExe -ArgumentList "`"$(Join-Path $Game 'SCUS_973.99')`"" -WorkingDirectory $Native -PassThru
    if (-not $p.WaitForExit($Seconds * 1000)) { Stop-Process -Id $p.Id -Force }
    foreach ($k in $vars.Keys) { Remove-Item -Path "env:$k" -ErrorAction SilentlyContinue }
    $vu1Count = (Get-ChildItem -Path $Vu1 -Filter *.bin -ErrorAction SilentlyContinue).Count
    $iopCount = (Get-ChildItem -Path $mods -Filter *.bin -ErrorAction SilentlyContinue).Count
    Write-Host "VU1: $vu1Count micro-memories; IOP: $iopCount modules."
    if ($iopCount -gt 0) { Run $toolsExe @('iop-gen', $mods, (Join-Path $Native 'ps2xIOP\src\native')) }
    Configure
    BuildGame
    (Get-Sha256 (Join-Path $Game 'SCUS_973.99')) |
        Set-Content -LiteralPath (Join-Path (Split-Path $GameExe) 'game-elf.sha256') -Encoding ASCII
}

Step 'Done'
Write-Host "  Game: $GameExe"
Write-Host '  Open the launcher and use Files > Find files automatically (or run the game from its wizard).'
Write-Host '  More native code after playing new areas: install.cmd -Optimize'
if (-not $NoPause) { Read-Host 'Press Enter to exit' }
