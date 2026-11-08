$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$executable = Join-Path $root 'target/release/objectlift.exe'
if (!(Test-Path -LiteralPath $executable)) { throw 'Build first: cargo build --release' }
Start-Process -FilePath $executable -WorkingDirectory $root -WindowStyle Hidden
Write-Host 'ObjectLift started in the tray. Press Ctrl+Shift+O to snip. Right-click its tray icon to exit.'
