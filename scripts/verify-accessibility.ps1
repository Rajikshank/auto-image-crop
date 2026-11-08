param([string]$Executable = 'target/release/objectlift.exe')
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    cargo build --release --example accessibility_probe
    if ($LASTEXITCODE -ne 0) { throw 'Native UI Automation probe build failed' }
    $exe = (Resolve-Path -LiteralPath $Executable).Path
    & target/release/examples/accessibility_probe.exe $exe
    if ($LASTEXITCODE -ne 0) { throw 'Native UI Automation verification failed' }
} finally { Pop-Location }
