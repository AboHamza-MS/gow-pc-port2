# Tests of the pure decisions of install.ps1 (the build job count) without running an installation: the functions are taken
# out of the script with the PowerShell parser and defined here, so nothing else in it runs. Works in PowerShell 5.1 and 7.
# Usage: pwsh -NoProfile -File scripts/ci/test-install-ps1.ps1
$ErrorActionPreference = 'Stop'
$script = Join-Path $PSScriptRoot '..\..\install.ps1'
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $script).Path, [ref]$null, [ref]$null)
$failures = 0

# The text of a function of install.ps1; it is defined here with a dot-source at script level (a function's own scope would lose it).
function Get-ScriptFunction([string]$name) {
    $f = $ast.Find({ param($n) $n -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $name }, $true)
    if (-not $f) { throw "install.ps1 has no function $name" }
    return $f.Extent.Text
}
function Expect([string]$what, $expected, $actual) {
    if ($expected -ceq $actual) { Write-Host "  ok   $what" }
    else { Write-Host "  FAIL $what (expected '$expected', got '$actual')"; $script:failures++ }
}

. ([scriptblock]::Create((Get-ScriptFunction 'Get-BuildJobs')))
Write-Host 'build jobs: cores, RAM in GB -> jobs'
# (cores, RAM, expected): the reserve is 2 from 8 logical processors, 1 from 4, 0 below; one job per 2 GB; never below 1.
$jobCases = @(
    @(16, 32, 14), @(8, 32, 6), @(6, 16, 5), @(4, 16, 3), @(2, 16, 2), @(16, 8, 4), @(1, 4, 1),
    @(12, 16, 8), @(7, 64, 6), @(3, 64, 3), @(4, 2, 1), @(8, 1, 1), @(32, 128, 30), @(2, 2, 1)
)
foreach ($c in $jobCases) { Expect "$($c[0]) cores, $($c[1]) GB" $c[2] (Get-BuildJobs $c[0] $c[1]) }

if ($failures -gt 0) { Write-Host "$failures test(s) failed"; exit 1 }
Write-Host 'all passed'
