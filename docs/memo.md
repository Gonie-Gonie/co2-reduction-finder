# Project Memo

Last updated: 2026-06-03

## Product Goal

- Replace the Excel lookup tool's first `dashboard` sheet with a Windows single-exe desktop app.
- Keep the app light enough for portable distribution.
- Let the user run intermediate builds and give feedback.
- Match the Excel dashboard layout as closely as practical.

## Reference Policy

- `.reference/` is local reference data only.
- `.reference/` must stay Git ignored.
- The Excel workbook is larger than 1GB and may take more than 10 minutes to open. Do not open it casually in automation.
- Move all durable knowledge from `.reference/` into docs/code before deleting any reference data.

## Architecture Principles

- Use Rust native GUI for the app.
- Current GUI stack: `egui` / `eframe`.
- Do not use Tauri for now. WebView/WebView2 adds runtime cost that works against the lightweight single-exe goal.
- Tauri can be reconsidered later in a separate track if a web UI becomes clearly worth the added runtime cost.
- Do not use ONNX. The ANN is a simple Dense MLP and should be implemented directly in Rust.
- Extract H5 Dense layer weights, biases, and activations into a compact app model format.
- Do not ship the huge expanded Excel/CSV lookup table.

## Uncertainty Handling

- The Python reference evaluates each ECM combination with 1000 uncertain building-energy samples.
- Existing Excel workflow mostly reduces that to mean/std and then uses a normal distribution assumption.
- The app should preserve empirical sample distributions where useful.
- UI charts may smooth the empirical distribution for readability, but summary statistics must be computed from unsmoothed samples.
- Parallel batch MLP inference is mandatory.

## Pareto Sampling Strategy

1. Run all candidates with about 50 samples for fast screening.
2. Remove clearly dominated candidates using uncertainty-aware comparison.
3. Re-evaluate boundary candidates with an intermediate sample size, roughly 200-300.
4. Recompute final Pareto candidates with the full 1000 samples or a user-selected precision level.
5. Expose progress phase, elapsed time, and partial results in the UI.

## Current Implementation Baseline

- `src/app.rs`: egui dashboard skeleton with dynamic comparison options, progress display, embedded Korean font support, and live ANN-backed calculation.
- `src/domain/coefficients.rs`: ANN-backed estimate path using 1000 uncertain samples, Umap conversion, Python-style `_1/_2` sample split, and CO2 summaries.
- `src/domain/mlp.rs`: custom Dense MLP forward pass with parallel batch prediction.
- `src/domain/model_store.rs`: embedded compact model asset loader.
- `src/domain/uncertainty.rs`: empirical distribution summary and smoothed histogram data.
- `assets/models.c2m`: compact Dense MLP weights extracted from corrected `.reference/data-02 annmodels/*.h5`.
- `assets/models_manifest.json`: generated model asset metadata.
- `assets/info.csv`: UTF-8 normalized building/model metadata from reference `info.csv`.
- `assets/Umap.csv`: UTF-8 normalized thermal-property map from reference `Umap.csv`.
- `assets/fonts/Pretendard-Regular.ttf`: bundled OFL Korean font so the single exe does not depend on system CJK font fallback.
- `scripts/setup.ps1`: repo-local Rust toolchain setup.
- `scripts/extract_models.py`: developer-side H5 to compact model asset extraction.
- `scripts/extract_reference_assets.py`: developer-side reference CSV normalization.
- `.github/workflows/ci.yml`: main push/PR checks.
- `.github/workflows/release.yml`: automatic GitHub Release for Windows exe.

## Model Asset Notes

- The corrected source H5 files live in `.reference/data-02 annmodels`.
- The corrected source set has 40 H5 files, matching the 40 rows in `info.csv`.
- Extracted inference-only f32 weights are about 22.2MB.
- The corrected extracted models are all 25-input, 2-output Dense MLPs.
- Building-type predictions must use the Python `get_coeff()` split strategy: `{Type}_1` predicts the first `int(weight * sample_count)` samples, and `{Type}_2` predicts the remaining samples. This is not a weighted average of prediction values.
- Current app estimates already use the corrected embedded models and the split strategy above.
- Full converted ECM lookup tables are not included because they are about 57MB each. They should be generated in Rust from compact rules/data.

## Current Limitations

- UI retrofit inputs are still boolean toggles. The Python lookup supports multi-level wall/roof/floor/window options, so the UI should expose those levels next.
- Retrofit cost is still a simplified placeholder and needs the detailed constants/branches from `post_simulation.py`.
- Pareto progress is still a staged preview job; real cancellable Pareto search is pending.

## Official References Checked

- Rust 1.96.0 release: https://blog.rust-lang.org/2026/05/28/Rust-1.96.0/
- Tauri docs were checked earlier, but Tauri is currently excluded by design.
