from __future__ import annotations

import argparse
import json
import struct
import zipfile
from pathlib import Path

import h5py
import numpy as np


MAGIC = b"C2M1"
ACTIVATION = {
    "linear": 0,
    "elu": 1,
}


def load_registry(path: Path):
    registry = json.loads(path.read_text(encoding="utf-8"))
    if registry.get("schemaVersion") != 1:
        raise ValueError(f"{path}: expected schemaVersion 1")
    if registry.get("inputSpec", {}).get("dimension") <= 0:
        raise ValueError(f"{path}: inputSpec.dimension must be positive")
    if registry.get("outputSpec", {}).get("dimension") <= 0:
        raise ValueError(f"{path}: outputSpec.dimension must be positive")

    model_names = set()
    building_codes = set()
    for building in registry.get("buildingTypes", []):
        code = building["code"]
        if code in building_codes:
            raise ValueError(f"{path}: duplicate building type {code}")
        building_codes.add(code)

        models = building.get("models", [])
        if not models:
            raise ValueError(f"{path}: {code} must contain at least one model")
        weight_sum = sum(float(model["weight"]) for model in models)
        if abs(weight_sum - 1.0) > 0.000_001:
            raise ValueError(f"{path}: {code} model weights sum to {weight_sum}")

        for model in models:
            name = model["name"]
            if name in model_names:
                raise ValueError(f"{path}: duplicate model name {name}")
            model_names.add(name)
            source = path.parent / model["source"]
            if not source.exists():
                raise FileNotFoundError(f"{path}: source not found for {name}: {source}")

    if not building_codes:
        raise ValueError(f"{path}: buildingTypes must not be empty")
    return registry


def iter_datasets(h5: h5py.File):
    def visitor(name, obj):
        if isinstance(obj, h5py.Dataset):
            datasets.append(name)

    datasets: list[str] = []
    h5.visititems(visitor)
    return datasets


def normalize_activation(name: str) -> str:
    normalized = name.lower()
    if normalized not in ACTIVATION:
        raise ValueError(f"unsupported activation {name}")
    return normalized


def find_weight_dataset(h5: h5py.File, datasets: list[str], dense_name: str, suffix: str):
    matches = [
        name
        for name in datasets
        if name.startswith("model_weights/")
        and name.endswith("/" + suffix)
        and len(name.split("/")) >= 2
        and name.split("/")[-2] == dense_name
    ]
    if len(matches) != 1:
        raise ValueError(f"{dense_name}/{suffix}: expected one dataset, found {matches}")
    return np.asarray(h5[matches[0]], dtype="<f4")


def extract_hdf5(path: Path, expected_name: str | None = None):
    with h5py.File(path, "r") as h5:
        config = json.loads(h5.attrs["model_config"])
        layers = config["config"]["layers"]
        dense_layers = [
            layer["config"]
            for layer in layers
            if layer["class_name"] == "Dense"
        ]
        input_layers = [
            layer["config"]
            for layer in layers
            if layer["class_name"] == "InputLayer"
        ]
        if len(input_layers) != 1:
            raise ValueError(f"{path.name}: expected one InputLayer")

        input_shape = input_layers[0].get("batch_shape")
        input_dim = int(input_shape[-1])
        datasets = iter_datasets(h5)

        extracted_layers = []
        for layer in dense_layers:
            dense_name = layer["name"]
            activation = normalize_activation(layer["activation"])

            kernel = find_weight_dataset(h5, datasets, dense_name, "kernel")
            bias = find_weight_dataset(h5, datasets, dense_name, "bias")
            if kernel.ndim != 2 or bias.ndim != 1:
                raise ValueError(f"{path.name}: invalid Dense tensor rank for {dense_name}")
            if kernel.shape[1] != bias.shape[0]:
                raise ValueError(f"{path.name}: kernel/bias mismatch for {dense_name}")

            extracted_layers.append(
                {
                    "name": dense_name,
                    "activation": activation,
                    "kernel": np.ascontiguousarray(kernel),
                    "bias": np.ascontiguousarray(bias),
                }
            )

    output_dim = int(extracted_layers[-1]["bias"].shape[0])
    return {
        "name": expected_name or path.stem,
        "source": path.name,
        "input_dim": input_dim,
        "output_dim": output_dim,
        "layers": extracted_layers,
    }


def import_keras_loader():
    try:
        from keras.models import load_model  # type: ignore

        return load_model
    except Exception:
        try:
            from tensorflow.keras.models import load_model  # type: ignore

            return load_model
        except Exception as error:
            raise RuntimeError(
                "Keras source extraction requires `keras` or `tensorflow` to be installed"
            ) from error


def extract_with_keras_api(path: Path, expected_name: str | None = None):
    load_model = import_keras_loader()
    model = load_model(path, compile=False)
    input_shape = model.input_shape
    if isinstance(input_shape, list):
        if len(input_shape) != 1:
            raise ValueError(f"{path.name}: expected one input tensor")
        input_shape = input_shape[0]
    input_dim = int(input_shape[-1])

    extracted_layers = []
    for layer in model.layers:
        if layer.__class__.__name__ != "Dense":
            continue
        weights = layer.get_weights()
        if len(weights) != 2:
            raise ValueError(f"{path.name}: Dense layer {layer.name} must have kernel and bias")
        kernel = np.asarray(weights[0], dtype="<f4")
        bias = np.asarray(weights[1], dtype="<f4")
        activation = normalize_activation(getattr(layer.activation, "__name__", ""))
        if kernel.ndim != 2 or bias.ndim != 1 or kernel.shape[1] != bias.shape[0]:
            raise ValueError(f"{path.name}: invalid Dense tensor shape for {layer.name}")
        extracted_layers.append(
            {
                "name": layer.name,
                "activation": activation,
                "kernel": np.ascontiguousarray(kernel),
                "bias": np.ascontiguousarray(bias),
            }
        )

    if not extracted_layers:
        raise ValueError(f"{path.name}: no Dense layers found")

    return {
        "name": expected_name or path.stem,
        "source": path.name,
        "input_dim": input_dim,
        "output_dim": int(extracted_layers[-1]["bias"].shape[0]),
        "layers": extracted_layers,
    }


