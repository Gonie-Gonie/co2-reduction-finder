[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "env.ps1")

Push-Location (Resolve-Path (Join-Path $PSScriptRoot ".."))
try {
    cargo test
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
finally {
    Pop-Location
}
