param([string]$Destination)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$version = (Get-Content -LiteralPath (Join-Path $root 'Cargo.toml') |
    Select-String '^version = "([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
if (!$Destination) { $Destination = Join-Path $root "dist/ObjectLift-$version-windows-x64" }
if (Test-Path -LiteralPath $Destination) { throw 'Destination already exists. Choose an empty release location.' }
$exe = Join-Path $root 'target/release/objectlift.exe'
if ((& $exe --version) -ne "ObjectLift $version" -or $LASTEXITCODE -ne 0) {
    throw 'Build the matching release executable first.'
}
New-Item -ItemType Directory -Path $Destination | Out-Null
New-Item -ItemType Directory -Path (Join-Path $Destination 'scripts') | Out-Null
Copy-Item -LiteralPath $exe -Destination $Destination
foreach ($file in @('LICENSE','THIRD-PARTY-NOTICES.md','DEPENDENCY-LICENSES.txt')) {
    Copy-Item -LiteralPath (Join-Path $root $file) -Destination $Destination
}
Copy-Item -LiteralPath (Join-Path $root 'docs/RELEASE-NOTES.md') -Destination (Join-Path $Destination 'README.md')
Copy-Item -LiteralPath (Join-Path $root 'scripts/setup-models.ps1') -Destination (Join-Path $Destination 'scripts')
@'
@echo off
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\setup-models.ps1"
if errorlevel 1 (
  echo Setup failed. Check the error above and try again.
  pause
  exit /b 1
)
echo Setup finished. Run Start-ObjectLift.cmd.
pause
'@ | Set-Content -LiteralPath (Join-Path $Destination 'Setup-ObjectLift.cmd') -Encoding ascii
@'
@echo off
setlocal
set "OBJECTLIFT_LAUNCH_DIR=%~dp0"
if not exist "%~dp0models\runtime\runtimes\win-x64\native\onnxruntime.dll" (
  echo Run Setup-ObjectLift.cmd first.
  pause
  exit /b 1
)
powershell.exe -NoProfile -WindowStyle Hidden -Command "Start-Process -FilePath ([IO.Path]::Combine($env:OBJECTLIFT_LAUNCH_DIR, 'objectlift.exe')) -WorkingDirectory $env:OBJECTLIFT_LAUNCH_DIR -WindowStyle Hidden"
'@ | Set-Content -LiteralPath (Join-Path $Destination 'Start-ObjectLift.cmd') -Encoding ascii
Write-Host "Release folder prepared: $Destination"
