use std::collections::BTreeMap;

use crate::domain::mlp::{Activation, DenseLayer, MlpModel};

const MODEL_BYTES: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/models.c2m"));
const MAGIC: &[u8; 4] = b"C2M1";

#[derive(Debug)]
pub struct EmbeddedModelStore {
    models: BTreeMap<String, MlpModel>,
    total_params: usize,
}

impl EmbeddedModelStore {
    pub fn load() -> Result<Self, String> {
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

        if !reader.is_done() {
            return Err(format!("{} trailing bytes in model asset", reader.remaining()));
        }

        Ok(Self {
            models,
            total_params,
        })
    }

    pub fn len(&self) -> usize {
        self.models.len()
    }

    pub fn total_params(&self) -> usize {
        self.total_params
    }

    pub fn get(&self, name: &str) -> Option<&MlpModel> {
        self.models.get(name)
    }

    pub fn predict_pair_split(
        &self,
        base_type: &str,
        model1_weight: f64,
        inputs: &[Vec<f32>],
    ) -> Result<Vec<Vec<f32>>, String> {
        let model1_name = format!("{base_type}_1");
        let model2_name = format!("{base_type}_2");
        let model1 = self
            .get(&model1_name)
            .ok_or_else(|| format!("model not found: {model1_name}"))?;
        let model2 = self
            .get(&model2_name)
            .ok_or_else(|| format!("model not found: {model2_name}"))?;

        if !(0.0..=1.0).contains(&model1_weight) {
            return Err(format!("invalid model1 weight: {model1_weight}"));
        }

        let split_index = (model1_weight * inputs.len() as f64) as usize;
        let mut outputs = Vec::with_capacity(inputs.len());
        outputs.extend(model1.predict_batch_parallel(&inputs[..split_index])?);
        outputs.extend(model2.predict_batch_parallel(&inputs[split_index..])?);
        Ok(outputs)
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
    use crate::domain::ReferenceData;

    #[test]
    fn loads_embedded_models() {
        let store = EmbeddedModelStore::load().expect("embedded model asset should load");

        assert_eq!(store.len(), 40);
        assert!(store.total_params() > 5_000_000);
        let office = store.get("Office_1").expect("Office_1 should exist");
        assert_eq!(office.input_dim, 25);
        assert_eq!(office.output_dim, 2);
    }

    #[test]
    fn pair_split_matches_python_get_coeff_model_split() {
        let store = EmbeddedModelStore::load().expect("embedded model asset should load");
        let inputs = vec![vec![0.0; 25]; 10];
        let reference_data = ReferenceData::load().expect("reference metadata should load");
        let weight = reference_data
            .model1_weight_for_base("Office")
            .expect("Office pair weight should load");
        let split_index = (weight * inputs.len() as f64) as usize;

        let mixed = store
            .predict_pair_split("Office", weight, &inputs)
            .expect("pair split prediction should succeed");
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
