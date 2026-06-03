# Development Plan

## Milestone 0: Repo Baseline

- Ignore `.reference/`.
- Add project memo and reference analysis.
- Add repo-local Rust setup scripts.
- Add native Rust GUI skeleton.
- Add calculation/progress placeholders.

## Milestone 1: Model Extraction

- Inspect H5 layout without loading the 1GB Excel workbook.
- Write an extraction script that reads each Keras Dense layer weight and bias.
- Strip training-only metadata.
- Emit compact model artifacts for Rust inference.
- Add parity tests against Keras predictions for fixed inputs.

## Milestone 2: Rust Calculation Engine

- Port Umap conversion.
- Port uncertain variable generation.
- Port ANN MLP inference with parallel batch execution.
- Port energy/CO2 statistics using both summary statistics and empirical sample distributions.
- Add smoothed histogram/density data for UI visualization.
- Port retrofit cost calculation.
- Add focused unit tests for conversions, statistics, and cost branches.

## Milestone 3: Dashboard UI

- Inspect Excel `dashboard` layout deliberately.
- Recreate the dashboard surface in egui.
- Support dynamic comparison options.
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

## Milestone 5: Packaging

- Produce a Windows release `.exe`.
- Confirm actual artifact size.
- Test on a clean Windows PC.
- Reconsider Tauri only if the native GUI path becomes insufficient.

