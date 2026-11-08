param([switch]$Native, [switch]$Models, [switch]$Psd)
$features = @()
if ($Psd) { $features = @('--features', 'ag-psd') }
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    cargo fmt --check
    if ($LASTEXITCODE -ne 0) { throw 'Formatting failed' }
    cargo clippy --locked --all-targets --all-features -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw 'Lint failed' }
    cargo test --locked --tests @features
    if ($LASTEXITCODE -ne 0) { throw 'Tests failed' }
    cargo build --locked --release @features
    if ($LASTEXITCODE -ne 0) { throw 'Release build failed' }
    if ($Native -or $Models) {
        New-Item -ItemType Directory -Force artifacts | Out-Null
        if (!(Test-Path -LiteralPath artifacts/truck.jpg)) {
            Invoke-WebRequest 'https://raw.githubusercontent.com/facebookresearch/sam2/main/notebooks/images/truck.jpg' -OutFile artifacts/truck.jpg
        }
        if ((Get-FileHash -LiteralPath artifacts/truck.jpg -Algorithm SHA256).Hash -ne '941715E721C8864324A1425B445EA4DDE0498B995C45DDCE0141A58971C6FF99') { throw 'Fixture checksum mismatch' }
    }
    if ($Models) {
        cargo test --locked --release --test model @features -- --ignored --nocapture --test-threads=1
        if ($LASTEXITCODE -ne 0) { throw 'Real-model verification failed' }
        cargo test --locked --release --test upgraded_models @features -- --ignored --nocapture --test-threads=1
        if ($LASTEXITCODE -ne 0) { throw 'Upgraded-model verification failed' }
    }
    if ($Native) {
        & target/release/objectlift.exe probe
        if ($LASTEXITCODE -ne 0) { throw 'Native capture/clipboard verification failed' }
        & target/release/objectlift.exe --smoke artifacts/truck.jpg
        if ($LASTEXITCODE -ne 0) { throw 'Native overlay verification failed' }
        & (Join-Path $PSScriptRoot 'verify-accessibility.ps1')
    }
} finally { Pop-Location }
