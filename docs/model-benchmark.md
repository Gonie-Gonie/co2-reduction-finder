# Python/Keras and Rust model benchmark

Measured on 2026-10-20. This is a practical local comparison, not a controlled research benchmark or a claim about Python and Rust in general.

The installed Python/TensorFlow/Keras runtime plus the original 40 models occupies **1,580.48 MiB (1.54 GiB)**. The complete Rust GUI EXE, including all models and application assets, is **30.35 MiB**, approximately **52.1 times smaller (98.08% less)** under the counting rules below.

Rust was faster than Keras `predict()` in all measured cases. Against warmed TensorFlow graph execution, Rust was faster for the small and medium models; TensorFlow was faster for the largest 1,000-sample case. The large `predict()` gap therefore should not be presented as the arithmetic-engine speedup alone.

## Timing results

Milliseconds per scenario, median of nine repetitions after two warmups; loading excluded. `SmallMultiHousing` includes a 1,051,052-parameter model, versus roughly 18,000 parameters per `Univ` model and 39,000-49,000 per `Office` model.

| Building | Samples | Python `predict` | Python eager | Python graph | Rust |
| --- | ---: | ---: | ---: | ---: | ---: |
| Univ | 50 | 176.685 | 5.099 | 1.663 | 0.116 |
| Univ | 200 | 178.145 | 5.282 | 1.247 | 0.492 |
| Univ | 1,000 | 208.192 | 6.757 | 2.048 | 1.331 |
| Office | 50 | 174.396 | 6.202 | 1.700 | 0.344 |
| Office | 200 | 175.532 | 9.182 | 2.062 | 0.750 |
| Office | 1,000 | 212.019 | 10.414 | 3.595 | 2.037 |
| SmallMultiHousing | 50 | 187.839 | 12.798 | 4.534 | 1.450 |
| SmallMultiHousing | 200 | 181.037 | 16.357 | 5.530 | 3.557 |
| SmallMultiHousing | 1,000 | 261.330 | 31.222 | 11.610 | 16.320 |

At 1,000 samples, Rust saves approximately 207 ms / 210 ms / 245 ms against `predict()` for Univ / Office / SmallMultiHousing (156 / 104 / 16 times as fast). Against graph execution it saves 0.72 ms and 1.56 ms for Univ and Office; the large SmallMultiHousing graph run saves **4.71 ms versus Rust**, or approximately 1.41 times as fast.

The compared outputs passed `abs(rust - python) <= 0.001 + 0.0001 * abs(python)` for every output, sample count and Python mode. Maximum observed absolute difference was **0.00009174 kWh/m2**; maximum relative difference was **0.000001065**. This checks the six representative models, not every H5 model or the application's full energy-calculation pipeline.

Single observations of initialization, reported separately from the repeated inference timings:

| Operation | Time |
| --- | ---: |
| Python NumPy/TensorFlow/Keras imports | 10,192.82 ms |
| Python loading all 40 H5 models (`compile=False`) | 2,941.35 ms |
| Rust loading all 40 embedded models and registry | 32.52 ms |

These are library/model initialization observations, not a repeated cold-start benchmark or full GUI startup times.

## Installed-file size

1 MiB = 1,048,576 bytes. These values describe the installed environment's declared dependencies, not a proven smallest possible inference distribution.

| Component | Bytes | MiB |
| --- | ---: | ---: |
| Python interpreter and standard library | 66,439,918 | 63.36 |
| TensorFlow, Keras and required packages (38 distributions total) | 1,519,584,848 | 1,449.19 |
| Original H5 models (40 files) | 71,201,072 | 67.90 |
| Python benchmark script and model registry | 29,827 | 0.03 |
| **Python total** | **1,657,255,665** | **1,580.48** |
| **Rust GUI EXE, with models, font and reference data** | **31,822,848** | **30.35** |
| Rust inference benchmark EXE, with models | 23,796,224 | 22.69 |
| Converted model binary alone (already embedded in both EXEs) | 23,262,546 | 22.19 |

TensorFlow alone contributes 1,334,458,460 bytes (1,272.64 MiB); Keras contributes 6,123,792 bytes (5.84 MiB). The dependency total also includes TensorFlow's declared dependencies such as libclang and TensorBoard. This is ordinary package installation accounting; an inference-specific packaging/pruning exercise could reduce it.

Both Rust EXEs were checked for imported DLLs and require only Windows system DLLs; there is no extra `libgcc` or `libwinpthread` file to add. H5 source files contain `model_weights` and `optimizer_weights`; the compact Rust asset retains inference parameters without optimizer state. Some of the model-file size reduction comes from removing that training state, not from Rust itself.

The checked-in [measurement snapshot](benchmarks/2026-10-20.json) preserves all repetition timings, comparison errors, versions and per-package size totals. Full predictions and the detailed installed-file inventory remain under `target/benchmark/` on this machine.

## Method

