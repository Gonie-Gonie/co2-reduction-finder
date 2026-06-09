use std::{
    collections::BTreeMap,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
    thread,
};

use serde::Deserialize;

use super::{
    metrics::EnergyMetric, model_store::EmbeddedModelStore, reference_data::ReferenceData,
};

const DEFAULT_SAMPLE_COUNT: usize = 1000;
const RETROFIT_COSTS_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/retrofit_costs.json"
));
static RETROFIT_COSTS: OnceLock<RetrofitCostConfig> = OnceLock::new();

#[derive(Debug, Clone, Copy)]
pub struct BuildingType {
    pub code: &'static str,
    pub label: &'static str,
    #[allow(dead_code)]
    pub residential: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct SelectItem {
    pub code: &'static str,
    pub label: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BinaryRetrofitMeasure {
    Cooling,
    Heating,
    Hx,
    Lights,
    HwBoiler,
    CoolRoof,
    Blind,
    Pv,
}

impl BinaryRetrofitMeasure {
    pub const ALL: [Self; 8] = [
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RetrofitSpec {
    pub wall: u8,
    pub roof: u8,
    pub floor: u8,
    pub window: u8,
    pub cooling: bool,
    pub heating: bool,
    pub hx: bool,
    pub lights: bool,
    pub hw_boiler: bool,
    pub coolroof: bool,
    pub blind: bool,
    pub pv: bool,
}

impl RetrofitSpec {
    pub fn active_count(self) -> usize {
        [
            self.wall > 0,
            self.roof > 0,
            self.floor > 0,
            self.window > 0,
            self.cooling,
            self.heating,
            self.hx,
            self.lights,
            self.hw_boiler,
            self.coolroof,
            self.blind,
            self.pv,
        ]
        .into_iter()
        .filter(|active| *active)
        .count()
    }

    pub fn is_enabled(self, measure: BinaryRetrofitMeasure) -> bool {
        match measure {
            BinaryRetrofitMeasure::Cooling => self.cooling,
            BinaryRetrofitMeasure::Heating => self.heating,
            BinaryRetrofitMeasure::Hx => self.hx,
            BinaryRetrofitMeasure::Lights => self.lights,
            BinaryRetrofitMeasure::HwBoiler => self.hw_boiler,
            BinaryRetrofitMeasure::CoolRoof => self.coolroof,
            BinaryRetrofitMeasure::Blind => self.blind,
            BinaryRetrofitMeasure::Pv => self.pv,
        }
    }

    pub fn set_enabled(&mut self, measure: BinaryRetrofitMeasure, enabled: bool) {
        match measure {
            BinaryRetrofitMeasure::Cooling => self.cooling = enabled,
            BinaryRetrofitMeasure::Heating => self.heating = enabled,
            BinaryRetrofitMeasure::Hx => self.hx = enabled,
            BinaryRetrofitMeasure::Lights => self.lights = enabled,
            BinaryRetrofitMeasure::HwBoiler => self.hw_boiler = enabled,
            BinaryRetrofitMeasure::CoolRoof => self.coolroof = enabled,
            BinaryRetrofitMeasure::Blind => self.blind = enabled,
            BinaryRetrofitMeasure::Pv => self.pv = enabled,
        }
    }

    pub fn summary_label(self) -> String {
        let mut parts = Vec::new();
        if self.wall > 0 {
            parts.push(format!("벽{}", self.wall));
        }
        if self.roof > 0 {
            parts.push(format!("지붕{}", self.roof));
        }
        if self.floor > 0 {
            parts.push(format!("바닥{}", self.floor));
        }
        if self.window > 0 {
            parts.push(format!("창호 {}", window_level_label(self.window)));
        }
        for measure in BinaryRetrofitMeasure::ALL {
            if self.is_enabled(measure) {
                parts.push(measure.label().to_string());
            }
        }

        if parts.is_empty() {
            "-".to_string()
        } else {
            parts.join(" ")
        }
    }
}

fn window_level_label(level: u8) -> &'static str {
    match level {
        1 => "1등급",
        2 => "2등급",
        3 => "현행",
        _ => "-",
    }
}

#[derive(Debug, Clone)]
pub struct RetrofitOption {
    pub id: u64,
    pub label: String,
    pub spec: RetrofitSpec,
}

#[derive(Debug, Clone)]
pub struct EstimateRequest {
    pub building_type: String,
    pub climate: String,
    pub era: String,
    pub area_m2: f64,
    pub metric: EnergyMetric,
    pub options: Vec<RetrofitOption>,
}

#[derive(Debug, Clone, Copy)]
pub struct EnergyValues {
    pub gas: f64,
    pub elec: f64,
    pub total: f64,
    pub primary: f64,
    pub co2: f64,
}

impl EnergyValues {
    fn from_gas_elec(gas: f64, elec: f64) -> Self {
        let primary = EnergyMetric::PrimaryEnergy
            .factor()
            .per_area_value(gas, elec);
        let co2 = EnergyMetric::Ghg.factor().per_area_value(gas, elec);
        Self {
            gas,
            elec,
            total: gas + elec,
            primary,
            co2,
        }
    }

    fn zero() -> Self {
        Self {
            gas: 0.0,
            elec: 0.0,
            total: 0.0,
            primary: 0.0,
            co2: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct EnergyStats {
    pub mean: EnergyValues,
    pub std: EnergyValues,
}

#[derive(Debug, Clone)]
struct EnergyDistribution {
    stats: EnergyStats,
    samples: Vec<EnergyValues>,
}

#[derive(Debug, Clone)]
pub struct OptionEstimate {
    pub id: u64,
    pub label: String,
    pub spec: RetrofitSpec,
    pub after: EnergyStats,
    pub reduction: EnergyStats,
    pub cost: u64,
}

#[derive(Debug, Clone)]
pub struct EstimateResult {
    pub baseline: EnergyStats,
    pub options: Vec<OptionEstimate>,
}

#[derive(Debug, Clone)]
pub struct ParetoMetricResult {
    pub metric: EnergyMetric,
    pub result: EstimateResult,
}

#[derive(Debug, Clone)]
pub struct ParetoResultSet {
    pub results: Vec<ParetoMetricResult>,
}

impl ParetoResultSet {
    pub fn result_for(&self, metric: EnergyMetric) -> Option<&EstimateResult> {
        self.results
            .iter()
            .find(|item| item.metric == metric)
            .map(|item| &item.result)
    }
}

#[derive(Debug, Clone)]
pub struct ParetoProgressEvent {
    pub progress: f32,
    pub message: String,
    pub result: Option<ParetoResultSet>,
    pub error: Option<String>,
}

impl ParetoProgressEvent {
    fn progress(progress: f32, message: impl Into<String>) -> Self {
        Self {
            progress,
            message: message.into(),
            result: None,
            error: None,
        }
    }

    fn finished(message: impl Into<String>, result: ParetoResultSet) -> Self {
        Self {
            progress: 1.0,
            message: message.into(),
            result: Some(result),
            error: None,
        }
    }

    fn failed(message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            progress: 1.0,
            message: message.clone(),
            result: None,
            error: Some(message),
        }
    }
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
    estimate_reduction_with_sample_count(request, model_store, reference_data, DEFAULT_SAMPLE_COUNT)
}

fn estimate_reduction_with_sample_count(
    request: &EstimateRequest,
    model_store: &EmbeddedModelStore,
    reference_data: &ReferenceData,
    sample_count: usize,
) -> Result<EstimateResult, String> {
    if request.area_m2 <= 0.0 {
        return Err("area_m2 must be greater than zero".to_string());
    }
    if sample_count == 0 {
        return Err("sample_count must be greater than zero".to_string());
    }

    let context = prepare_evaluation_context(request, model_store, reference_data, sample_count)?;

    let options = request
        .options
        .iter()
        .map(|option| {
            evaluate_option(
                &context,
                request.area_m2,
                option,
                model_store,
                reference_data,
            )
        })
        .collect::<Result<Vec<_>, String>>()?;

    Ok(EstimateResult {
        baseline: context.baseline,
        options,
    })
}

#[derive(Debug, Clone)]
struct EvaluationContext {
    base_type: String,
    residential: bool,
    climate: u8,
    era: u8,
    uncertain_samples: Vec<[f32; 7]>,
    baseline: EnergyStats,
    baseline_samples: Vec<EnergyValues>,
}

fn prepare_evaluation_context(
    request: &EstimateRequest,
    model_store: &EmbeddedModelStore,
    reference_data: &ReferenceData,
    sample_count: usize,
) -> Result<EvaluationContext, String> {
    let climate = request
        .climate
        .parse::<u8>()
        .map_err(|error| error.to_string())?;
    let era = request
        .era
        .parse::<u8>()
        .map_err(|error| error.to_string())?;
    let base_type = request.building_type.clone();
    let residential = model_store
        .building_type(&base_type)
        .ok_or_else(|| format!("building type not found in model registry: {base_type}"))?
        .residential;
    let uncertain_samples = generate_uncertain_samples(sample_count);

    let before_row = converted_input_row(
        reference_data,
        residential,
        climate,
        era,
        &RetrofitSpec::default(),
    )?;
    let before_inputs = build_ann_inputs(&uncertain_samples, &before_row);
    let before_predictions = model_store.predict_weighted_segments(&base_type, &before_inputs)?;
    let baseline_distribution = summarize_predictions(&before_predictions)?;

    Ok(EvaluationContext {
        base_type,
        residential,
        climate,
        era,
        uncertain_samples,
        baseline: baseline_distribution.stats,
        baseline_samples: baseline_distribution.samples,
    })
}

fn evaluate_option(
    context: &EvaluationContext,
    area_m2: f64,
    option: &RetrofitOption,
    model_store: &EmbeddedModelStore,
    reference_data: &ReferenceData,
) -> Result<OptionEstimate, String> {
    validate_retrofit_spec(option.spec)?;
    let after_row = converted_input_row(
        reference_data,
        context.residential,
        context.climate,
        context.era,
        &option.spec,
    )?;
    let after_inputs = build_ann_inputs(&context.uncertain_samples, &after_row);
    let after_predictions =
        model_store.predict_weighted_segments(&context.base_type, &after_inputs)?;
    let after = summarize_predictions(&after_predictions)?;
    let reduction = summarize_reductions(&context.baseline_samples, &after.samples)?;

    Ok(estimate_option(
        context.baseline,
        after.stats,
        reduction,
        area_m2,
        option,
    ))
}

pub fn run_pareto_job(
    request: EstimateRequest,
    model_store: EmbeddedModelStore,
    reference_data: ReferenceData,
    sender: Sender<ParetoProgressEvent>,
    cancel: Arc<AtomicBool>,
) {
    thread::spawn(move || {
        match run_pareto_job_inner(&request, &model_store, &reference_data, &sender, &cancel) {
            Ok(result) => {
                let _ = sender.send(ParetoProgressEvent::finished(
                    "Pareto 후보 계산 완료",
                    result,
                ));
            }
            Err(error) if error == PARETO_CANCELLED => {
                let _ = sender.send(ParetoProgressEvent::progress(1.0, "중지됨"));
            }
            Err(error) => {
                let _ = sender.send(ParetoProgressEvent::failed(error));
            }
        }
    });
}

const PARETO_CANCELLED: &str = "__pareto_cancelled__";
const PARETO_SCREEN_SAMPLE_COUNT: usize = 50;
const PARETO_INTERMEDIATE_SAMPLE_COUNT: usize = 200;
const PARETO_FINAL_SAMPLE_COUNT: usize = 1000;
const PARETO_INTERMEDIATE_LIMIT: usize = 96;
const PARETO_FINAL_LIMIT: usize = 12;

#[derive(Debug, Clone)]
struct ScoredCandidate {
    option: RetrofitOption,
    estimate: OptionEstimate,
}

fn run_pareto_job_inner(
    request: &EstimateRequest,
    model_store: &EmbeddedModelStore,
    reference_data: &ReferenceData,
    sender: &Sender<ParetoProgressEvent>,
    cancel: &AtomicBool,
) -> Result<ParetoResultSet, String> {
    send_progress(sender, cancel, 0.01, "Pareto 후보 생성")?;
    let candidates = generate_pareto_candidates();

    send_progress(
        sender,
        cancel,
        0.03,
        format!("50샘플 예비평가: {}개 후보", candidates.len()),
    )?;
    let screened = evaluate_pareto_stage(
        request,
        model_store,
        reference_data,
        candidates,
        PARETO_SCREEN_SAMPLE_COUNT,
        (0.03, 0.58),
        "50샘플 예비평가",
        sender,
        cancel,
    )?;
    let intermediate_options = combined_limited_front(&screened, PARETO_INTERMEDIATE_LIMIT)
        .into_iter()
        .map(|candidate| candidate.option)
        .collect::<Vec<_>>();

    send_progress(
        sender,
        cancel,
        0.60,
        format!("200샘플 경계 재평가: {}개 후보", intermediate_options.len()),
    )?;
    let intermediate = evaluate_pareto_stage(
        request,
        model_store,
        reference_data,
        intermediate_options,
        PARETO_INTERMEDIATE_SAMPLE_COUNT,
        (0.60, 0.82),
        "200샘플 경계 재평가",
        sender,
        cancel,
    )?;
    let final_options = combined_limited_front(&intermediate, PARETO_FINAL_LIMIT)
        .into_iter()
        .map(|candidate| candidate.option)
        .collect::<Vec<_>>();

    send_progress(
        sender,
        cancel,
        0.84,
        format!("1000샘플 최종평가: {}개 후보", final_options.len()),
    )?;
    let final_scored = evaluate_pareto_stage(
        request,
        model_store,
        reference_data,
        final_options,
        PARETO_FINAL_SAMPLE_COUNT,
        (0.84, 0.96),
        "1000샘플 최종평가",
        sender,
        cancel,
    )?;
    send_progress(sender, cancel, 0.98, "지표별 Pareto 결과 정렬")?;

    let mut results = Vec::with_capacity(EnergyMetric::ALL.len());
    for metric in EnergyMetric::ALL {
        let mut final_front = limit_front(
            pareto_front(final_scored.clone(), metric),
            PARETO_FINAL_LIMIT,
        );
        final_front.sort_by(|a, b| {
            a.estimate.cost.cmp(&b.estimate.cost).then_with(|| {
                estimate_metric_reduction(&b.estimate, metric)
                    .total_cmp(&estimate_metric_reduction(&a.estimate, metric))
            })
        });

        let mut final_request = request.clone();
        final_request.metric = metric;
        final_request.options = final_front
            .into_iter()
            .enumerate()
            .map(|(index, candidate)| {
                let mut option = candidate.option;
                option.id = 100_000 + index as u64;
                option.label = format!("Pareto {}: {}", index + 1, option.spec.summary_label());
                option
            })
            .collect::<Vec<_>>();

        let result = estimate_reduction_with_sample_count(
            &final_request,
            model_store,
            reference_data,
            DEFAULT_SAMPLE_COUNT,
        )?;
        results.push(ParetoMetricResult { metric, result });
    }

    Ok(ParetoResultSet { results })
}

#[allow(clippy::too_many_arguments)]
fn evaluate_pareto_stage(
    request: &EstimateRequest,
    model_store: &EmbeddedModelStore,
    reference_data: &ReferenceData,
    candidates: Vec<RetrofitOption>,
    sample_count: usize,
    progress_range: (f32, f32),
    phase: &str,
    sender: &Sender<ParetoProgressEvent>,
    cancel: &AtomicBool,
) -> Result<Vec<ScoredCandidate>, String> {
    let context = prepare_evaluation_context(request, model_store, reference_data, sample_count)?;
    let total = candidates.len().max(1);
    let mut scored = Vec::with_capacity(candidates.len());

    for (index, option) in candidates.into_iter().enumerate() {
        if index % 64 == 0 {
            let ratio = index as f32 / total as f32;
            let progress = progress_range.0 + (progress_range.1 - progress_range.0) * ratio;
            send_progress(
                sender,
                cancel,
                progress,
                format!("{phase}: {}/{}", index, total),
            )?;
        }

        let estimate = evaluate_option(
            &context,
            request.area_m2,
            &option,
            model_store,
            reference_data,
        )?;
        if has_positive_metric_reduction(&estimate) && estimate.cost > 0 {
            scored.push(ScoredCandidate { option, estimate });
        }
    }

    send_progress(
        sender,
        cancel,
        progress_range.1,
        format!("{phase}: {}/{}", total, total),
    )?;

    Ok(scored)
}

fn generate_pareto_candidates() -> Vec<RetrofitOption> {
    let mut candidates = Vec::with_capacity(27_647);
    let mut id = 10_000;

    for wall in 0..=2 {
        for roof in 0..=2 {
            for floor in 0..=2 {
                for window in 0..=3 {
                    for bits in 0..(1_u16 << BinaryRetrofitMeasure::ALL.len()) {
                        let spec = RetrofitSpec {
                            wall,
                            roof,
                            floor,
                            window,
                            cooling: bit_enabled(bits, 0),
                            heating: bit_enabled(bits, 1),
                            hx: bit_enabled(bits, 2),
                            lights: bit_enabled(bits, 3),
                            hw_boiler: bit_enabled(bits, 4),
                            coolroof: bit_enabled(bits, 5),
                            blind: bit_enabled(bits, 6),
                            pv: bit_enabled(bits, 7),
                        };
                        if spec.active_count() == 0 {
                            continue;
                        }

                        candidates.push(RetrofitOption {
                            id,
                            label: spec.summary_label(),
                            spec,
                        });
                        id += 1;
                    }
                }
            }
        }
    }

    candidates
}

fn bit_enabled(bits: u16, index: u16) -> bool {
    bits & (1_u16 << index) != 0
}

fn pareto_front(
    mut candidates: Vec<ScoredCandidate>,
    metric: EnergyMetric,
) -> Vec<ScoredCandidate> {
    candidates.sort_by(|a, b| {
        a.estimate.cost.cmp(&b.estimate.cost).then_with(|| {
            estimate_metric_reduction(&b.estimate, metric)
                .total_cmp(&estimate_metric_reduction(&a.estimate, metric))
        })
    });

    let mut front = Vec::new();
    let mut best_reduction = f64::NEG_INFINITY;
    for candidate in candidates {
        let reduction = estimate_metric_reduction(&candidate.estimate, metric);
        if reduction > best_reduction + 0.05 {
            best_reduction = reduction;
            front.push(candidate);
        }
    }

    front
}

fn combined_limited_front(candidates: &[ScoredCandidate], limit: usize) -> Vec<ScoredCandidate> {
    let mut combined = BTreeMap::new();
    for metric in EnergyMetric::ALL {
        for candidate in limit_front(pareto_front(candidates.to_vec(), metric), limit) {
            combined.entry(candidate.option.id).or_insert(candidate);
        }
    }

    combined.into_values().collect()
}

fn limit_front(front: Vec<ScoredCandidate>, limit: usize) -> Vec<ScoredCandidate> {
    if front.len() <= limit || limit == 0 {
        return front;
    }
    if limit == 1 {
        return front.into_iter().last().into_iter().collect();
    }

    let last = front.len() - 1;
    let mut limited = Vec::with_capacity(limit);
    for index in 0..limit {
        let source_index = (index * last + (limit - 1) / 2) / (limit - 1);
        limited.push(front[source_index].clone());
    }
    limited
}

fn send_progress(
    sender: &Sender<ParetoProgressEvent>,
    cancel: &AtomicBool,
    progress: f32,
    message: impl Into<String>,
) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        return Err(PARETO_CANCELLED.to_string());
    }
    sender
        .send(ParetoProgressEvent::progress(progress, message))
        .map_err(|_| PARETO_CANCELLED.to_string())
}

fn estimate_option(
    _baseline: EnergyStats,
    after: EnergyStats,
    reduction: EnergyStats,
    area_m2: f64,
    option: &RetrofitOption,
) -> OptionEstimate {
    OptionEstimate {
        id: option.id,
        label: option.label.clone(),
        spec: option.spec,
        after,
        reduction,
        cost: retrofit_cost(option.spec, area_m2),
    }
}

fn estimate_metric_reduction(estimate: &OptionEstimate, metric: EnergyMetric) -> f64 {
    let factor = metric.factor();
    factor.per_area_value(estimate.reduction.mean.gas, estimate.reduction.mean.elec)
}

fn has_positive_metric_reduction(estimate: &OptionEstimate) -> bool {
    EnergyMetric::ALL
        .iter()
        .any(|metric| estimate_metric_reduction(estimate, *metric) > 0.0)
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RetrofitCostConfig {
    schema_version: u32,
    unit_costs: RetrofitUnitCosts,
    rates: RetrofitCostRates,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RetrofitUnitCosts {
    wall: f64,
    roof: f64,
    floor: f64,
    window: f64,
    cooling: f64,
    heating: f64,
    hx: f64,
    lights: f64,
    hw_boiler: f64,
    coolroof: f64,
    blind: f64,
    pv: f64,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RetrofitCostRates {
    additional_cost_ratio_for_enhanced_option: f64,
    architecture_side_work: f64,
    machine_side_work: f64,
    demolition: f64,
    waste: f64,
    direct_labor: f64,
    indirect_labor: f64,
    expense: f64,
    overhead: f64,
    profit: f64,
    design_fee: f64,
    remodeling_design_surcharge: f64,
    supervision_fee: f64,
    tax: f64,
}

impl RetrofitCostConfig {
    fn load() -> Result<Self, String> {
        let config: Self =
            serde_json::from_str(RETROFIT_COSTS_JSON).map_err(|error| error.to_string())?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err(format!(
                "unsupported retrofit cost schemaVersion {}",
                self.schema_version
            ));
        }

        for (name, value) in [
            ("wall", self.unit_costs.wall),
            ("roof", self.unit_costs.roof),
            ("floor", self.unit_costs.floor),
            ("window", self.unit_costs.window),
            ("cooling", self.unit_costs.cooling),
            ("heating", self.unit_costs.heating),
            ("hx", self.unit_costs.hx),
            ("lights", self.unit_costs.lights),
            ("hwBoiler", self.unit_costs.hw_boiler),
            ("coolroof", self.unit_costs.coolroof),
            ("blind", self.unit_costs.blind),
            ("pv", self.unit_costs.pv),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(format!("invalid retrofit unit cost {name}: {value}"));
            }
        }

        for (name, value) in [
            (
                "additionalCostRatioForEnhancedOption",
                self.rates.additional_cost_ratio_for_enhanced_option,
            ),
            ("architectureSideWork", self.rates.architecture_side_work),
            ("machineSideWork", self.rates.machine_side_work),
            ("demolition", self.rates.demolition),
            ("waste", self.rates.waste),
            ("directLabor", self.rates.direct_labor),
            ("indirectLabor", self.rates.indirect_labor),
            ("expense", self.rates.expense),
            ("overhead", self.rates.overhead),
            ("profit", self.rates.profit),
            ("designFee", self.rates.design_fee),
            (
                "remodelingDesignSurcharge",
                self.rates.remodeling_design_surcharge,
            ),
            ("supervisionFee", self.rates.supervision_fee),
            ("tax", self.rates.tax),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(format!("invalid retrofit cost rate {name}: {value}"));
            }
        }

        Ok(())
    }
}

fn retrofit_cost_config() -> &'static RetrofitCostConfig {
    RETROFIT_COSTS
        .get_or_init(|| RetrofitCostConfig::load().expect("embedded retrofit costs should load"))
}

pub fn retrofit_cost(spec: RetrofitSpec, area_m2: f64) -> u64 {
    if spec.active_count() == 0 || area_m2 <= 0.0 {
        return 0;
    }

    let config = retrofit_cost_config();
    let costs = &config.unit_costs;
    let rates = &config.rates;

    let mut required_architecture = 0.0;
    if spec.wall > 0 {
        required_architecture += costs.wall;
    }
    if spec.roof > 0 {
        required_architecture += costs.roof;
    }
    if spec.floor > 0 {
        required_architecture += costs.floor;
    }
    if spec.window > 0 {
        required_architecture += costs.window;
    }
    if spec.wall == 2 {
        required_architecture += costs.wall * rates.additional_cost_ratio_for_enhanced_option;
    }
    if spec.roof == 2 {
        required_architecture += costs.roof * rates.additional_cost_ratio_for_enhanced_option;
    }
    if spec.floor == 2 {
        required_architecture += costs.floor * rates.additional_cost_ratio_for_enhanced_option;
    }
    if spec.window == 1 {
        required_architecture +=
            costs.window * rates.additional_cost_ratio_for_enhanced_option * 2.0;
    }
    if spec.window == 2 {
        required_architecture += costs.window * rates.additional_cost_ratio_for_enhanced_option;
    }

    let required_machine = bool_cost(spec.hx, costs.hx)
        + bool_cost(spec.cooling, costs.cooling)
        + bool_cost(spec.heating, costs.heating)
        + bool_cost(spec.hw_boiler, costs.hw_boiler);
    let required_electric = bool_cost(spec.lights, costs.lights) + bool_cost(spec.pv, costs.pv);
    let required_other = bool_cost(spec.coolroof, costs.coolroof);
    let optional_architecture = bool_cost(spec.blind, costs.blind);

    let required_work =
        required_architecture + required_machine + required_electric + required_other;
    let optional_work = optional_architecture;

    let architecture_side_work =
        (required_architecture + optional_architecture) * rates.architecture_side_work;
    let machine_side_work = required_machine * rates.machine_side_work;
    let demolition = required_work * rates.demolition;
    let waste = (required_architecture
        + required_machine
        + bool_cost(spec.lights, costs.lights)
        + bool_cost(spec.coolroof, costs.coolroof)
        + demolition
        + architecture_side_work
        + machine_side_work)
        * rates.waste;
    let side_work = architecture_side_work + machine_side_work + demolition + waste;

    let direct_labor = (required_work + optional_work + side_work) * rates.direct_labor;
    let indirect_labor = direct_labor * rates.indirect_labor;
    let expense = (required_work + optional_work + side_work + indirect_labor) * rates.expense;
    let overhead =
        (required_work + optional_work + side_work + indirect_labor + expense) * rates.overhead;
    let profit = (direct_labor + indirect_labor + expense + overhead) * rates.profit;

    let construction_cost = required_work
        + optional_work
        + side_work
        + direct_labor
        + indirect_labor
        + expense
        + overhead
        + profit;
    let design_fee = construction_cost * rates.design_fee * rates.remodeling_design_surcharge;
    let supervision_fee = construction_cost * rates.supervision_fee;
    let tax = construction_cost * rates.tax;

    ((construction_cost + design_fee + supervision_fee + tax) * area_m2).round() as u64
}

fn bool_cost(enabled: bool, cost: f64) -> f64 {
    if enabled { cost } else { 0.0 }
}

fn validate_retrofit_spec(spec: RetrofitSpec) -> Result<(), String> {
    if spec.wall > 2 {
        return Err(format!(
            "wall retrofit level must be 0-2, got {}",
            spec.wall
        ));
    }
    if spec.roof > 2 {
        return Err(format!(
            "roof retrofit level must be 0-2, got {}",
            spec.roof
        ));
    }
    if spec.floor > 2 {
        return Err(format!(
            "floor retrofit level must be 0-2, got {}",
            spec.floor
        ));
    }
    if spec.window > 3 {
        return Err(format!(
            "window retrofit level must be 0-3, got {}",
            spec.window
        ));
    }
    Ok(())
}

fn converted_input_row(
    reference_data: &ReferenceData,
    residential: bool,
    climate: u8,
    era: u8,
    spec: &RetrofitSpec,
) -> Result<[f32; 18], String> {
    let thermal = reference_data.thermal_properties(
        residential,
        climate,
        era,
        spec.wall,
        spec.roof,
        spec.floor,
        spec.window,
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
        spec.cooling as u8 as f32,
        spec.heating as u8 as f32,
        spec.hx as u8 as f32,
        spec.lights as u8 as f32,
        spec.hw_boiler as u8 as f32,
        spec.coolroof as u8 as f32,
        spec.blind as u8 as f32,
        spec.pv as u8 as f32,
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

fn summarize_predictions(predictions: &[Vec<f32>]) -> Result<EnergyDistribution, String> {
    if predictions.is_empty() {
        return Err("cannot summarize empty prediction array".to_string());
    }

    let mut samples = Vec::with_capacity(predictions.len());
    for prediction in predictions {
        if prediction.len() != 2 {
            return Err(format!(
                "expected 2 model outputs, got {}",
                prediction.len()
            ));
        }

        let gas = prediction[0] as f64;
        let elec = prediction[1] as f64;
        samples.push(EnergyValues::from_gas_elec(gas, elec));
    }

    Ok(EnergyDistribution {
        stats: summarize_energy_values(&samples)?,
        samples,
    })
}

fn summarize_reductions(
    baseline: &[EnergyValues],
    after: &[EnergyValues],
) -> Result<EnergyStats, String> {
    if baseline.len() != after.len() {
        return Err(format!(
            "baseline and after sample counts differ: {} vs {}",
            baseline.len(),
            after.len()
        ));
    }

    let reductions = baseline
        .iter()
        .zip(after.iter())
        .map(|(before, after)| {
            EnergyValues::from_gas_elec(before.gas - after.gas, before.elec - after.elec)
        })
        .collect::<Vec<_>>();
    summarize_energy_values(&reductions)
}

fn summarize_energy_values(samples: &[EnergyValues]) -> Result<EnergyStats, String> {
    if samples.is_empty() {
        return Err("cannot summarize empty energy sample array".to_string());
    }

    let n = samples.len() as f64;
    let mut sum = EnergyValues::zero();
    for sample in samples {
        sum.gas += sample.gas;
        sum.elec += sample.elec;
        sum.total += sample.total;
        sum.primary += sample.primary;
        sum.co2 += sample.co2;
    }

    let mean = EnergyValues {
        gas: sum.gas / n,
        elec: sum.elec / n,
        total: sum.total / n,
        primary: sum.primary / n,
        co2: sum.co2 / n,
    };

    let mut variance_sum = EnergyValues::zero();
    for sample in samples {
        variance_sum.gas += (sample.gas - mean.gas).powi(2);
        variance_sum.elec += (sample.elec - mean.elec).powi(2);
        variance_sum.total += (sample.total - mean.total).powi(2);
        variance_sum.primary += (sample.primary - mean.primary).powi(2);
        variance_sum.co2 += (sample.co2 - mean.co2).powi(2);
    }

    Ok(EnergyStats {
        mean: round_values(mean),
        std: round_values(EnergyValues {
            gas: (variance_sum.gas / n).sqrt(),
            elec: (variance_sum.elec / n).sqrt(),
            total: (variance_sum.total / n).sqrt(),
            primary: (variance_sum.primary / n).sqrt(),
            co2: (variance_sum.co2 / n).sqrt(),
        }),
    })
}

fn round_values(values: EnergyValues) -> EnergyValues {
    EnergyValues {
        gas: round2(values.gas),
        elec: round2(values.elec),
        total: round2(values.total),
        primary: round2(values.primary),
        co2: round2(values.co2),
    }
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
        RetrofitSpec, build_ann_inputs, converted_input_row, generate_uncertain_samples,
        inverse_standard_normal, retrofit_cost, retrofit_cost_config,
    };
    use crate::domain::{EmbeddedModelStore, EstimateRequest, ReferenceData, RetrofitOption};

    #[test]
    fn builds_25_dimensional_ann_inputs() {
        let reference_data = ReferenceData::load().expect("reference metadata should load");
        let uncertain_samples = generate_uncertain_samples(10);
        let converted = converted_input_row(&reference_data, false, 0, 0, &RetrofitSpec::default())
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
            metric: crate::domain::EnergyMetric::Ghg,
            options: vec![RetrofitOption {
                id: 1,
                label: "test".to_string(),
                spec: RetrofitSpec::default(),
            }],
        };

        let result = super::estimate_reduction(&request, &model_store, &reference_data)
            .expect("ANN estimate should succeed");

        assert_eq!(result.options.len(), 1);
        assert!(result.baseline.mean.elec.is_finite());
        assert!(result.baseline.std.elec.is_finite());
        assert!(result.options[0].after.mean.elec.is_finite());
        assert!(result.options[0].reduction.mean.co2.is_finite());
    }

    #[test]
    fn ports_python_retrofit_cost_formula() {
        assert_eq!(retrofit_cost(RetrofitSpec::default(), 1000.0), 0);

        let config = retrofit_cost_config();
        assert_eq!(config.schema_version, 1);
        assert_eq!(config.unit_costs.wall.round() as u64, 93_593);
        assert!((config.rates.tax - 0.1).abs() < f64::EPSILON);

        let spec = RetrofitSpec {
            wall: 2,
            window: 1,
            lights: true,
            ..Default::default()
        };
        let cost_per_m2 = retrofit_cost(spec, 1.0);
        let total_cost = retrofit_cost(spec, 1000.0);

        assert_eq!(cost_per_m2, 614_908);
        assert_eq!(total_cost, 614_908_244);
    }

    #[test]
    fn labels_window_levels_by_improvement_order() {
        assert_eq!(
            RetrofitSpec {
                window: 3,
                ..Default::default()
            }
            .summary_label(),
            "창호 현행"
        );
        assert_eq!(
            RetrofitSpec {
                window: 2,
                ..Default::default()
            }
            .summary_label(),
            "창호 2등급"
        );
        assert_eq!(
            RetrofitSpec {
                window: 1,
                ..Default::default()
            }
            .summary_label(),
            "창호 1등급"
        );
    }

    #[test]
    fn generates_full_pareto_candidate_grid_without_baseline() {
        let candidates = super::generate_pareto_candidates();

        assert_eq!(candidates.len(), 27_647);
        assert!(
            candidates
                .iter()
                .all(|candidate| candidate.spec.active_count() > 0)
        );
    }
}
