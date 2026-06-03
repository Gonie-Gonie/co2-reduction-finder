# CO2 Reduction Finder

Native Windows desktop app for replacing the first `dashboard` sheet of the Excel-based CO2 reduction coefficient lookup tool.

The app should stay small by avoiding the huge Excel lookup table. Instead, it will run the existing Keras `.h5` MLP models directly in Rust after extracting compact weights.

## Baseline

- Runtime: Rust native GUI with `egui` / `eframe`
- Target OS: modern Windows desktop
- Reference data: `.reference/` only, Git ignored
- ANN policy: no ONNX; implement Dense MLP forward pass directly in Rust
- Packaging target: lightweight single `.exe`
- Tauri: intentionally excluded for now; may be reconsidered later if a web UI becomes worth the extra runtime cost

## Setup

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\setup.ps1
```

Run the app in development:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\dev.ps1
```

Check and test:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\check.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\test.ps1
```

Build release exe:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build.ps1
```

The release executable is generated at `target/release/co2-reduction-finder.exe`.

## Release

GitHub Release is created automatically from a tag push:

```powershell
git tag v0.1.0
git push origin v0.1.0
```

Details are tracked in [docs/release.md](docs/release.md).

## Notes

Project direction and reference-code analysis are tracked in [docs/memo.md](docs/memo.md) and [docs/reference-analysis.md](docs/reference-analysis.md).

