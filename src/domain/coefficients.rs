use std::{collections::BTreeSet, thread, time::Duration};

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
    BuildingType { code: "SingleHousing", label: "단독주택", residential: true },
    BuildingType { code: "SmallMultiHousing", label: "소형공동주택 유형", residential: true },
    BuildingType { code: "MultiHousing", label: "공동주택 유형", residential: true },
    BuildingType { code: "Office", label: "업무시설", residential: false },
    BuildingType { code: "PoliceOffice", label: "파출소", residential: false },
    BuildingType { code: "Firestation", label: "소방서", residential: false },
    BuildingType { code: "PostOffice", label: "우체국", residential: false },
    BuildingType { code: "Theater", label: "공연시설", residential: false },
    BuildingType { code: "Gallery", label: "전시장", residential: false },
    BuildingType { code: "Terminal", label: "여객시설", residential: false },
    BuildingType { code: "Transport", label: "철도공항", residential: false },
    BuildingType { code: "Hospital", label: "병원", residential: false },
    BuildingType { code: "Kindergarten", label: "아동관련시설", residential: false },
    BuildingType { code: "School", label: "초중고", residential: false },
    BuildingType { code: "Univ", label: "대학", residential: false },
    BuildingType { code: "Lab", label: "연구소", residential: false },
    BuildingType { code: "TrainingCtr", label: "수련시설", residential: false },
    BuildingType { code: "Gymnasium", label: "체육시설", residential: false },
    BuildingType { code: "Library", label: "도서관 유형", residential: false },
    BuildingType { code: "ClassA", label: "업무지원시설 유형", residential: false },
];

pub const CLIMATES: &[SelectItem] = &[
    SelectItem { code: "0", label: "중부1" },
    SelectItem { code: "1", label: "중부2" },
    SelectItem { code: "2", label: "남부" },
    SelectItem { code: "3", label: "제주" },
];

pub const ERAS: &[SelectItem] = &[
    SelectItem { code: "0", label: "1986 이전" },
    SelectItem { code: "1", label: "1987-2000" },
    SelectItem { code: "2", label: "2001-2007" },
    SelectItem { code: "3", label: "2008-2009" },
    SelectItem { code: "4", label: "2010-2012" },
    SelectItem { code: "5", label: "2013-2017" },
];

pub fn estimate_reduction(request: &EstimateRequest) -> Result<EstimateResult, String> {
    if request.area_m2 <= 0.0 {
        return Err("area_m2 must be greater than zero".to_string());
    }

    let baseline = baseline_for(request);
    let options = request
        .options
        .iter()
        .enumerate()
        .map(|(index, option)| estimate_option(&baseline, request.area_m2, index, option))
        .collect();

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

fn baseline_for(request: &EstimateRequest) -> EnergyTriplet {
    let type_factor = match request.building_type.as_str() {
        "SingleHousing" => 1.42,
        "MultiHousing" | "SmallMultiHousing" => 0.92,
        "Hospital" => 1.32,
        "School" => 0.86,
        "Office" => 0.78,
        _ => 1.0,
    };
    let climate_factor = 1.0 + request.climate.parse::<f64>().unwrap_or(0.0) * 0.035;
    let era_factor = 1.25 - request.era.parse::<f64>().unwrap_or(2.0) * 0.045;

    let gas = round1(request.area_m2 * 0.018 * type_factor * climate_factor * era_factor);
    let elec = round1(request.area_m2 * 0.052 * type_factor * climate_factor);
    let co2 = round1(elec * 0.45941 + gas * 0.20245);

    EnergyTriplet { gas, elec, co2 }
}

fn estimate_option(
    baseline: &EnergyTriplet,
    area_m2: f64,
    index: usize,
    option: &RetrofitOption,
) -> OptionEstimate {
    let active_measures = option.measures.len();
    let reduction = (0.035 + active_measures as f64 * 0.0175 + index as f64 * 0.004).min(0.72);
    let gas_after = round1(baseline.gas * (1.0 - reduction));
    let elec_after = round1(baseline.elec * (1.0 - reduction * 0.86));
    let co2_after = elec_after * 0.45941 + gas_after * 0.20245;
    let co2_reduction = round1((baseline.co2 - co2_after).max(0.0));
    let cost = (area_m2 * active_measures as f64 * 42_000.0).round() as u64;

    OptionEstimate {
        id: option.id,
        label: option.label.clone(),
        gas_after,
        elec_after,
        co2_reduction,
        cost,
    }
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

