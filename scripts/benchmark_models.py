"""Compare original Keras H5 models with the production Rust inference path.

Requires the existing TensorFlow/Keras environment and a release build of
``cargo build --release --example model_benchmark``. No packages are installed.
Run from any directory; output files default to ignored target/benchmark/.
"""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import platform
from statistics import NormalDist, median
import subprocess
import sys
import time


ROOT = Path(__file__).resolve().parents[1]


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2), encoding="utf-8")


def make_cases(buildings, sample_counts):
    """Synthetic scenarios: app's LHS-style uncertainty + fixed building features.

    Thermal values are a representative nonresidential Umap baseline; binary
    equipment features, era=2 and one-hot climate stay fixed within each case.
    This measures ANN work, not sampling, feature conversion or the GUI.
    """
    steps = [37, 53, 71, 91, 101, 113, 127]
    offsets = [11, 23, 31, 47, 59, 67, 83]
    normal = NormalDist()
    converted = [0.47, 0.29, 0.41, 3.84 / 6.6, 0.676,
                 0, 0, 0, 0, 0, 0, 0, 0, 2, 1, 0, 0, 0]
    cases = []
    for building in buildings:
        for count in sample_counts:
            inputs = []
            for row in range(count):
                uncertain = []
                for column, (step, offset) in enumerate(zip(steps, offsets)):
                    u = (((row * step + offset) % count) + 0.5) / count
                    uncertain.append(0.5 + normal.inv_cdf(u) / 6
                                     if column in (0, 1, 5, 6) else u)
                inputs.append(uncertain + converted)
            cases.append({"name": f"{building}_{count}",
                          "building_type": building, "inputs": inputs})
    return {"cases": cases}


def timed(function, repeats, warmup):
    for _ in range(warmup):
        function()
    durations = []
    for _ in range(repeats):
        start = time.perf_counter_ns()
        outputs = function()
        durations.append((time.perf_counter_ns() - start) / 1e6)
    return {"times_ms": durations, "median_ms": median(durations),
            "min_ms": min(durations), "max_ms": max(durations),
            "outputs": outputs.tolist()}


def python_worker(args):
    start = time.perf_counter_ns()
    import numpy as np
    import tensorflow as tf
    import keras

    import_ms = (time.perf_counter_ns() - start) / 1e6
    if args.threads:
        tf.config.threading.set_intra_op_parallelism_threads(args.threads)
        tf.config.threading.set_inter_op_parallelism_threads(args.threads)
    registry = json.loads((ROOT / "models/ann/v1/model_registry.json").read_text(encoding="utf-8"))
    buildings = {item["code"]: item for item in registry["buildingTypes"]}
    start = time.perf_counter_ns()
    models = {spec["name"]: keras.models.load_model(
        ROOT / "models/ann/v1" / spec["source"], compile=False)
        for building in buildings.values() for spec in building["models"]}
    load_ms = (time.perf_counter_ns() - start) / 1e6
    graphs = {name: tf.function(
        lambda x, model=model: model(x, training=False),
        input_signature=[tf.TensorSpec([None, registry["inputSpec"]["dimension"]], tf.float32)],
        autograph=False) for name, model in models.items()}
    cases = json.loads(args.input.read_text(encoding="utf-8"))["cases"]
    results = []
    for case in cases:
        x = np.asarray(case["inputs"], dtype=np.float32)
        specs = buildings[case["building_type"]]["models"]
        segments = []
        begin, cumulative = 0, 0.0
        for index, spec in enumerate(specs):
            cumulative += spec["weight"]
            end = len(x) if index + 1 == len(specs) else int(cumulative * len(x))
            if end > begin:
                array = x[begin:end]
                segments.append((models[spec["name"]], graphs[spec["name"]],
                                 array, tf.convert_to_tensor(array)))
            begin = end

        # Materialize every Tensor result before stopping the timer. predict()
        # includes its default batch_size=32 and its input-data adapter overhead.
        def predict():
            return np.concatenate([m.predict(a, verbose=0) for m, _, a, _ in segments])

        def eager():
            return np.concatenate([m(t, training=False).numpy() for m, _, _, t in segments])

        def graph():
            return np.concatenate([g(t).numpy() for _, g, _, t in segments])

        modes = {}
        for name, function in (("predict", predict), ("eager", eager), ("graph", graph)):
            modes[name] = timed(function, args.repeats, args.warmup)
        results.append({"name": case["name"], "samples": len(x), "modes": modes})
        print(f"Python {case['name']}: " + ", ".join(
            f"{name}={data['median_ms']:.3f} ms" for name, data in modes.items()), flush=True)
    write_json(args.output, {
        "python_version": platform.python_version(), "tensorflow_version": tf.__version__,
        "keras_version": keras.__version__, "numpy_version": np.__version__,
        "import_ms": import_ms, "load_all_models_ms": load_ms, "model_count": len(models),
        "gpu_devices": [str(device) for device in tf.config.list_physical_devices("GPU")],
        "tf_intra_op_threads_setting": tf.config.threading.get_intra_op_parallelism_threads(),
        "tf_inter_op_threads_setting": tf.config.threading.get_inter_op_parallelism_threads(),
        "cases": results})


