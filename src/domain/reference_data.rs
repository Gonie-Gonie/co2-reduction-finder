use std::collections::BTreeMap;

const INFO_CSV: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/info.csv"));
const UMAP_CSV: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/Umap.csv"));

#[derive(Debug, Clone)]
pub struct BuildingMetadata {
    #[allow(dead_code)]
    pub korean_name: String,
    pub model_name: String,
    pub residential: bool,
    #[allow(dead_code)]
    pub gas_heating: bool,
    #[allow(dead_code)]
    pub area: f64,
    pub weight: f64,
}

#[derive(Debug, Clone)]
pub struct ReferenceData {
    info: BTreeMap<String, BuildingMetadata>,
    umap: BTreeMap<UmapKey, UmapRow>,
    #[allow(dead_code)]
    umap_rows: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct UmapKey {
    residential: bool,
    climate: u8,
    label: String,
}

#[derive(Debug, Clone)]
pub struct ThermalProperties {
    pub wall: f32,
    pub roof: f32,
    pub floor: f32,
    pub win_u_scaled: f32,
    pub shgc: f32,
}

#[derive(Debug, Clone)]
struct UmapRow {
    wall: Option<f32>,
    roof: Option<f32>,
    floor: Option<f32>,
    win_u: Option<f32>,
    shgc: Option<f32>,
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
                return Err(format!(
                    "info.csv line {} has {} columns",
                    line_index + 1,
                    columns.len()
                ));
            }

            let metadata = BuildingMetadata {
                korean_name: columns[2].to_string(),
                model_name: columns[3].to_string(),
                residential: columns[1] == "주거",
                gas_heating: columns[4] == "1",
                area: columns[5]
                    .parse::<f64>()
                    .map_err(|error| error.to_string())?,
                weight: columns[6]
                    .parse::<f64>()
                    .map_err(|error| error.to_string())?,
            };
            info.insert(metadata.model_name.clone(), metadata);
        }

        let mut umap = BTreeMap::new();
        for (line_index, line) in UMAP_CSV.lines().enumerate() {
            if line_index == 0 || line.trim().is_empty() {
                continue;
            }

            let columns = line.split(',').collect::<Vec<_>>();
            if columns.len() != 10 {
                return Err(format!(
                    "Umap.csv line {} has {} columns",
                    line_index + 1,
                    columns.len()
                ));
            }

            let key = UmapKey {
                residential: columns[0] == "주거",
                climate: columns[2]
                    .parse::<u8>()
                    .map_err(|error| error.to_string())?,
                label: columns[4].to_string(),
            };
            let row = UmapRow {
                wall: parse_optional_f32(columns[5])?,
                roof: parse_optional_f32(columns[6])?,
                floor: parse_optional_f32(columns[7])?,
                win_u: parse_optional_f32(columns[8])?,
                shgc: parse_optional_f32(columns[9])?,
            };
            umap.insert(key, row);
        }

        let umap_rows = umap.len();

        Ok(Self {
            info,
            umap,
            umap_rows,
        })
    }

    #[allow(dead_code)]
    pub fn info_len(&self) -> usize {
        self.info.len()
    }

    #[allow(dead_code)]
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
            return Err(format!(
                "{base_type} model weights do not sum to 1.0: {sum}"
            ));
        }

        Ok(model1.weight)
    }

    pub fn thermal_properties(
        &self,
        residential: bool,
        climate: u8,
        era: u8,
        wall_option: u8,
        roof_option: u8,
        floor_option: u8,
        window_option: u8,
    ) -> Result<ThermalProperties, String> {
        let era_label = era.to_string();
        let wall_label = retrofit_label("wall", wall_option, &era_label);
        let roof_label = retrofit_label("wall", roof_option, &era_label);
        let floor_label = retrofit_label("wall", floor_option, &era_label);
        let window_label = retrofit_label("win", window_option, &era_label);

        let wall = self
            .lookup_umap(residential, climate, &wall_label)?
            .wall
            .ok_or_else(|| format!("wall value missing for {wall_label}"))?;
        let roof = self
            .lookup_umap(residential, climate, &roof_label)?
            .roof
            .ok_or_else(|| format!("roof value missing for {roof_label}"))?;
        let floor = self
            .lookup_umap(residential, climate, &floor_label)?
            .floor
            .ok_or_else(|| format!("floor value missing for {floor_label}"))?;
        let window = self.lookup_umap(residential, climate, &window_label)?;
        let win_u = window
            .win_u
            .ok_or_else(|| format!("window U value missing for {window_label}"))?;
        let shgc = window
            .shgc
            .ok_or_else(|| format!("window SHGC missing for {window_label}"))?;

        Ok(ThermalProperties {
            wall,
            roof,
            floor,
            win_u_scaled: win_u / 6.6,
            shgc,
        })
    }

    fn lookup_umap(&self, residential: bool, climate: u8, label: &str) -> Result<&UmapRow, String> {
        let key = UmapKey {
            residential,
            climate,
            label: label.to_string(),
        };
        self.umap.get(&key).ok_or_else(|| {
            format!(
                "Umap row not found: residential={residential}, climate={climate}, label={label}"
            )
        })
    }
}

fn retrofit_label(prefix: &str, option: u8, era_label: &str) -> String {
    if option == 0 {
        era_label.to_string()
    } else {
        format!("{prefix}{option}")
    }
}

fn parse_optional_f32(value: &str) -> Result<Option<f32>, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("#N/A") {
        return Ok(None);
    }

    trimmed
        .parse::<f32>()
        .map(Some)
        .map_err(|error| error.to_string())
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

        let thermal = data
            .thermal_properties(false, 0, 0, 0, 0, 0, 0)
            .expect("thermal properties should load");
        assert!((thermal.wall - 0.582).abs() < 0.000_001);
        assert!((thermal.win_u_scaled - (3.489 / 6.6)).abs() < 0.000_001);
    }
}
