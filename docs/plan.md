# Development Plan

## Milestone 0: Repo Baseline

- Ignore `.reference/`.
- Add project memo and reference analysis.
- Add repo-local Rust setup scripts.
- Add native Rust GUI skeleton.
- Add calculation/progress placeholders.

## Milestone 1: Model Extraction

- Inspect H5 layout without loading the 1GB Excel workbook. Done for current `.reference` models.
- Write an extraction script that reads each Keras Dense layer weight and bias. Done in `scripts/extract_models.py`.
- Strip training-only metadata. Done: optimizer weights are excluded.
- Emit compact model artifacts for Rust inference. Done: `assets/models.c2m`.
- Include compact reference metadata. Done: `assets/info.csv` and `assets/Umap.csv`.
- Add parity tests against Keras predictions for fixed inputs.
- Confirm corrected H5/reference-code shape. Done: corrected H5 files are 25-input/2-output.
- Confirm model-pair split logic follows Python `get_coeff()`. Done in embedded model store tests.

## Milestone 2: Rust Calculation Engine

- Port Umap conversion. Done for baseline and level-based retrofit options.
- Port uncertain variable generation. Initial deterministic LHS-style 1000-sample path done.
- Port ANN MLP inference with parallel batch execution. Done for embedded corrected MLP assets.
- Port energy/CO2 statistics using mean/std from sample-level predictions. Done for gas, electricity, final energy, primary energy, and greenhouse gas metrics.
- Add Excel-compatible metric conversion factor management. Done in `src/domain/metrics.rs`.
- Add smoothed histogram/density data for UI visualization. Initial distribution curves are drawn from the same mean/std layer as Excel; empirical smoothed histograms remain a follow-up.
- Port retrofit cost calculation. Done using the Python reference formula, scaled by selected app area.
- Add focused unit tests for conversions, statistics, and cost branches.

## Milestone 3: Dashboard UI

- Inspect Excel `MAIN` dashboard layout deliberately. Done through XLSX XML without opening the 1GB workbook.
- Recreate the dashboard surface in egui. Initial graph-based section layout done.
- Support dynamic comparison options. Done.
- Support metric selection for electricity, gas, final energy, primary energy, and greenhouse gas. Done.
- Auto-refresh non-Pareto results as inputs change. Done.
- Add validation states and saved presets if useful.

## Milestone 4: Pareto Backend Jobs

- Implement long-running Pareto search as cancellable backend jobs.
- Emit progress, phase, elapsed time, and partial results.
- Use staged sampling:
  - 50-sample screening
  - uncertainty-aware candidate filtering
  - intermediate resampling for boundary candidates
  - final 1000-sample refinement
- Keep UI responsive and show result updates as they arrive.
- Initial staged Pareto backend is implemented with cancellation. The next improvement is uncertainty-band dominance instead of point-estimate dominance.
- Pareto scoring now follows the selected dashboard metric instead of always using CO2.
- Pareto is invalidated when building context, area, or metric changes, but remains available while user comparison options change.

## Milestone 5: Packaging

- Produce a Windows release `.exe`.
- Confirm actual artifact size.
- Test on a clean Windows PC.
- Reconsider Tauri only if the native GUI path becomes insufficient.

## Milestone 6: Font And Portability Polish

- Bundle an open Korean font in the executable. Done with Pretendard Regular.
- Verify dashboard Korean text on a clean Windows machine.

## Milestone 7: Release Notes

- Manage release notes by tag under `docs/release-notes/`. Done.
- Feed matching release notes into GitHub Release automation. Done.
- Use tag-only GitHub Release titles and start release note bodies with `## Changes` to avoid duplicate-looking release names. Done.
