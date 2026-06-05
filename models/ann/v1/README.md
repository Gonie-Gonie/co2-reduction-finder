# ANN Model Registry v1

This directory is the official source model set for the app.

- `model_registry.json` is the source of truth for building types, model segment weights, input/output dimensions, and uncertainty-variable conventions.
- `h5/*.h5` contains the original Keras HDF5 source models.
- The app does not load these H5 files at runtime. Run `python scripts/extract_models.py` to convert this registry into the compact embedded assets under `assets/`.

Model update rules:

- Each building type may have one or more model segments.
- Segment weights for one building type must sum to `1.0`.
- All models in this registry must follow the declared input/output spec unless the registry schema is intentionally revised.
- HDF5 `.h5` is supported directly. Keras `.keras` sources are supported by the extraction script when a compatible `keras` or `tensorflow` Python package is installed.
