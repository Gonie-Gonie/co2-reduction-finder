# Release Process

GitHub Actions builds the Windows native executable and uploads it as a GitHub Release asset.

## Local Verification

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\check.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\test.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build.ps1
```

The release executable is generated at `target/release/co2-reduction-finder.exe`.

## Automatic Release

Push a version tag:

```powershell
git tag v0.1.6
git push origin v0.1.6
```

The workflow performs:

1. Install Rust 1.96.0.
2. Run `cargo check`.
3. Run `cargo test`.
4. Run `cargo build --release`.
5. Read `docs/release-notes/<tag>.md` when present.
6. Upload `co2-reduction-finder-<tag>-windows-x64.exe` to GitHub Release.

## Manual Release

The `release` workflow can also be run manually from GitHub Actions. Enter `0.1.6` as the `version` input to create or update release tag `v0.1.6`.

## Release Notes

Release notes live under `docs/release-notes/` by tag name. Add `docs/release-notes/vX.Y.Z.md` before pushing `vX.Y.Z`. The GitHub Release title is the tag, so notes should start with `## Changes` instead of repeating the product/version heading.

## Tauri Track

Tauri is not part of the current release path. It may be reconsidered later if a web-based UI becomes necessary.
