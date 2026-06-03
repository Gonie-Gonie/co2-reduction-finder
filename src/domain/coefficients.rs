use std::{collections::BTreeSet, thread, time::Duration};

use super::{model_store::EmbeddedModelStore, reference_data::ReferenceData};

const DEFAULT_SAMPLE_COUNT: usize = 1000;
const CO2_ELEC: f64 = 0.45941;
const CO2_GAS: f64 = 0.20245;

#[derive(Debug, Clone, Copy)]
pub struct BuildingType {
    pub code: &'static str,
    pub label: &'static str,
    pub residential: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct SelectItem {
    pub code: &'static str,
    pub label: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RetrofitMeasure {
    Wall,
    Roof,
    Floor,
    Window,
    Cooling,
    Heating,
    Hx,
    Lights,
    HwBoiler,
    CoolRoof,
    Blind,
    Pv,
}

impl RetrofitMeasure {
    pub const ALL: [Self; 12] = [
        Self::Wall,
        Self::Roof,
        Self::Floor,
        Self::Window,
        Self::Cooling,
        Self::Heating,
        Self::Hx,
        Self::Lights,
        Self::HwBoiler,
        Self::CoolRoof,
        Self::Blind,
        Self::Pv,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Wall => "벽체",
            Self::Roof => "지붕",
            Self::Floor => "바닥",
            Self::Window => "창호",
            Self::Cooling => "냉방",
            Self::Heating => "난방",
            Self::Hx => "열교환",
            Self::Lights => "조명",
            Self::HwBoiler => "급탕",
            Self::CoolRoof => "쿨루프",
            Self::Blind => "블라인드",
            Self::Pv => "PV",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RetrofitOption {
    pub id: u64,
    pub label: String,
    pub measures: BTreeSet<RetrofitMeasure>,
}

#[derive(Debug, Clone)]
pub struct EstimateRequest {
    pub building_type: String,
    pub climate: String,
    pub era: String,
    pub area_m2: f64,
    pub options: Vec<RetrofitOption>,
}

#[derive(Debug, Clone)]
pub struct EnergyTriplet {
    pub gas: f64,
    pub elec: f64,
    pub co2: f64,
}

#[derive(Debug, Clone)]
pub struct OptionEstimate {
    pub id: u64,
    pub label: String,
    pub gas_after: f64,
    pub elec_after: f64,
    pub co2_reduction: f64,
    pub cost: u64,
}

#[derive(Debug, Clone)]
pub struct EstimateResult {
    pub baseline: EnergyTriplet,
    pub options: Vec<OptionEstimate>,
}

#[derive(Debug, Clone)]
pub struct ParetoProgressEvent {
    pub progress: f32,
    pub message: String,
}

pub const BUILDING_TYPES: &[BuildingType] = &[
    BuildingType {
        code: "SingleHousing",
        label: "단독주택",
        residential: true,
    },
    BuildingType {
        code: "SmallMultiHousing",
        label: "소형공동주택 유형",
        residential: true,
    },
    BuildingType {
        code: "MultiHousing",
        label: "공동주택 유형",
        residential: true,
    },
    BuildingType {
        code: "Office",
        label: "업무시설",
        residential: false,
    },
    BuildingType {
        code: "PoliceOffice",
        label: "파출소",
        residential: false,
    },
    BuildingType {
        code: "Firestation",
        label: "소방서",
        residential: false,
    },
    BuildingType {
        code: "PostOffice",
        label: "우체국",
        residential: false,
    },
    BuildingType {
        code: "Theater",
        label: "공연시설",
        residential: false,
    },
    BuildingType {
        code: "Gallery",
        label: "전시장",
        residential: false,
    },
    BuildingType {
        code: "Terminal",
        label: "여객시설",
        residential: false,
    },
    BuildingType {
        code: "Transport",
        label: "철도공항",
        residential: false,
    },
    BuildingType {
        code: "Hospital",
        label: "병원",
        residential: false,
    },
    BuildingType {
        code: "Kindergarten",
        label: "아동관련시설",
        residential: false,
    },
    BuildingType {
        code: "School",
        label: "초중고",
        residential: false,
    },
    BuildingType {
        code: "Univ",
        label: "대학",
        residential: false,
    },
    BuildingType {
        code: "Lab",
        label: "연구소",
        residential: false,
    },
    BuildingType {
        code: "TrainingCtr",
        label: "수련시설",
        residential: false,
    },
    BuildingType {
        code: "Gymnasium",
        label: "체육시설",
        residential: false,
    },
    BuildingType {
        code: "Library",
        label: "도서관 유형",
        residential: false,
    },
    BuildingType {
        code: "ClassA",
        label: "업무지원시설 유형",
        residential: false,
    },
];

pub const CLIMATES: &[SelectItem] = &[
    SelectItem {
        code: "0",
        label: "중부1",
    },
    SelectItem {
        code: "1",
        label: "중부2",
    },
    SelectItem {
        code: "2",
        label: "남부",
    },
    SelectItem {
        code: "3",
        label: "제주",
    },
];

pub const ERAS: &[SelectItem] = &[
    SelectItem {
        code: "0",
        label: "1986 이전",
    },
    SelectItem {
        code: "1",
        label: "1987-2000",
    },
    SelectItem {
        code: "2",
        label: "2001-2007",
    },
    SelectItem {
        code: "3",
        label: "2008-2009",
    },
    SelectItem {
        code: "4",
        label: "2010-2012",
    },
    SelectItem {
        code: "5",
        label: "2013-2017",
    },
];

pub fn estimate_reduction(
    request: &EstimateRequest,
    model_store: &EmbeddedModelStore,
    reference_data: &ReferenceData,
) -> Result<EstimateResult, String> {
    if request.area_m2 <= 0.0 {
        return Err("area_m2 must be greater than zero".to_string());
    }

    let climate = request
        .climate
        .parse::<u8>()
        .map_err(|error| error.to_string())?;
    let era = request
        .era
        .parse::<u8>()
        .map_err(|error| error.to_string())?;
    let base_type = request.building_type.as_str();
    let model1_name = format!("{base_type}_1");
    let residential = reference_data
        .get(&model1_name)
        .ok_or_else(|| format!("metadata not found: {model1_name}"))?
        .residential;
    let model1_weight = reference_data.model1_weight_for_base(base_type)?;
    let uncertain_samples = generate_uncertain_samples(DEFAULT_SAMPLE_COUNT);

    let before_row = converted_input_row(
        reference_data,
        residential,
        climate,
        era,
        &RetrofitCodes::default(),
    )?;
    let before_inputs = build_ann_inputs(&uncertain_samples, &before_row);
    let before_predictions =
        model_store.predict_pair_split(base_type, model1_weight, &before_inputs)?;
    let baseline = summarize_predictions(&before_predictions)?;

    let options = request
        .options
        .iter()
        .map(|option| {
            let codes = RetrofitCodes::from_option(option);
            let after_row = converted_input_row(reference_data, residential, climate, era, &codes)?;
            let after_inputs = build_ann_inputs(&uncertain_samples, &after_row);
            let after_predictions =
                model_store.predict_pair_split(base_type, model1_weight, &after_inputs)?;
            let after = summarize_predictions(&after_predictions)?;

            Ok(estimate_option(&baseline, &after, request.area_m2, option))
        })
        .collect::<Result<Vec<_>, String>>()?;

    Ok(EstimateResult { baseline, options })
}

pub fn run_preview_pareto_job(sender: std::sync::mpsc::Sender<ParetoProgressEvent>) {
    thread::spawn(move || {
        let phases = [
            "50샘플 예비평가",
            "불확실성 필터링",
            "후보 재샘플링",
            "1000샘플 정밀평가",
            "결과 정렬",
        ];

        for step in 0..=100 {
            let phase = phases[(step as usize * phases.len()).saturating_sub(1) / 100];
            let _ = sender.send(ParetoProgressEvent {
                progress: step as f32 / 100.0,
                message: phase.to_string(),
            });
            thread::sleep(Duration::from_millis(35));
        }
    });
}

fn estimate_option(
    baseline: &EnergyTriplet,
    after: &EnergyTriplet,
    area_m2: f64,
    option: &RetrofitOption,
) -> OptionEstimate {
    let active_measures = option.measures.len();
    let cost = (area_m2 * active_measures as f64 * 42_000.0).round() as u64;

    OptionEstimate {
        id: option.id,
        label: option.label.clone(),
        gas_after: after.gas,
        elec_after: after.elec,
        co2_reduction: round1(baseline.co2 - after.co2),
        cost,
    }
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

#[derive(Debug, Clone, Default)]
struct RetrofitCodes {
    wall: u8,
    roof: u8,
    floor: u8,
    window: u8,
    cooling: u8,
    heating: u8,
    hx: u8,
    lights: u8,
    hw_boiler: u8,
    coolroof: u8,
    blind: u8,
    pv: u8,
}

impl RetrofitCodes {
    fn from_option(option: &RetrofitOption) -> Self {
        Self {
            wall: option.measures.contains(&RetrofitMeasure::Wall) as u8,
            roof: option.measures.contains(&RetrofitMeasure::Roof) as u8,
            floor: option.measures.contains(&RetrofitMeasure::Floor) as u8,
            window: option.measures.contains(&RetrofitMeasure::Window) as u8,
            cooling: option.measures.contains(&RetrofitMeasure::Cooling) as u8,
            heating: option.measures.contains(&RetrofitMeasure::Heating) as u8,
            hx: option.measures.contains(&RetrofitMeasure::Hx) as u8,
            lights: option.measures.contains(&RetrofitMeasure::Lights) as u8,
            hw_boiler: option.measures.contains(&RetrofitMeasure::HwBoiler) as u8,
            coolroof: option.measures.contains(&RetrofitMeasure::CoolRoof) as u8,
            blind: option.measures.contains(&RetrofitMeasure::Blind) as u8,
            pv: option.measures.contains(&RetrofitMeasure::Pv) as u8,
        }
    }
}

fn converted_input_row(
    reference_data: &ReferenceData,
    residential: bool,
    climate: u8,
    era: u8,
    codes: &RetrofitCodes,
) -> Result<[f32; 18], String> {
    let thermal = reference_data.thermal_properties(
        residential,
        climate,
        era,
        codes.wall,
        codes.roof,
        codes.floor,
        codes.window,
    )?;
    let mut climate_one_hot = [0.0; 4];
    let climate_index = climate as usize;
    if climate_index >= climate_one_hot.len() {
        return Err(format!("invalid climate index: {climate}"));
    }
    climate_one_hot[climate_index] = 1.0;

    Ok([
        thermal.wall,
        thermal.roof,
        thermal.floor,
        thermal.win_u_scaled,
        thermal.shgc,
        codes.cooling as f32,
        codes.heating as f32,
        codes.hx as f32,
        codes.lights as f32,
        codes.hw_boiler as f32,
        codes.coolroof as f32,
        codes.blind as f32,
        codes.pv as f32,
        era as f32,
        climate_one_hot[0],
        climate_one_hot[1],
        climate_one_hot[2],
        climate_one_hot[3],
    ])
}

fn build_ann_inputs(uncertain_samples: &[[f32; 7]], converted_row: &[f32; 18]) -> Vec<Vec<f32>> {
    uncertain_samples
        .iter()
        .map(|sample| {
            let mut input = Vec::with_capacity(25);
            input.extend_from_slice(sample);
            input.extend_from_slice(converted_row);
            input
        })
        .collect()
}

fn summarize_predictions(predictions: &[Vec<f32>]) -> Result<EnergyTriplet, String> {
    if predictions.is_empty() {
        return Err("cannot summarize empty prediction array".to_string());
    }

    let mut gas_sum = 0.0;
    let mut elec_sum = 0.0;
    let mut co2_sum = 0.0;
    for prediction in predictions {
        if prediction.len() != 2 {
            return Err(format!(
                "expected 2 model outputs, got {}",
                prediction.len()
            ));
        }

        let gas = prediction[0] as f64;
        let elec = prediction[1] as f64;
        gas_sum += gas;
        elec_sum += elec;
        co2_sum += elec * CO2_ELEC + gas * CO2_GAS;
    }

    let n = predictions.len() as f64;
    Ok(EnergyTriplet {
        gas: round1(gas_sum / n),
        elec: round1(elec_sum / n),
        co2: round1(co2_sum / n),
    })
}

fn generate_uncertain_samples(count: usize) -> Vec<[f32; 7]> {
    let steps = [37, 53, 71, 91, 101, 113, 127];
    let offsets = [11, 23, 31, 47, 59, 67, 83];

    (0..count)
        .map(|sample_index| {
            let mut sample = [0.0; 7];
            for variable_index in 0..7 {
                let u = lhs_unit(
                    sample_index,
                    count,
                    steps[variable_index],
                    offsets[variable_index],
                );
                sample[variable_index] = match variable_index {
                    0 | 1 | 5 | 6 => (0.5 + inverse_standard_normal(u) / 6.0) as f32,
                    _ => u as f32,
                };
            }
            sample
        })
        .collect()
}

fn lhs_unit(sample_index: usize, sample_count: usize, step: usize, offset: usize) -> f64 {
    let bucket = (sample_index * step + offset) % sample_count;
    (bucket as f64 + 0.5) / sample_count as f64
}

fn inverse_standard_normal(p: f64) -> f64 {
    debug_assert!(p > 0.0 && p < 1.0);

    const A: [f64; 6] = [
        -3.969_683_028_665_376e1,
        2.209_460_984_245_205e2,
        -2.759_285_104_469_687e2,
        1.383_577_518_672_69e2,
        -3.066_479_806_614_716e1,
        2.506_628_277_459_239,
    ];
    const B: [f64; 5] = [
        -5.447_609_879_822_406e1,
        1.615_858_368_580_409e2,
        -1.556_989_798_598_866e2,
        6.680_131_188_771_972e1,
        -1.328_068_155_288_572e1,
    ];
    const C: [f64; 6] = [
        -7.784_894_002_430_293e-3,
        -3.223_964_580_411_365e-1,
        -2.400_758_277_161_838,
        -2.549_732_539_343_734,
        4.374_664_141_464_968,
        2.938_163_982_698_783,
    ];
    const D: [f64; 4] = [
        7.784_695_709_041_462e-3,
        3.224_671_290_700_398e-1,
        2.445_134_137_142_996,
        3.754_408_661_907_416,
    ];

    let low = 0.02425;
    let high = 1.0 - low;

    if p < low {
        let q = (-2.0 * p.ln()).sqrt();
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    } else if p <= high {
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    } else {
        let q = (-2.0 * (1.0 - p).ln()).sqrt();
        -(((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        RetrofitCodes, build_ann_inputs, converted_input_row, generate_uncertain_samples,
        inverse_standard_normal,
    };
    use crate::domain::{EmbeddedModelStore, EstimateRequest, ReferenceData, RetrofitOption};
    use std::collections::BTreeSet;

    #[test]
    fn builds_25_dimensional_ann_inputs() {
        let reference_data = ReferenceData::load().expect("reference metadata should load");
        let uncertain_samples = generate_uncertain_samples(10);
        let converted =
            converted_input_row(&reference_data, false, 0, 0, &RetrofitCodes::default())
                .expect("converted row should build");
        let inputs = build_ann_inputs(&uncertain_samples, &converted);

        assert_eq!(inputs.len(), 10);
        assert_eq!(inputs[0].len(), 25);
        assert!((inverse_standard_normal(0.5)).abs() < 0.000_001);
    }

    #[test]
    fn estimates_with_embedded_ann_models() {
        let model_store = EmbeddedModelStore::load().expect("embedded model asset should load");
        let reference_data = ReferenceData::load().expect("reference metadata should load");
        let request = EstimateRequest {
            building_type: "Office".to_string(),
            climate: "0".to_string(),
            era: "2".to_string(),
            area_m2: 1000.0,
            options: vec![RetrofitOption {
                id: 1,
                label: "test".to_string(),
                measures: BTreeSet::new(),
            }],
        };

        let result = super::estimate_reduction(&request, &model_store, &reference_data)
            .expect("ANN estimate should succeed");

        assert_eq!(result.options.len(), 1);
        assert!(result.baseline.elec.is_finite());
        assert!(result.options[0].elec_after.is_finite());
    }
}
