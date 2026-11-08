# Build and verify ObjectLift

Use Windows 11 x64, Rust with the MSVC target, and Visual Studio Build Tools with the Desktop development with C++ workload and Windows SDK. The SDK resource compiler embeds the icon and version metadata.

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/setup-models.ps1
cargo build --locked --release --features ag-psd
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/Start-ObjectLift.ps1
```

The release profile uses thin LTO, a single code generation unit, and strips debug information. Cargo's feature flag enables experimental PSD export. Run `target/release/objectlift.exe --version` to confirm the build version.

For optional advanced inference, install Python 3.12 and run `scripts/setup-upgrade-models.ps1` with that Python executable. This development path downloads checkpoints, generates ONNX assets, and verifies them against the model lock. It can take several gigabytes. The app itself has no Python requirement.

## Checks

Close other ObjectLift instances before native tests so the test app can register the hotkey. Save or clear any sensitive clipboard contents before running clipboard checks. The clipboard probe restores supported original data.

```powershell
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-features --tests
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/verify.ps1 -Native -Models -Psd
```

Model checks need local models and the pinned truck fixture. The optional upgraded-model tests need the advanced model assets. Native checks run the capture, clipboard, overlay, export, and Windows UI Automation probes. A fixture inserts a nested candidate during the click and delivers a stale model result. The selected mask must stay identical to the displayed mask, and the PNG, WebP, and ZIP outputs must agree with Inspect.

Output from local tests lives under `artifacts/`. The release archive is assembled by `scripts/package-release.ps1` from an explicit file list. It includes the application, setup and launch scripts, and license notices.