def extract_one(path: Path, expected_name: str | None = None):
    suffix = path.suffix.lower()
    if suffix in {".h5", ".hdf5"}:
        try:
            return extract_hdf5(path, expected_name)
        except Exception:
            return extract_with_keras_api(path, expected_name)
    if suffix == ".keras":
        if not zipfile.is_zipfile(path):
            raise ValueError(f"{path.name}: .keras source must be a zip archive")
        return extract_with_keras_api(path, expected_name)
    raise ValueError(f"{path.name}: unsupported model source extension {suffix}")


def write_u8(f, value: int):
    f.write(struct.pack("<B", value))


def write_u16(f, value: int):
    f.write(struct.pack("<H", value))


def write_u32(f, value: int):
    f.write(struct.pack("<I", value))


def write_binary(models, output_path: Path):
    with output_path.open("wb") as f:
        f.write(MAGIC)
        write_u32(f, len(models))

        for model in models:
            name = model["name"].encode("utf-8")
            write_u16(f, len(name))
            f.write(name)
            write_u32(f, model["input_dim"])
            write_u32(f, model["output_dim"])
            write_u32(f, len(model["layers"]))

            for layer in model["layers"]:
                kernel = layer["kernel"]
                bias = layer["bias"]
                write_u8(f, ACTIVATION[layer["activation"]])
                write_u32(f, int(kernel.shape[0]))
                write_u32(f, int(kernel.shape[1]))
                f.write(kernel.astype("<f4", copy=False).tobytes(order="C"))
                f.write(bias.astype("<f4", copy=False).tobytes(order="C"))


def write_manifest(models, manifest_path: Path, binary_path: Path, registry=None):
    manifest_models = []
    total_params = 0
    for model in models:
        params = 0
        layers = []
        for layer in model["layers"]:
            kernel = layer["kernel"]
            bias = layer["bias"]
            layer_params = int(kernel.size + bias.size)
            params += layer_params
            layers.append(
                {
                    "name": layer["name"],
                    "activation": layer["activation"],
                    "input_dim": int(kernel.shape[0]),
                    "output_dim": int(kernel.shape[1]),
                    "params": layer_params,
                }
            )
        total_params += params
        manifest_models.append(
            {
                "name": model["name"],
                "source": model["source"],
                "input_dim": model["input_dim"],
                "output_dim": model["output_dim"],
                "layer_count": len(layers),
                "params": params,
                "layers": layers,
            }
        )

    manifest = {
        "format": "C2M1",
        "binary": binary_path.name,
        "registry": registry_summary(registry) if registry else None,
        "model_count": len(models),
        "total_params": total_params,
        "total_f32_bytes": total_params * 4,
        "models": manifest_models,
    }
    manifest_path.write_text(json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8")


def registry_summary(registry):
    return {
        "schemaVersion": registry["schemaVersion"],
        "name": registry.get("name"),
        "inputDim": registry["inputSpec"]["dimension"],
        "outputDim": registry["outputSpec"]["dimension"],
        "buildingTypes": [
            {
                "code": building["code"],
                "label": building["label"],
                "residential": building["residential"],
                "models": [
                    {
                        "name": model["name"],
                        "source": model["source"],
                        "weight": model["weight"],
                    }
                    for model in building["models"]
                ],
            }
            for building in registry["buildingTypes"]
        ],
    }


def models_from_registry(registry_path: Path):
    registry = load_registry(registry_path)
    expected_input_dim = int(registry["inputSpec"]["dimension"])
    expected_output_dim = int(registry["outputSpec"]["dimension"])
    extracted = []
    for building in registry["buildingTypes"]:
        for model_spec in building["models"]:
            source = registry_path.parent / model_spec["source"]
            model = extract_one(source, model_spec["name"])
            if model["input_dim"] != expected_input_dim:
                raise ValueError(
                    f"{model['name']}: expected input_dim {expected_input_dim}, got {model['input_dim']}"
                )
            if model["output_dim"] != expected_output_dim:
                raise ValueError(
                    f"{model['name']}: expected output_dim {expected_output_dim}, got {model['output_dim']}"
                )
            extracted.append(model)
    return registry, extracted


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--registry", default="models/ann/v1/model_registry.json")
    parser.add_argument("--model-dir", default=None, help="legacy fallback: extract every .h5 in a directory")
    parser.add_argument("--out", default="assets/models.c2m")
    parser.add_argument("--manifest", default="assets/models_manifest.json")
    args = parser.parse_args()

    output_path = Path(args.out)
    manifest_path = Path(args.manifest)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    manifest_path.parent.mkdir(parents=True, exist_ok=True)

    registry = None
    if args.model_dir:
        model_dir = Path(args.model_dir)
        models = [extract_one(path) for path in sorted(model_dir.glob("*.h5"))]
    else:
        registry, models = models_from_registry(Path(args.registry))

    write_binary(models, output_path)
    write_manifest(models, manifest_path, output_path, registry)

    size_mb = output_path.stat().st_size / 1024 / 1024
    print(f"wrote {output_path} ({size_mb:.2f} MB), models={len(models)}")
    print(f"wrote {manifest_path}")


if __name__ == "__main__":
    main()
