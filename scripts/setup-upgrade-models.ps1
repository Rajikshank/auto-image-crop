param([string]$Python = 'python')
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    if (!(Test-Path -LiteralPath '.tools/ml/Scripts/python.exe')) {
        & $Python -c "import sys; assert sys.version_info[:2] == (3,12), 'Use Python 3.12 for the pinned exporter environment'"
        if ($LASTEXITCODE -ne 0) { throw 'Select Python 3.12 using -Python <path-to-python.exe>' }
        & $Python -m venv .tools/ml
        if ($LASTEXITCODE -ne 0) { throw 'Python 3.12 venv creation failed' }
    }
    $mlPython = Join-Path $root '.tools/ml/Scripts/python.exe'
    & $mlPython -m pip install -r scripts/upgrade-requirements.txt
    if ($LASTEXITCODE -ne 0) { throw 'Development dependencies failed' }
    & $mlPython scripts/fetch-upgrade-models.py
    if ($LASTEXITCODE -ne 0) { throw 'Checkpoint download failed' }
    & $mlPython scripts/export-upgrade-models.py
    if ($LASTEXITCODE -ne 0) { throw 'Source export or parity check failed' }
    & $mlPython scripts/export-interactive-decoder.py
    if ($LASTEXITCODE -ne 0) { throw 'Iterative decoder export or parity check failed' }
    & $mlPython scripts/verify-upgrade-assets.py
    if ($LASTEXITCODE -ne 0) { throw 'Pinned export verification failed' }
} finally { Pop-Location }
