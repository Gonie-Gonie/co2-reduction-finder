use std::collections::BTreeMap;

use crate::domain::mlp::{Activation, DenseLayer, MlpModel};
use serde::Deserialize;

const MODEL_BYTES: &[u8] =
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/models.c2m"));
const MODEL_REGISTRY_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/models/ann/v1/model_registry.json"
));
const MAGIC: &[u8; 4] = b"C2M1";

#[derive(Debug, Clone)]
pub struct EmbeddedModelStore {
    models: BTreeMap<String, MlpModel>,
    registry: ModelRegistry,
    #[allow(dead_code)]
    total_params: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRegistry {
    schema_version: u32,
    input_spec: DimensionSpec,
    output_spec: DimensionSpec,
    building_types: Vec<RegistryBuildingType>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DimensionSpec {
    dimension: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryBuildingType {
    pub code: String,
    pub label: String,
    pub residential: bool,
    #[allow(dead_code)]
    pub gas_heating: bool,
    pub models: Vec<RegistryModelRef>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryModelRef {
    pub name: String,
    #[allow(dead_code)]
    pub source: String,
    pub weight: f64,
}

impl EmbeddedModelStore {
    pub fn load() -> Result<Self, String> {
        let registry = ModelRegistry::load()?;
        let mut reader = Reader::new(MODEL_BYTES);
        let magic = reader.read_exact(4)?;
        if magic != MAGIC {
            return Err("invalid model asset magic".to_string());
        }

        let model_count = reader.read_u32()? as usize;
        let mut models = BTreeMap::new();
        let mut total_params = 0;

        for _ in 0..model_count {
            let name_len = reader.read_u16()? as usize;
            let name = reader.read_string(name_len)?;
            let input_dim = reader.read_u32()? as usize;
            let output_dim = reader.read_u32()? as usize;
            let layer_count = reader.read_u32()? as usize;
            let mut layers = Vec::with_capacity(layer_count);

            for _ in 0..layer_count {
                let activation = match reader.read_u8()? {
                    0 => Activation::Linear,
                    1 => Activation::Elu,
                    other => return Err(format!("unsupported activation code {other}")),
                };
                let in_dim = reader.read_u32()? as usize;
                let out_dim = reader.read_u32()? as usize;
                let weights_flat = reader.read_f32_vec(in_dim * out_dim)?;
                let bias = reader.read_f32_vec(out_dim)?;
                let weights = weights_flat
                    .chunks_exact(out_dim)
                    .map(|chunk| chunk.to_vec())
                    .collect::<Vec<_>>();

                layers.push(DenseLayer {
                    weights,
                    bias,
                    activation,
                });
            }

            let model = MlpModel {
                input_dim,
                output_dim,
                layers,
            };
            total_params += model.parameter_count();

            if models.insert(name.clone(), model).is_some() {
                return Err(format!("duplicate model name {name}"));
            }
        }

        registry.validate(&models)?;

        if !reader.is_done() {
            return Err(format!(
                "{} trailing bytes in model asset",
                reader.remaining()
            ));
        }

        Ok(Self {
            models,
            registry,
            total_params,
        })
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.models.len()
    }

    #[allow(dead_code)]
    pub fn total_params(&self) -> usize {
        self.total_params
    }

    pub fn get(&self, name: &str) -> Option<&MlpModel> {
        self.models.get(name)
    }

    pub fn building_types(&self) -> &[RegistryBuildingType] {
        self.registry.building_types()
    }

    pub fn building_type(&self, code: &str) -> Option<&RegistryBuildingType> {
        self.registry.building_type(code)
    }

    pub fn predict_weighted_segments(
        &self,
        base_type: &str,
        inputs: &[Vec<f32>],
    ) -> Result<Vec<Vec<f32>>, String> {
        let building = self
            .registry
            .building_type(base_type)
            .ok_or_else(|| format!("building type not found in model registry: {base_type}"))?;
        let mut start = 0_usize;
        let mut cumulative_weight = 0.0_f64;
        let mut outputs = Vec::with_capacity(inputs.len());

        for (index, model_ref) in building.models.iter().enumerate() {
            let is_last = index + 1 == building.models.len();
            cumulative_weight += model_ref.weight;
            let end = if is_last {
                inputs.len()
            } else {
                ((cumulative_weight * inputs.len() as f64) as usize).clamp(start, inputs.len())
            };
            let model = self
                .get(&model_ref.name)
                .ok_or_else(|| format!("model not found: {}", model_ref.name))?;
            outputs.extend(model.predict_batch_parallel(&inputs[start..end])?);
            start = end;
        }

        Ok(outputs)
    }
}

impl ModelRegistry {
    fn load() -> Result<Self, String> {
        serde_json::from_str(MODEL_REGISTRY_JSON).map_err(|error| error.to_string())
    }

    fn building_types(&self) -> &[RegistryBuildingType] {
        &self.building_types
    }

    fn building_type(&self, code: &str) -> Option<&RegistryBuildingType> {
        self.building_types
            .iter()
            .find(|building| building.code == code)
    }

    fn validate(&self, models: &BTreeMap<String, MlpModel>) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err(format!(
                "unsupported model registry schemaVersion {}",
                self.schema_version
            ));
        }
        if self.input_spec.dimension == 0 || self.output_spec.dimension == 0 {
            return Err("model registry dimensions must be positive".to_string());
        }
        if self.building_types.is_empty() {
            return Err("model registry has no building types".to_string());
        }

        let mut seen_buildings = BTreeMap::new();
        let mut seen_models = BTreeMap::new();
        for building in &self.building_types {
            if seen_buildings.insert(building.code.clone(), ()).is_some() {
                return Err(format!("duplicate building type {}", building.code));
            }
            if building.models.is_empty() {
                return Err(format!("{} has no model segments", building.code));
            }

            let weight_sum = building
                .models
                .iter()
                .map(|model| model.weight)
                .sum::<f64>();
            if (weight_sum - 1.0).abs() > 0.000_001 {
                return Err(format!(
                    "{} model weights do not sum to 1.0: {weight_sum}",
                    building.code
                ));
            }

            for model_ref in &building.models {
                if !(0.0..=1.0).contains(&model_ref.weight) {
                    return Err(format!(
                        "{} has invalid model weight {}",
                        model_ref.name, model_ref.weight
                    ));
                }
                if seen_models
                    .insert(model_ref.name.clone(), building.code.clone())
                    .is_some()
                {
                    return Err(format!("duplicate model registry entry {}", model_ref.name));
                }
                let model = models
                    .get(&model_ref.name)
                    .ok_or_else(|| format!("model asset missing {}", model_ref.name))?;
                if model.input_dim != self.input_spec.dimension {
                    return Err(format!(
                        "{} input_dim mismatch: registry {}, asset {}",
                        model_ref.name, self.input_spec.dimension, model.input_dim
                    ));
                }
                if model.output_dim != self.output_spec.dimension {
                    return Err(format!(
                        "{} output_dim mismatch: registry {}, asset {}",
                        model_ref.name, self.output_spec.dimension, model.output_dim
                    ));
                }
            }
        }

        Ok(())
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.offset)
    }

    fn is_done(&self) -> bool {
        self.offset == self.bytes.len()
    }

    fn read_exact(&mut self, len: usize) -> Result<&'a [u8], String> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or_else(|| "model asset offset overflow".to_string())?;
        if end > self.bytes.len() {
            return Err("unexpected end of model asset".to_string());
        }
        let slice = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(slice)
    }

