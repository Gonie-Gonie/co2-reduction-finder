from __future__ import annotations

import argparse
import json
import struct
from pathlib import Path

import h5py
import numpy as np


MAGIC = b"C2M1"
ACTIVATION = {
    "linear": 0,
    "elu": 1,
}


def iter_datasets(h5: h5py.File):
    def visitor(name, obj):
        if isinstance(obj, h5py.Dataset):
            datasets.append(name)

    datasets: list[str] = []
    h5.visititems(visitor)
    return datasets


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


def extract_one(path: Path):
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
            activation = layer["activation"]
            if activation not in ACTIVATION:
                raise ValueError(f"{path.name}: unsupported activation {activation}")

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
        "name": path.stem,
        "source": path.name,
        "input_dim": input_dim,
        "output_dim": output_dim,
        "layers": extracted_layers,
    }


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


def write_manifest(models, manifest_path: Path, binary_path: Path):
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
        "model_count": len(models),
        "total_params": total_params,
        "total_f32_bytes": total_params * 4,
        "models": manifest_models,
    }
    manifest_path.write_text(json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--model-dir", default=".reference/pyCO2module/models")
    parser.add_argument("--out", default="assets/models.c2m")
    parser.add_argument("--manifest", default="assets/models_manifest.json")
    args = parser.parse_args()

    model_dir = Path(args.model_dir)
    output_path = Path(args.out)
    manifest_path = Path(args.manifest)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    manifest_path.parent.mkdir(parents=True, exist_ok=True)

    models = [extract_one(path) for path in sorted(model_dir.glob("*.h5"))]
    write_binary(models, output_path)
    write_manifest(models, manifest_path, output_path)

    size_mb = output_path.stat().st_size / 1024 / 1024
    print(f"wrote {output_path} ({size_mb:.2f} MB), models={len(models)}")
    print(f"wrote {manifest_path}")


if __name__ == "__main__":
    main()

