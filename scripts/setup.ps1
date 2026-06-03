[CmdletBinding()]
param(
    [string]$RustToolchain = "1.96.0",
    [switch]$InstallSystemPrereqs
)

$ErrorActionPreference = "Stop"

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$ToolchainRoot = Join-Path $RepoRoot ".toolchain"
$Downloads = Join-Path $ToolchainRoot "downloads"
$CargoHome = Join-Path $ToolchainRoot "cargo"
$RustupHome = Join-Path $ToolchainRoot "rustup"

New-Item -ItemType Directory -Force -Path $Downloads, $CargoHome, $RustupHome | Out-Null

function Add-LocalToolchainToPath {
    . (Join-Path $PSScriptRoot "env.ps1")
}

function Install-Rust {
    $rustupExe = Join-Path $CargoHome "bin\rustup.exe"
    $rustupInit = Join-Path $Downloads "rustup-init.exe"

    $env:CARGO_HOME = $CargoHome
    $env:RUSTUP_HOME = $RustupHome

    if (-not (Test-Path $rustupExe)) {
        Write-Host "Downloading rustup-init..."
        Invoke-WebRequest -Uri "https://win.rustup.rs/x86_64" -OutFile $rustupInit

        Write-Host "Installing Rust toolchain $RustToolchain locally..."
        & $rustupInit -y --no-modify-path --default-host x86_64-pc-windows-msvc --default-toolchain $RustToolchain
    }
    else {
        Write-Host "Rustup already installed locally."
        & $rustupExe toolchain install $RustToolchain
    }

    & $rustupExe default $RustToolchain
    & $rustupExe component add rustfmt clippy
}

function Install-SystemPrereqs {
    if (-not $InstallSystemPrereqs) {
        Write-Host "System prerequisite install skipped."
        Write-Host "If native Windows build fails, install Microsoft C++ Build Tools with the Desktop development with C++ workload."
        return
    }

    if (-not (Get-Command winget -ErrorAction SilentlyContinue)) {
        Write-Warning "winget was not found. Install Microsoft C++ Build Tools manually."
        return
    }

    Write-Host "Installing Visual Studio Build Tools through winget. This can take a while."
    winget install --id Microsoft.VisualStudio.2022.BuildTools --source winget --accept-package-agreements --accept-source-agreements --override "--quiet --wait --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
}

Install-SystemPrereqs
Install-Rust
Add-LocalToolchainToPath

Write-Host "Rust version:"
rustc --version
Write-Host "Cargo version:"
cargo --version
Write-Host "Setup completed."