def compare(python_result, rust_result):
    # Independent per-output checks, including both relative and absolute error.
    # Float32 summation order differs between BLAS/TensorFlow and scalar Rust.
    python_cases = {case["name"]: case for case in python_result["cases"]}
    results = []
    passed = True
    for rust_case in rust_result["cases"]:
        modes = {}
        for name, data in python_cases[rust_case["name"]]["modes"].items():
            pairs = list(zip(
                [value for row in data["outputs"] for value in row],
                [value for row in rust_case["outputs"] for value in row], strict=True))
            close = all(abs(r - p) <= 1e-3 + 1e-4 * abs(p) for p, r in pairs)
            passed = passed and close
            modes[name] = {"python_ms": data["median_ms"],
                           "python_div_rust": data["median_ms"] / rust_case["median_ms"],
                           "max_abs_error": max(abs(p - r) for p, r in pairs),
                           "max_rel_error": max(abs(p - r) / max(abs(p), 1e-6) for p, r in pairs),
                           "allclose_atol_0_001_rtol_0_0001": close}
        results.append({"name": rust_case["name"], "samples": rust_case["samples"],
                        "rust_ms": rust_case["median_ms"], "modes": modes})
    return results, passed


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--worker", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--input", type=Path, help=argparse.SUPPRESS)
    parser.add_argument("--output", type=Path, help=argparse.SUPPRESS)
    parser.add_argument("--output-dir", type=Path, default=ROOT / "target/benchmark")
    parser.add_argument("--buildings", nargs="+", default=["Univ", "Office", "SmallMultiHousing"])
    parser.add_argument("--samples", nargs="+", type=int, default=[50, 200, 1000])
    parser.add_argument("--repeats", type=int, default=9)
    parser.add_argument("--warmup", type=int, default=2)
    parser.add_argument("--threads", type=int, default=0, help="0: framework defaults; >0: fix TF and Rayon workers")
    parser.add_argument("--rust-exe", type=Path,
                        default=ROOT / "target/release/examples/model_benchmark.exe")
    args = parser.parse_args()
    if args.repeats < 1 or args.warmup < 1 or args.threads < 0 or any(n < 1 for n in args.samples):
        parser.error("repeats, warmup and samples must be positive; threads must be nonnegative")
    if args.worker:
        python_worker(args)
        return
    if not args.rust_exe.is_file():
        parser.error("Build the Rust benchmark first: cargo build --release --example model_benchmark")
    registry = json.loads((ROOT / "models/ann/v1/model_registry.json").read_text(encoding="utf-8"))
    known = {building["code"] for building in registry["buildingTypes"]}
    if set(args.buildings) - known:
        parser.error(f"Unknown buildings: {set(args.buildings) - known}")
    args.output_dir.mkdir(parents=True, exist_ok=True)
    cases_path = (args.output_dir / "inputs.json").resolve()
    python_path = (args.output_dir / "python.json").resolve()
    rust_path = (args.output_dir / "rust.json").resolve()
    write_json(cases_path, make_cases(args.buildings, args.samples))
    env = os.environ.copy()
    env.update(CUDA_VISIBLE_DEVICES="-1", TF_CPP_MIN_LOG_LEVEL="2", KERAS_BACKEND="tensorflow")
    if args.threads:
        env["RAYON_NUM_THREADS"] = str(args.threads)
    # Sequential subprocesses avoid CPU contention between the implementations.
    subprocess.run([sys.executable, str(Path(__file__).resolve()), "--worker",
                    "--input", str(cases_path), "--output", str(python_path),
                    "--repeats", str(args.repeats), "--warmup", str(args.warmup),
                    "--threads", str(args.threads)], env=env, cwd=ROOT, check=True)
    subprocess.run([str(args.rust_exe.resolve()), "--input", str(cases_path),
                    "--output", str(rust_path), "--repeats", str(args.repeats),
                    "--warmup", str(args.warmup)], env=env, cwd=ROOT, check=True)
    python_result = json.loads(python_path.read_text(encoding="utf-8"))
    rust_result = json.loads(rust_path.read_text(encoding="utf-8"))
    comparison, passed = compare(python_result, rust_result)
    result = {
        "measured_at_utc": datetime.now(timezone.utc).isoformat(),
        "platform": platform.platform(), "logical_cpus": os.cpu_count(),
        "cpu": platform.processor(), "threads_requested": args.threads,
        "repeats": args.repeats, "warmup": args.warmup,
        "input_description": "Synthetic fixed building scenario; LHS-style uncertainty; float32; weighted sample segmentation",
        "includes": "Inference + output materialization only. Excludes model load, graph tracing, input generation, GUI and postprocessing.",
        "python": {k: v for k, v in python_result.items() if k != "cases"},
        "rust": {k: v for k, v in rust_result.items() if k != "cases"},
        "all_outputs_close": passed, "comparison": comparison}
    write_json(args.output_dir / "summary.json", result)
    print("\ncase | Rust ms | Python predict ms | Python eager ms | Python graph ms")
    for case in comparison:
        print(f"{case['name']} | {case['rust_ms']:.3f} | " + " | ".join(
            f"{case['modes'][name]['python_ms']:.3f}" for name in ("predict", "eager", "graph")))
    print(f"Output parity: {passed}; results: {args.output_dir / 'summary.json'}")
    if not passed:
        raise SystemExit("Output parity check failed")


if __name__ == "__main__":
    main()
