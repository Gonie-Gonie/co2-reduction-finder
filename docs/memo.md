# Project Memo

Last updated: 2026-06-05

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

- `src/app.rs`: egui dashboard with dynamic comparison options, level-based ECM controls, graph sections for user alternatives, single-measure effects, and Pareto candidates, progress display, cancellation, embedded Korean font support, and live ANN-backed calculation for non-Pareto outputs.
- `src/domain/metrics.rs`: Excel-compatible metric conversion factors from `계산sheet!R5:U9` for electricity, gas, final energy demand, primary energy demand, and greenhouse gas emissions.
- `src/domain/coefficients.rs`: ANN-backed estimate path using 1000 uncertain samples, Umap conversion, registry-defined weighted model segments, energy/CO2 mean and standard deviation summaries, Python-reference retrofit costs, and metric-aware staged Pareto search.
- `src/domain/mlp.rs`: custom Dense MLP forward pass with parallel batch prediction.
- `src/domain/model_store.rs`: embedded compact model asset loader plus official model registry validation and weighted segment inference.
- `src/domain/uncertainty.rs`: empirical distribution summary and smoothed histogram data.
- `models/ann/v1/model_registry.json`: official ANN model registry, including building type model segments, weights, input/output spec, and uncertainty-variable conventions.
- `models/ann/v1/h5/*.h5`: official source Keras HDF5 models; these are not loaded by the app at runtime.
- `assets/models.c2m`: compact Dense MLP weights generated from the official model registry.
- `assets/models_manifest.json`: generated model asset metadata and registry summary.
- `assets/info.csv`: UTF-8 normalized building/model metadata from reference `info.csv`.
- `assets/Umap.csv`: UTF-8 normalized thermal-property map from reference `Umap.csv`.
- `assets/fonts/Pretendard-Regular.ttf`: bundled OFL Korean font so the single exe does not depend on system CJK font fallback.
- `scripts/setup.ps1`: repo-local Rust toolchain setup.
- `scripts/extract_models.py`: developer-side registry-driven H5/Keras to compact model asset extraction.
- `scripts/extract_reference_assets.py`: developer-side reference CSV normalization.
- `.github/workflows/ci.yml`: main push/PR checks.
- `.github/workflows/release.yml`: automatic GitHub Release for Windows exe using tag-specific release notes.
- `docs/release-notes/`: managed GitHub Release notes by tag.

## Model Asset Notes

- The corrected source H5 files now live officially in `models/ann/v1/h5`.
- The official source set has 40 H5 files and is described by `models/ann/v1/model_registry.json`.
- Extracted inference-only f32 weights are about 22.2MB.
- The corrected extracted models are all 25-input, 2-output Dense MLPs.
- Building-type predictions use registry-defined weighted sample segments. The current registry has two segments per building type and preserves the Python `get_coeff()` split strategy; the runtime supports one or more segments as long as weights sum to `1.0`.
- Current app estimates already use the corrected embedded models and registry-defined segment strategy.
- Full converted ECM lookup tables are not included because they are about 57MB each. They should be generated in Rust from compact rules/data.

## Current Limitations

- Pareto dominance currently uses point estimates from staged sample counts; uncertainty-band dominance still needs a richer sample-distribution result type.
- Pareto runs automatically for the selected building context and is reset/restarted when the building type, climate, era, or area changes.
- Each Pareto run now keeps metric-specific fronts for all dashboard metrics, so switching the selected metric should not trigger a Pareto recomputation.
- The UI is organized as top building/option inputs plus two lower tabs: `리모델링 대안 비교` and `기준 조건 분석`.
- Pareto charts should connect cost-ordered solutions, use compact P-number labels with collision avoidance, and show hover details for values and selected technologies.
- Pareto result tables should show interval efficiency as a compact horizontal bar, with technology details separated into a P-row by technology-column matrix.
- Construction year is a direct slider input and maps automatically to the backend era band; retrofit level controls omit an explicit no-treatment choice, and re-clicking the selected level turns it off.
- The graph layout is now sectioned like Excel `MAIN`, but exact visual styling can still be refined after user feedback on built exe screenshots.
- UI distribution curves currently use the computed mean/std result layer that matches Excel's dashboard plotting flow; a later refinement can expose empirical smoothed histograms from raw sample arrays.

## Excel Dashboard Notes

- The 1GB workbook was inspected through XLSX XML entries rather than opened in Excel.
- `MAIN` is small enough to inspect safely and contains final dashboard formulas and chart links.
- `계산sheet` contains the selected metric conversion table:
  - 전기: gas `0`, elec `1`, unit `[MWh]`
  - 가스: gas `1`, elec `0`, unit `[MWh]`
  - 에너지소요량: gas `1`, elec `1`, unit `[MWh]`
  - 1차에너지소요량: gas `1.1`, elec `2.75`, unit `[MWh]`
  - CO2: gas `0.20245`, elec `0.45941`, unit `[tCO2eq]`
- `MAIN` chart links confirmed:
  - distribution chart from `계산sheet!H56:DL63`
  - single-measure chart from `계산sheet!B69:J80`
  - reduction/cost scatter charts from `계산sheet!H86:J285`
- GitHub Releases show a tag and title separately. This is official GitHub behavior, but the workflow now sets the release title to the tag only and release notes start with `## Changes` to avoid duplicate-looking names.
- UI smoke captures must use DWM extended frame bounds (`scripts/smoke_capture.ps1`) instead of raw `GetWindowRect + CopyFromScreen`; Windows DPI scaling and invisible resize borders can otherwise offset the capture relative to the visible app window.

## Excel Transfer Audit

- 2026-06-05 one-time audit used workbook cached XML values, not Excel automation.
- Sampling avoided the regular DB ordering bias by taking 5 offsets (`0`, `1234`, `7777`, `15000`, `27647`) from each of the 24 climate/era blocks in `DB_INDEX`.
- Compared 6 building DB sheets: `Office`, `SingleHousing`, `School`, `Hospital`, `MultiHousing`, and `ClassA`.
- Energy/stat comparison covered 8,640 cached DB cells:
  - columns: gas/electric before, after, reduction, and sigma fields
  - max absolute difference: `0.95 kWh/m2`
  - mean absolute difference: `0.0876 kWh/m2`
  - median absolute difference: `0.03 kWh/m2`
  - 7,937 / 8,640 cells within `0.25 kWh/m2`
  - 8,640 / 8,640 cells within `1.00 kWh/m2`
- Largest differences concentrated in `School` gas sigma values around `53 kWh/m2`, consistent with sample-sequence differences between Excel's original `skopt.Lhs` run and the app's deterministic LHS-style sequence rather than a model/feature-order mismatch.
- Cost comparison covered the same 120 stratified `DB_INDEX` rows and matched exactly: 120 / 120 exact, max absolute difference `0`.
- After this audit, Rust summary statistics now keep 2 decimal places to match the Excel/Python DB export precision before UI formatting.
- Conclusion: source H5 extraction, weighted model split, thermal conversion, DB row interpretation, and retrofit cost formula are consistent enough to proceed with app-native ANN calculations as the source of truth. Keep `.reference` until a final user-facing screenshot/build review confirms no workbook-only data is still needed.

## Official References Checked

- Rust 1.96.0 release: https://blog.rust-lang.org/2026/05/28/Rust-1.96.0/
- Tauri docs were checked earlier, but Tauri is currently excluded by design.
