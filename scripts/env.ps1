[CmdletBinding()]
param()

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$ToolchainRoot = Join-Path $RepoRoot ".toolchain"
$CargoHome = Join-Path $ToolchainRoot "cargo"
$RustupHome = Join-Path $ToolchainRoot "rustup"

$env:CARGO_HOME = $CargoHome
$env:RUSTUP_HOME = $RustupHome

$pathEntries = New-Object System.Collections.Generic.List[string]

$CargoBin = Join-Path $CargoHome "bin"
if (Test-Path $CargoBin) {
    $pathEntries.Add($CargoBin)
}

$pathEntries.Add($env:PATH)
$env:PATH = ($pathEntries | Where-Object { $_ -and $_.Trim().Length -gt 0 }) -join [System.IO.Path]::PathSeparator

Write-Host "Repo root: $RepoRoot"
Write-Host "Cargo home: $CargoHome"
Write-Host "Rustup home: $RustupHome"