- Windows 11 Education 10.0.26300, AMD Ryzen 9 3900X (12 physical / 24 logical cores), approximately 64 GiB RAM.
- Python 3.12.7, TensorFlow 2.20.0, Keras 3.13.2, NumPy 2.3.1; CPU only, `KERAS_BACKEND=tensorflow`, `CUDA_VISIBLE_DEVICES=-1`.
- Rust 1.96.0, Cargo 1.96.0, release profile from `Cargo.toml` (strip, thin LTO, one codegen unit, panic abort).
- This PC lacks the MSVC linker/Windows SDK. These measurements use the installed `x86_64-pc-windows-gnu` target with the bundled LLVM linker and MinGW assembler/import-library tools. Its EXE size and speed may differ from the usual MSVC release.
- Framework defaults select CPU parallelism: Rayon uses 24 workers; TensorFlow intra/inter-op settings are both 0 (automatic).
- Three representative building types: small `Univ`, medium `Office`, and large `SmallMultiHousing`. Each uses its registry's original two H5 models and exact weighted sample segmentation. These are not averages across all 40 models.
- Batch sizes 50, 200, and 1,000 correspond to the application's screening, intermediate, and final sample counts. Each number is one scenario's inference; a before/after comparison calls inference twice.
- Shared JSON inputs become float32 on both sides. Inputs use the application's LHS-style uncertainty sequence, a fixed synthetic era-2/climate-0 building row, and zero retrofit switches. The thermal row is a nonresidential Umap baseline, including for the residential test: this is an inference workload, not an energy-performance validation.
- Two warmups precede nine timed repetitions. Tables use the median. The two implementations run in separate, sequential processes, after compilation finishes.
- Timers include inference and materialized CPU outputs, and exclude model loading, input preparation, GUI, CO2/statistical calculations, graph tracing and serialization. Python receives prepared tensors/slices; Rust calls the production method including model lookup/splitting and output assembly.

Python execution modes:

| Mode | Meaning |
| --- | --- |
| `predict` | `keras.models.load_model(h5, compile=False)` followed by `model.predict(array, verbose=0)`, default batch size 32. Includes the Keras data adapter and sub-batch overhead. |
| `eager` | `model(tensor, training=False).numpy()`, once for each model segment. |
| `graph` | The same forward pass in a warmed `tf.function`, with a dynamic batch dimension, followed by `.numpy()`. No XLA. |
| Rust | The application's unchanged `EmbeddedModelStore::predict_weighted_segments` / `MlpModel::predict_batch_parallel`. |

The original Python calling scripts are absent from this checkout. The `predict` measurement represents a common Keras API, not a verified reproduction of the original application's exact calling code. The graph measurement makes a comparison against a lower-overhead TensorFlow path possible.

## Reproduce

With a working default Windows Rust toolchain and existing compatible Python packages:

```powershell
cargo build --release --bin co2-reduction-finder --example model_benchmark
python scripts/benchmark_models.py
python scripts/measure_runtime_size.py --output target/benchmark/runtime_size.json > $null
```

No benchmark script installs packages. Required Python distributions are `tensorflow`, `keras`, `numpy`, `h5py`, and `packaging`, with their dependencies.

The GNU build actually used on this PC:

```powershell
$gnuToolchain = Join-Path $env:USERPROFILE '.rustup/toolchains/1.96.0-x86_64-pc-windows-gnu'
$gnuSelfContained = Join-Path $gnuToolchain 'lib/rustlib/x86_64-pc-windows-gnu/bin/self-contained'
$env:PATH = 'C:\msys64\mingw64\bin;' + $gnuSelfContained + ';' + $env:PATH
$env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = Join-Path $gnuToolchain 'lib/rustlib/x86_64-pc-windows-gnu/bin/rust-lld.exe'
$env:RUSTFLAGS = '-C linker-flavor=ld.lld -C link-self-contained=yes'
cargo +1.96.0-x86_64-pc-windows-gnu build --release --target x86_64-pc-windows-gnu --example model_benchmark
New-Item -ItemType Directory -Path target/tooling -Force | Out-Null
@('LIBRARY SHLWAPI.dll', 'EXPORTS', 'AssocQueryStringW') | Set-Content -LiteralPath target/tooling/shlwapi.def -Encoding ascii
& 'C:\msys64\mingw64\bin\dlltool.exe' -D SHLWAPI.dll -d target/tooling/shlwapi.def -l target/tooling/libshlwapi.a -m i386:x86-64
cargo +1.96.0-x86_64-pc-windows-gnu rustc --release --target x86_64-pc-windows-gnu --bin co2-reduction-finder -- -L native=target/tooling
python scripts/benchmark_models.py --rust-exe target/x86_64-pc-windows-gnu/release/examples/model_benchmark.exe
python scripts/measure_runtime_size.py --rust-target x86_64-pc-windows-gnu --output target/benchmark/runtime_size.json > $null
```

The small `libshlwapi.a` workaround supplies an import entry for the existing Windows `SHLWAPI.dll`; the bundled GNU toolchain lacks that import library. It does not add a runtime library or alter application code.

Raw inputs, per-repetition timings and predictions are written to ignored `target/benchmark/inputs.json`, `python.json`, and `rust.json`; the combined comparison is `summary.json`. `--threads N` fixes TensorFlow intra/inter-op and Rayon worker settings for an optional controlled comparison. `--buildings`, `--samples`, `--warmup`, `--repeats`, and `--output-dir` allow smaller runs or other registered models.

The size script reads installed distribution `RECORD` metadata and recursively follows active `Requires-Dist` entries, validates versions, excludes extras and unrelated installed packages, and counts each real path once. It includes native DLLs, package metadata, and entry-point scripts. Python's interpreter DLLs/executables, `DLLs/`, and `Lib/` are counted separately; `site-packages` is supplied by the dependency inventory. Generated bytecode, external build toolchains, pip caches, and shared OS DLLs are excluded. The full standard library, including tests and optional modules, is retained.

This is an uncompressed installed-file estimate, not a download size, RAM measurement, PyInstaller build, or aggressively trimmed minimum. In particular, TensorFlow's installed package includes files beyond those an inference-only packager might retain. Rust embeds converted model data and application assets, so its model binary must not be added to the EXE size a second time.
