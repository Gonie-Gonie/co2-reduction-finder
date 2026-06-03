use std::collections::BTreeMap;

const INFO_CSV: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/info.csv"));
const UMAP_CSV: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/Umap.csv"));

#[derive(Debug, Clone)]
pub struct BuildingMetadata {
    pub korean_name: String,
    pub model_name: String,
    pub residential: bool,
    pub gas_heating: bool,
    pub area: f64,
    pub weight: f64,
}

#[derive(Debug, Clone)]
pub struct ReferenceData {
    info: BTreeMap<String, BuildingMetadata>,
    umap_rows: usize,
}

impl ReferenceData {
    pub fn load() -> Result<Self, String> {
        let mut info = BTreeMap::new();

        for (line_index, line) in INFO_CSV.lines().enumerate() {
            if line_index == 0 || line.trim().is_empty() {
                continue;
            }

            let columns = line.split(',').collect::<Vec<_>>();
            if columns.len() != 7 {
                return Err(format!("info.csv line {} has {} columns", line_index + 1, columns.len()));
            }

            let metadata = BuildingMetadata {
                korean_name: columns[2].to_string(),
                model_name: columns[3].to_string(),
                residential: columns[1] == "주거",
                gas_heating: columns[4] == "1",
                area: columns[5].parse::<f64>().map_err(|error| error.to_string())?,
                weight: columns[6].parse::<f64>().map_err(|error| error.to_string())?,
            };
            info.insert(metadata.model_name.clone(), metadata);
        }

        let umap_rows = UMAP_CSV.lines().filter(|line| !line.trim().is_empty()).count().saturating_sub(1);

        Ok(Self { info, umap_rows })
    }

    pub fn info_len(&self) -> usize {
        self.info.len()
    }

    pub fn umap_rows(&self) -> usize {
        self.umap_rows
    }

    pub fn get(&self, model_name: &str) -> Option<&BuildingMetadata> {
        self.info.get(model_name)
    }

    pub fn model1_weight_for_base(&self, base_type: &str) -> Result<f64, String> {
        let model1_name = format!("{base_type}_1");
        let model2_name = format!("{base_type}_2");
        let model1 = self
            .get(&model1_name)
            .ok_or_else(|| format!("metadata not found: {model1_name}"))?;
        let model2 = self
            .get(&model2_name)
            .ok_or_else(|| format!("metadata not found: {model2_name}"))?;

        let sum = model1.weight + model2.weight;
        if (sum - 1.0).abs() > 0.000_001 {
            return Err(format!("{base_type} model weights do not sum to 1.0: {sum}"));
        }

        Ok(model1.weight)
    }
}

#[cfg(test)]
mod tests {
    use super::ReferenceData;

    #[test]
    fn loads_reference_metadata() {
        let data = ReferenceData::load().expect("reference metadata should load");

        assert_eq!(data.info_len(), 40);
        assert!(data.umap_rows() > 0);
        assert!(data.get("Office_1").is_some());
        assert!((data.model1_weight_for_base("Office").unwrap() - 0.522416097).abs() < 0.000_001);
    }
}
