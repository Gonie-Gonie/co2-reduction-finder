# Model Framework

The repository now treats ANN models as official source data rather than reference-only artifacts.

## Source Layout

- `models/ann/v1/model_registry.json`: official model registry.
- `models/ann/v1/h5/*.h5`: official source models copied from the corrected reference model set.
- `assets/models.c2m`: generated compact binary consumed by the Rust app.
- `assets/models_manifest.json`: generated manifest for review and CI diffs.
- `assets/Umap.csv`: official thermal-property map used to convert era/climate/retrofit selections into model inputs.
- `assets/retrofit_costs.json`: official retrofit unit costs and indirect-cost rate factors used by cost/Pareto calculations.

The final executable embeds the compact/runtime assets under `assets/`; H5/Keras source files stay in the repository for traceability and regeneration and are not loaded by the app at runtime.

## Registry Contract

The v1 registry supports:

- multiple building types
- one or more model segments per building type
- arbitrary segment weights as long as the per-building sum is `1.0`
- fixed MLP input/output specs for the current app:
  - input dimension: `25`
  - output dimension: `2`
  - outputs: gas and electricity in `kWh/m2`

The current inference rule is weighted sample segmentation. For a building type with N model segments, the uncertain sample array is split into contiguous segments by cumulative model weight. This generalizes the original Python `_1`/`_2` split without blending prediction values.

## Input Spec

The 25 ANN inputs are ordered exactly as declared in `model_registry.json`.

Uncertainty inputs:

1. `people`
2. `equip`
3. `htgset`
4. `clgset`
5. `wwr`
6. `HW`
7. `infil`

Converted building/retrofit inputs:

8. `wall`
9. `roof`
10. `floor`
11. `winU`
12. `SHGC`
13. `cooling`
14. `heating`
15. `HX`
16. `lights`
17. `HWBoiler`
18. `coolroof`
19. `blind`
20. `PV`
21. `era`
22. `clm_0`
23. `clm_1`
24. `clm_2`
25. `clm_3`

If future models are trained on wider uncertainty ranges or a changed feature order, update the registry first and then update the Rust sampler/converter to match that schema revision.

## Reference Data Contract

Model updates should be separable from non-model reference updates:

- ANN behavior is governed by `model_registry.json`, source H5/Keras files, and generated `assets/models.c2m`.
- Thermal conversion behavior is governed by `assets/Umap.csv`.
- Cost behavior is governed by `assets/retrofit_costs.json`.

When changing model input ranges, uncertainty-variable conventions, or feature order, revise the model registry schema or its declared specs before changing Rust behavior. When changing U-value/SHGC or cost assumptions only, keep the ANN registry unchanged and update the relevant asset plus focused tests.

## Updating Models

1. Add or replace source files under `models/ann/v1/h5/` or a future source directory.
2. Update `models/ann/v1/model_registry.json`.
3. Run:

```powershell
python scripts\extract_models.py
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\check.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\test.ps1
```

The extractor validates registry weights, source existence, model dimensions, Dense layer shapes, and supported activations.

## Excel Verification Gate

Do not delete `.reference` until the app-generated values are rigorously compared with the workbook's cached dashboard/calculation values.

The verification should cover:

- baseline before values for multiple building type, climate, era, and area combinations
- retrofit after/reduction values for representative single and combined element technologies
- all dashboard metrics: electricity, gas, final energy, primary energy, and greenhouse gas
- standard deviations and uncertainty treatment
- Pareto candidate cost/reduction ordering against the workbook's saved ranges

Reference workbook parsing should avoid opening the 1GB workbook in Excel during automation. Use `scripts/extract_excel_cached_values.py` to extract cached values and formula target ranges through ZIP/XML parsing.
