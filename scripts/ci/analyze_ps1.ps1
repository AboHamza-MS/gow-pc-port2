# Syntactically analyzes all the repository's .ps1 files with PowerShell's parser (it does not run them).
# Usage: pwsh -NoProfile -File scripts/ci/analyze_ps1.ps1
$root = Resolve-Path (Join-Path $PSScriptRoot '..\..')
$failures = 0
$files = Get-ChildItem -Path $root -Recurse -Filter *.ps1 -File | Where-Object { $_.FullName -notmatch '[\\/]\.git[\\/]' }
foreach ($file in $files) {
    $tokens = $null
    $errors = $null
    [System.Management.Automation.Language.Parser]::ParseFile($file.FullName, [ref]$tokens, [ref]$errors) | Out-Null
    $relative = [System.IO.Path]::GetRelativePath($root, $file.FullName)
    if ($errors.Count -gt 0) {
        foreach ($e in $errors) {
            Write-Host ("::error file={0},line={1}::{2}" -f $relative, $e.Extent.StartLineNumber, $e.Message)
        }
        $failures += $errors.Count
    } else {
        Write-Host "ok  $relative"
    }
}
Write-Host ("{0} scripts, {1} errors" -f $files.Count, $failures)
if ($failures -gt 0) { exit 1 }