    fn read_u8(&mut self) -> Result<u8, String> {
        Ok(self.read_exact(1)?[0])
    }

    fn read_u16(&mut self) -> Result<u16, String> {
        let mut data = [0; 2];
        data.copy_from_slice(self.read_exact(2)?);
        Ok(u16::from_le_bytes(data))
    }

    fn read_u32(&mut self) -> Result<u32, String> {
        let mut data = [0; 4];
        data.copy_from_slice(self.read_exact(4)?);
        Ok(u32::from_le_bytes(data))
    }

    fn read_string(&mut self, len: usize) -> Result<String, String> {
        let bytes = self.read_exact(len)?;
        String::from_utf8(bytes.to_vec()).map_err(|error| error.to_string())
    }

    fn read_f32_vec(&mut self, len: usize) -> Result<Vec<f32>, String> {
        let bytes = self.read_exact(len * 4)?;
        let mut values = Vec::with_capacity(len);
        for chunk in bytes.chunks_exact(4) {
            let mut data = [0; 4];
            data.copy_from_slice(chunk);
            values.push(f32::from_le_bytes(data));
        }
        Ok(values)
    }
}

#[cfg(test)]
mod tests {
    use super::EmbeddedModelStore;

    #[test]
    fn loads_embedded_models_and_registry() {
        let store = EmbeddedModelStore::load().expect("embedded model asset should load");

        assert_eq!(store.len(), 40);
        assert_eq!(store.building_types().len(), 20);
        assert!(store.total_params() > 5_000_000);
        let office_registry = store
            .building_type("Office")
            .expect("Office registry entry should exist");
        assert_eq!(office_registry.models.len(), 2);
        assert!((office_registry.models[0].weight - 0.522416097).abs() < 0.000_001);

        let office = store.get("Office_1").expect("Office_1 should exist");
        assert_eq!(office.input_dim, 25);
        assert_eq!(office.output_dim, 2);
    }

    #[test]
    fn weighted_segments_match_python_two_model_split() {
        let store = EmbeddedModelStore::load().expect("embedded model asset should load");
        let inputs = vec![vec![0.0; 25]; 10];
        let weight = store
            .building_type("Office")
            .expect("Office registry entry should exist")
            .models[0]
            .weight;
        let split_index = (weight * inputs.len() as f64) as usize;

        let mixed = store
            .predict_weighted_segments("Office", &inputs)
            .expect("weighted segment prediction should succeed");
        let expected_first = store
            .get("Office_1")
            .expect("Office_1 should exist")
            .predict(&inputs[0])
            .expect("Office_1 prediction should succeed");
        let expected_after_split = store
            .get("Office_2")
            .expect("Office_2 should exist")
            .predict(&inputs[split_index])
            .expect("Office_2 prediction should succeed");

        assert_eq!(mixed.len(), inputs.len());
        assert_eq!(mixed[0], expected_first);
        assert_eq!(mixed[split_index], expected_after_split);
    }
}
