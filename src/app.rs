use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    time::Duration,
};

use eframe::egui::{
    self, Align2, Color32, FontData, FontDefinitions, FontFamily, Pos2, Rect, RichText, Sense,
    Shape, Stroke, Vec2,
};

use crate::domain::{
    BUILDING_TYPES, BinaryRetrofitMeasure, CLIMATES, ERAS, EmbeddedModelStore, EnergyMetric,
    EnergyStats, EnergyValues, EstimateRequest, EstimateResult, OptionEstimate,
    ParetoProgressEvent, ReferenceData, RetrofitOption, RetrofitSpec, estimate_reduction,
    run_pareto_job,
};

pub struct Co2App {
    selected_building: usize,
    selected_climate: usize,
    selected_era: usize,
    selected_metric: EnergyMetric,
    area_m2: f64,
    options: Vec<RetrofitOption>,
    next_option_id: u64,
    user_result: Option<EstimateResult>,
    single_result: Option<EstimateResult>,
    pareto_result: Option<EstimateResult>,
    error: Option<String>,
    model_store: Option<EmbeddedModelStore>,
    model_error: Option<String>,
    reference_data: Option<ReferenceData>,
    reference_error: Option<String>,
    model_blend_check: Option<String>,
    progress: f32,
    progress_message: String,
    progress_rx: Option<Receiver<ParetoProgressEvent>>,
    progress_cancel: Option<Arc<AtomicBool>>,
    is_running: bool,
}

impl Co2App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        apply_theme(&cc.egui_ctx);
        let (model_store, model_error) = match EmbeddedModelStore::load() {
            Ok(store) => (Some(store), None),
            Err(error) => (None, Some(error)),
        };
        let (reference_data, reference_error) = match ReferenceData::load() {
            Ok(data) => (Some(data), None),
            Err(error) => (None, Some(error)),
        };
        let model_blend_check = match (&model_store, &reference_data) {
            (Some(store), Some(data)) => {
                let result = data.model1_weight_for_base("Office").and_then(|weight| {
                    let inputs = vec![vec![0.0; 25]; 10];
                    store.predict_pair_split("Office", weight, &inputs)
                });
                match result {
                    Ok(outputs) => Some(format!("Office split sanity: {} samples", outputs.len())),
                    Err(error) => Some(format!("Office split sanity failed: {error}")),
                }
            }
            _ => None,
        };

        let mut app = Self {
            selected_building: 3,
            selected_climate: 0,
            selected_era: 2,
            selected_metric: EnergyMetric::Electricity,
            area_m2: 1000.0,
            options: vec![
                RetrofitOption {
                    id: 1,
                    label: "ALT 1".to_string(),
                    spec: RetrofitSpec {
                        wall: 1,
                        window: 1,
                        lights: true,
                        ..Default::default()
                    },
                },
                RetrofitOption {
                    id: 2,
                    label: "ALT 2".to_string(),
                    spec: RetrofitSpec {
                        roof: 1,
                        cooling: true,
                        pv: true,
                        ..Default::default()
                    },
                },
            ],
            next_option_id: 3,
            user_result: None,
            single_result: None,
            pareto_result: None,
            error: None,
            model_store,
            model_error,
            reference_data,
            reference_error,
            model_blend_check,
            progress: 0.0,
            progress_message: "Pareto 미실행".to_string(),
            progress_rx: None,
            progress_cancel: None,
            is_running: false,
        };
        app.refresh_user_outputs();
        app
    }

    fn request_with_options(&self, options: Vec<RetrofitOption>) -> EstimateRequest {
        EstimateRequest {
            building_type: BUILDING_TYPES[self.selected_building].code.to_string(),
            climate: CLIMATES[self.selected_climate].code.to_string(),
            era: ERAS[self.selected_era].code.to_string(),
            area_m2: self.area_m2,
            metric: self.selected_metric,
            options,
        }
    }

    fn user_signature(&self) -> String {
        let option_sig = self
            .options
            .iter()
            .map(|option| format!("{}:{:?}:{}", option.id, option.spec, option.label))
            .collect::<Vec<_>>()
            .join("|");
        format!(
            "{}|{}|{}|{:.3}|{}",
            self.selected_building,
            self.selected_climate,
            self.selected_era,
            self.area_m2,
            option_sig
        )
    }

    fn pareto_context_signature(&self) -> String {
        format!(
            "{}|{}|{}|{:.3}|{:?}",
            self.selected_building,
            self.selected_climate,
            self.selected_era,
            self.area_m2,
            self.selected_metric
        )
    }

    fn refresh_user_outputs(&mut self) {
        self.error = None;
        let mut all_options = self.options.clone();
        let user_count = all_options.len();
        all_options.extend(single_effect_options());
        let request = self.request_with_options(all_options);

        let estimate = match (&self.model_store, &self.reference_data) {
            (Some(model_store), Some(reference_data)) => {
                estimate_reduction(&request, model_store, reference_data)
            }
            (None, _) => Err(self
                .model_error
                .clone()
                .unwrap_or_else(|| "model store is not loaded".to_string())),
            (_, None) => Err(self
                .reference_error
                .clone()
                .unwrap_or_else(|| "reference data is not loaded".to_string())),
        };

        match estimate {
            Ok(mut result) => {
                let single_options = result.options.split_off(user_count);
                let baseline = result.baseline;
                self.user_result = Some(EstimateResult {
                    baseline,
                    options: result.options,
                });
                self.single_result = Some(EstimateResult {
                    baseline,
                    options: single_options,
                });
            }
            Err(error) => {
                self.user_result = None;
                self.single_result = None;
                self.error = Some(error);
            }
        }
    }

    fn start_pareto(&mut self) {
        self.error = None;
        self.pareto_result = None;
        self.progress_rx = None;
        self.progress_cancel = None;
        self.progress = 0.0;
        self.progress_message = "Pareto 계산 준비".to_string();

        let request = self.request_with_options(Vec::new());
        if let (Some(model_store), Some(reference_data)) = (&self.model_store, &self.reference_data)
        {
            let (sender, receiver) = mpsc::channel();
            let cancel = Arc::new(AtomicBool::new(false));
            run_pareto_job(
                request,
                model_store.clone(),
                reference_data.clone(),
                sender,
                cancel.clone(),
            );
            self.progress_rx = Some(receiver);
            self.progress_cancel = Some(cancel);
            self.is_running = true;
        } else {
            self.error = Some(
                self.model_error
                    .clone()
                    .or_else(|| self.reference_error.clone())
                    .unwrap_or_else(|| "model/reference data is not loaded".to_string()),
            );
        }
    }

    fn cancel_pareto(&mut self) {
        if let Some(cancel) = &self.progress_cancel {
            cancel.store(true, Ordering::Relaxed);
            self.progress_message = "중지 요청".to_string();
        }
    }

    fn invalidate_pareto(&mut self) {
        if self.is_running {
            self.cancel_pareto();
        }
        self.pareto_result = None;
        self.progress = 0.0;
        self.progress_message = "Pareto 갱신 필요".to_string();
    }

    fn poll_progress(&mut self, ctx: &egui::Context) {
        if let Some(receiver) = &self.progress_rx {
            while let Ok(event) = receiver.try_recv() {
                self.progress = event.progress;
                self.progress_message = event.message;
                if let Some(result) = event.result {
                    self.pareto_result = Some(result);
                }
                if let Some(error) = event.error {
                    self.error = Some(error);
                }
                if self.progress >= 1.0 {
                    self.is_running = false;
                    self.progress_cancel = None;
                }
            }
        }

        if self.is_running {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }

    fn add_option(&mut self) {
        let id = self.next_option_id;
        self.next_option_id += 1;
        self.options.push(RetrofitOption {
            id,
            label: format!("ALT {}", self.options.len() + 1),
            spec: RetrofitSpec::default(),
        });
    }

    fn duplicate_option(&mut self, index: usize) {
        let mut option = self.options[index].clone();
        option.id = self.next_option_id;
        option.label = format!("{} copy", option.label);
        self.next_option_id += 1;
        self.options.push(option);
    }
}

impl eframe::App for Co2App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_progress(&ctx);
        let previous_user_signature = self.user_signature();
        let previous_pareto_signature = self.pareto_context_signature();

        egui::Panel::top("topbar").show_inside(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("CO2 Reduction Finder").size(24.0).strong());
                    ui.label(
                        RichText::new("에너지 감축계수 조회 Dashboard")
                            .color(Color32::from_rgb(96, 119, 122)),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if self.is_running {
                        "중지"
                    } else {
                        "Pareto 갱신"
                    };
                    if ui
                        .add(egui::Button::new(label).min_size([108.0, 38.0].into()))
                        .clicked()
                    {
                        if self.is_running {
                            self.cancel_pareto();
                        } else {
                            self.start_pareto();
                        }
                    }
                });
            });
            ui.add_space(6.0);
        });

        egui::Panel::left("controls")
            .resizable(false)
            .exact_size(390.0)
            .show_inside(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.controls_ui(ui);
                });
            });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            self.dashboard_ui(ui);
        });

        if self.user_signature() != previous_user_signature {
            self.refresh_user_outputs();
        }
        if self.pareto_context_signature() != previous_pareto_signature {
            self.invalidate_pareto();
        }
    }
}

impl Co2App {
    fn controls_ui(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.heading("기준 조건");
        ui.add_space(8.0);
        self.model_status_ui(ui);
        ui.separator();

        labeled_combo(
            ui,
            "본과제용도분류",
            "building-type",
            &mut self.selected_building,
            BUILDING_TYPES.iter().map(|item| item.label),
        );
        let selected_type = BUILDING_TYPES[self.selected_building];
        ui.label(if selected_type.residential {
            "주거"
        } else {
            "비주거"
        });
        ui.add_space(8.0);
        labeled_combo(
            ui,
            "기후권",
            "climate-zone",
            &mut self.selected_climate,
            CLIMATES.iter().map(|item| item.label),
        );
        labeled_combo(
            ui,
            "준공연도",
            "era-band",
            &mut self.selected_era,
            ERAS.iter().map(|item| item.label),
        );

        ui.add_space(8.0);
        ui.label(
            RichText::new("연면적 m2")
                .strong()
                .color(Color32::from_rgb(82, 97, 100)),
        );
        ui.add(
            egui::DragValue::new(&mut self.area_m2)
                .speed(10.0)
                .range(1.0..=10_000_000.0)
                .fixed_decimals(1),
        );

        ui.separator();
        ui.horizontal(|ui| {
            ui.heading("비교안");
            if ui.button("+").on_hover_text("비교안 추가").clicked() {
                self.add_option();
            }
        });

        let mut duplicate_index = None;
        let mut remove_id = None;

        for index in 0..self.options.len() {
            ui.add_space(8.0);
            let can_remove = self.options.len() > 1;
            egui::Frame::default()
                .stroke(Stroke::new(1.0, Color32::from_rgb(216, 226, 226)))
                .fill(Color32::from_rgb(249, 251, 250))
                .corner_radius(6.0)
                .inner_margin(10.0)
                .show(ui, |ui| {
                    let option = &mut self.options[index];
                    ui.horizontal(|ui| {
                        ui.text_edit_singleline(&mut option.label);
                        if ui.button("Copy").clicked() {
                            duplicate_index = Some(index);
                        }
                        if ui
                            .add_enabled(can_remove, egui::Button::new("Del"))
                            .clicked()
                        {
                            remove_id = Some(option.id);
                        }
                    });

                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("외피")
                            .strong()
                            .color(Color32::from_rgb(82, 97, 100)),
                    );
                    level_combo(ui, "벽체", &mut option.spec.wall, &ENVELOPE_LEVELS);
                    level_combo(ui, "지붕", &mut option.spec.roof, &ENVELOPE_LEVELS);
                    level_combo(ui, "바닥", &mut option.spec.floor, &ENVELOPE_LEVELS);
                    level_combo(ui, "창호", &mut option.spec.window, &WINDOW_LEVELS);

                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("설비/전기/기타")
                            .strong()
                            .color(Color32::from_rgb(82, 97, 100)),
                    );
                    for chunk in BinaryRetrofitMeasure::ALL.chunks(4) {
                        ui.horizontal(|ui| {
                            for measure in chunk {
                                let mut enabled = option.spec.is_enabled(*measure);
                                if ui.checkbox(&mut enabled, measure.label()).changed() {
                                    option.spec.set_enabled(*measure, enabled);
                                }
                            }
                        });
                    }
                });
        }

        if let Some(index) = duplicate_index {
            self.duplicate_option(index);
        }
        if let Some(id) = remove_id {
            if self.options.len() > 1 {
                self.options.retain(|option| option.id != id);
            }
        }
    }

    fn dashboard_ui(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(8.0);
            self.metric_selector_ui(ui);
            ui.add_space(8.0);
            self.kpi_ui(ui);

            if let Some(error) = &self.error {
                ui.add_space(8.0);
                ui.colored_label(Color32::from_rgb(160, 54, 45), error);
            }

            ui.add_space(18.0);
            self.user_result_section(ui);
            ui.add_space(18.0);
            self.single_effect_section(ui);
            ui.add_space(18.0);
            self.pareto_section(ui);
            ui.add_space(18.0);
        });
    }

    fn metric_selector_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new("지표")
                    .strong()
                    .color(Color32::from_rgb(82, 97, 100)),
            );
            for metric in EnergyMetric::ALL {
                let factor = metric.factor();
                ui.selectable_value(&mut self.selected_metric, metric, factor.label);
            }
        });
        let factor = self.selected_metric.factor();
        ui.small(
            RichText::new(format!(
                "계수 G {:.5} / E {:.5} / {} -> {}",
                factor.gas, factor.elec, factor.per_area_unit, factor.unit
            ))
            .color(Color32::from_rgb(96, 119, 122)),
        );
    }

    fn user_result_section(&self, ui: &mut egui::Ui) {
        section_header(ui, "사용자 입력 결과");
        if let Some(result) = &self.user_result {
            let series = std::iter::once(("리모델링 이전".to_string(), result.baseline))
                .chain(
                    result
                        .options
                        .iter()
                        .map(|item| (item.label.clone(), item.after)),
                )
                .collect::<Vec<_>>();
            density_chart(
                ui,
                "user-density",
                &series,
                self.selected_metric,
                self.area_m2,
            );
            ui.add_space(8.0);

            let bars = result
                .options
                .iter()
                .map(|item| BarItem {
                    label: item.label.clone(),
                    value: metric_display_value(
                        self.selected_metric,
                        item.reduction.mean,
                        self.area_m2,
                    ),
                    cost: Some(item.cost),
                })
                .collect::<Vec<_>>();
            horizontal_bar_chart(
                ui,
                "user-bars",
                &bars,
                self.selected_metric.factor().unit,
                180.0,
            );
            ui.add_space(8.0);
            result_table(
                ui,
                "user-result-table",
                result,
                self.selected_metric,
                self.area_m2,
            );
        } else {
            ui.label("결과를 계산할 수 없습니다.");
        }
    }

    fn single_effect_section(&self, ui: &mut egui::Ui) {
        section_header(ui, "단일 요소기술 적용 효과");
        if let Some(result) = &self.single_result {
            let bars = result
                .options
                .iter()
                .map(|item| BarItem {
                    label: item.label.clone(),
                    value: metric_display_value(
                        self.selected_metric,
                        item.reduction.mean,
                        self.area_m2,
                    ),
                    cost: Some(item.cost),
                })
                .collect::<Vec<_>>();
            horizontal_bar_chart(
                ui,
                "single-effect-bars",
                &bars,
                self.selected_metric.factor().unit,
                360.0,
            );
            ui.add_space(8.0);
            compact_result_table(
                ui,
                "single-effect-table",
                result,
                self.selected_metric,
                self.area_m2,
            );
        } else {
            ui.label("단일 요소기술 결과를 계산할 수 없습니다.");
        }
    }

    fn pareto_section(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            section_header(ui, "Pareto 결과");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(&self.progress_message);
            });
        });
        ui.add(egui::ProgressBar::new(self.progress).show_percentage());
        ui.add_space(8.0);

        if let Some(result) = &self.pareto_result {
            scatter_chart(
                ui,
                "pareto-scatter",
                result,
                self.selected_metric,
                self.area_m2,
            );
            ui.add_space(8.0);
            result_table(
                ui,
                "pareto-table",
                result,
                self.selected_metric,
                self.area_m2,
            );
        } else {
            empty_chart(
                ui,
                "pareto-empty",
                "Pareto 갱신을 실행하면 최적 조합이 표시됩니다.",
            );
        }
    }

    fn model_status_ui(&self, ui: &mut egui::Ui) {
        if let Some(store) = &self.model_store {
            ui.label(
                RichText::new("모델 asset")
                    .strong()
                    .color(Color32::from_rgb(82, 97, 100)),
            );
            ui.label(format!(
                "{} models / {:.2}M params",
                store.len(),
                store.total_params() as f64 / 1_000_000.0
            ));

            let selected_type = BUILDING_TYPES[self.selected_building];
            let model_1 = format!("{}_1", selected_type.code);
            let model_2 = format!("{}_2", selected_type.code);
            let status = match (store.get(&model_1), store.get(&model_2)) {
                (Some(first), Some(second)) => {
                    let weight_prefix = self
                        .reference_data
                        .as_ref()
                        .and_then(|data| {
                            Some((data.get(&model_1)?.weight, data.get(&model_2)?.weight))
                        })
                        .map(|(w1, w2)| format!("weights {:.3}/{:.3}; ", w1, w2))
                        .unwrap_or_default();
                    let metadata_prefix = self
                        .reference_data
                        .as_ref()
                        .and_then(|data| data.get(&model_1))
                        .map(|metadata| {
                            format!(
                                "{} / {} / {} / {:.1}m2; ",
                                metadata.korean_name,
                                if metadata.residential {
                                    "주거"
                                } else {
                                    "비주거"
                                },
                                if metadata.gas_heating {
                                    "gas heat"
                                } else {
                                    "non-gas heat"
                                },
                                metadata.area
                            )
                        })
                        .unwrap_or_default();
                    format!(
                        "{}{}{}: {}d -> {}d, {}: {}d -> {}d",
                        metadata_prefix,
                        weight_prefix,
                        model_1,
                        first.input_dim,
                        first.output_dim,
                        model_2,
                        second.input_dim,
                        second.output_dim
                    )
                }
                _ => format!("{} / {} not fully mapped", model_1, model_2),
            };
            ui.small(status);
            ui.add_space(8.0);
        } else if let Some(error) = &self.model_error {
            ui.colored_label(
                Color32::from_rgb(160, 54, 45),
                format!("model load failed: {error}"),
            );
            ui.add_space(8.0);
        }

        if let Some(data) = &self.reference_data {
            ui.small(format!(
                "metadata: {} info rows / {} Umap rows",
                data.info_len(),
                data.umap_rows()
            ));
            if let Some(check) = &self.model_blend_check {
                ui.small(check);
            }
            ui.add_space(8.0);
        } else if let Some(error) = &self.reference_error {
            ui.colored_label(
                Color32::from_rgb(160, 54, 45),
                format!("metadata load failed: {error}"),
            );
            ui.add_space(8.0);
        }
    }

    fn kpi_ui(&self, ui: &mut egui::Ui) {
        let factor = self.selected_metric.factor();
        let baseline = self.user_result.as_ref().map(|result| {
            metric_display_value(self.selected_metric, result.baseline.mean, self.area_m2)
        });
        let baseline_std = self.user_result.as_ref().map(|result| {
            metric_display_value(self.selected_metric, result.baseline.std, self.area_m2)
        });
        let best = self.user_result.as_ref().and_then(|result| {
            result.options.iter().max_by(|a, b| {
                metric_display_value(self.selected_metric, a.reduction.mean, self.area_m2)
                    .total_cmp(&metric_display_value(
                        self.selected_metric,
                        b.reduction.mean,
                        self.area_m2,
                    ))
            })
        });
        let best_reduction = best.map(|item| {
            metric_display_value(self.selected_metric, item.reduction.mean, self.area_m2)
        });
        let best_rate = baseline
            .zip(best_reduction)
            .and_then(|(before, reduction)| {
                (before.abs() > f64::EPSILON).then_some(reduction / before)
            });

        ui.columns(2, |columns| {
            kpi_cell(&mut columns[0], "리모델링 이전", baseline, factor.unit);
            kpi_cell(&mut columns[1], "표준편차", baseline_std, factor.unit);
        });
        ui.add_space(8.0);
        ui.columns(2, |columns| {
            kpi_cell(&mut columns[0], "최대 감축", best_reduction, factor.unit);
            kpi_cell(
                &mut columns[1],
                "최대 감축률",
                best_rate.map(|value| value * 100.0),
                "%",
            );
        });
    }
}

#[derive(Debug, Clone)]
struct BarItem {
    label: String,
    value: f64,
    cost: Option<u64>,
}

fn single_effect_options() -> Vec<RetrofitOption> {
    let mut id = 50_000;
    let mut options = Vec::new();
    let mut push = |label: &str, spec: RetrofitSpec| {
        options.push(RetrofitOption {
            id,
            label: label.to_string(),
            spec,
        });
        id += 1;
    };

    push(
        "벽체 현행",
        RetrofitSpec {
            wall: 1,
            ..Default::default()
        },
    );
    push(
        "벽체 강화",
        RetrofitSpec {
            wall: 2,
            ..Default::default()
        },
    );
    push(
        "지붕 현행",
        RetrofitSpec {
            roof: 1,
            ..Default::default()
        },
    );
    push(
        "지붕 강화",
        RetrofitSpec {
            roof: 2,
            ..Default::default()
        },
    );
    push(
        "바닥 현행",
        RetrofitSpec {
            floor: 1,
            ..Default::default()
        },
    );
    push(
        "바닥 강화",
        RetrofitSpec {
            floor: 2,
            ..Default::default()
        },
    );
    push(
        "창호 1등급",
        RetrofitSpec {
            window: 1,
            ..Default::default()
        },
    );
    push(
        "창호 2등급",
        RetrofitSpec {
            window: 2,
            ..Default::default()
        },
    );
    push(
        "창호 3등급",
        RetrofitSpec {
            window: 3,
            ..Default::default()
        },
    );
    push(
        "고효율 냉방기기",
        RetrofitSpec {
            cooling: true,
            ..Default::default()
        },
    );
    push(
        "고효율 난방기기",
        RetrofitSpec {
            heating: true,
            ..Default::default()
        },
    );
    push(
        "전열교환기",
        RetrofitSpec {
            hx: true,
            ..Default::default()
        },
    );
    push(
        "조명 교체",
        RetrofitSpec {
            lights: true,
            ..Default::default()
        },
    );
    push(
        "고효율 급탕보일러",
        RetrofitSpec {
            hw_boiler: true,
            ..Default::default()
        },
    );
    push(
        "쿨루프",
        RetrofitSpec {
            coolroof: true,
            ..Default::default()
        },
    );
    push(
        "일사조절장치",
        RetrofitSpec {
            blind: true,
            ..Default::default()
        },
    );
    push(
        "태양광",
        RetrofitSpec {
            pv: true,
            ..Default::default()
        },
    );

    options
}

fn section_header(ui: &mut egui::Ui, title: &str) {
    ui.heading(title);
}

fn density_chart(
    ui: &mut egui::Ui,
    id: &str,
    series: &[(String, EnergyStats)],
    metric: EnergyMetric,
    area_m2: f64,
) {
    ui.push_id(id, |ui| {
        let desired = Vec2::new(ui.available_width(), 240.0);
        let (rect, _) = ui.allocate_exact_size(desired, Sense::hover());
        let painter = ui.painter_at(rect);
        chart_background(&painter, rect);

        if series.is_empty() {
            return;
        }

        let mut min_x = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = 0.0_f64;
        for (_, stats) in series {
            let mean = metric_display_value(metric, stats.mean, area_m2);
            let std = metric_display_value(metric, stats.std, area_m2)
                .abs()
                .max(0.001);
            min_x = min_x.min(mean - std * 3.0);
            max_x = max_x.max(mean + std * 3.0);
            max_y = max_y.max(normal_pdf(mean, mean, std));
        }

        if !min_x.is_finite() || !max_x.is_finite() || (max_x - min_x).abs() < f64::EPSILON {
            return;
        }

        let plot = plot_rect(rect, 44.0, 14.0, 22.0, 26.0);
        draw_axes(&painter, plot);
        let colors = [
            Color32::from_rgb(39, 118, 124),
            Color32::from_rgb(218, 105, 78),
            Color32::from_rgb(72, 132, 83),
            Color32::from_rgb(145, 91, 166),
            Color32::from_rgb(191, 143, 49),
        ];

        for (index, (label, stats)) in series.iter().enumerate() {
            let mean = metric_display_value(metric, stats.mean, area_m2);
            let std = metric_display_value(metric, stats.std, area_m2)
                .abs()
                .max(0.001);
            let color = colors[index % colors.len()];
            let mut points = Vec::with_capacity(96);
            for step in 0..96 {
                let t = step as f64 / 95.0;
                let x = min_x + (max_x - min_x) * t;
                let y = normal_pdf(x, mean, std);
                points.push(Pos2::new(
                    lerp(
                        plot.left(),
                        plot.right(),
                        ((x - min_x) / (max_x - min_x)) as f32,
                    ),
                    lerp(plot.bottom(), plot.top(), (y / max_y.max(0.001)) as f32),
                ));
            }
            painter.add(Shape::line(points, Stroke::new(2.0, color)));

            let legend_y = rect.top() + 14.0 + (index % 3) as f32 * 18.0;
            let legend_x = rect.left() + 16.0 + (index / 3) as f32 * 180.0;
            painter.rect_filled(
                Rect::from_min_size(Pos2::new(legend_x, legend_y + 4.0), Vec2::new(18.0, 4.0)),
                2.0,
                color,
            );
            painter.text(
                Pos2::new(legend_x + 24.0, legend_y + 6.0),
                Align2::LEFT_CENTER,
                short_label(label, 18),
                egui::FontId::proportional(12.0),
                Color32::from_rgb(67, 78, 82),
            );
        }

        let factor = metric.factor();
        painter.text(
            Pos2::new(plot.left(), rect.bottom() - 12.0),
            Align2::LEFT_CENTER,
            format!("배출/사용 분포 [{}]", factor.unit),
            egui::FontId::proportional(12.0),
            Color32::from_rgb(96, 119, 122),
        );
    });
}

fn horizontal_bar_chart(
    ui: &mut egui::Ui,
    id: &str,
    items: &[BarItem],
    unit: &str,
    min_height: f32,
) {
    ui.push_id(id, |ui| {
        let height = min_height.max(items.len() as f32 * 26.0 + 42.0);
        let desired = Vec2::new(ui.available_width(), height);
        let (rect, _) = ui.allocate_exact_size(desired, Sense::hover());
        let painter = ui.painter_at(rect);
        chart_background(&painter, rect);

        if items.is_empty() {
            return;
        }

        let plot = plot_rect(rect, 132.0, 76.0, 18.0, 28.0);
        draw_axes(&painter, plot);
        let min_value = items.iter().map(|item| item.value).fold(0.0_f64, f64::min);
        let max_value = items.iter().map(|item| item.value).fold(0.0_f64, f64::max);
        let span = (max_value - min_value).abs().max(1.0);
        let zero_x = lerp(plot.left(), plot.right(), ((0.0 - min_value) / span) as f32);
        painter.line_segment(
            [
                Pos2::new(zero_x, plot.top()),
                Pos2::new(zero_x, plot.bottom()),
            ],
            Stroke::new(1.0, Color32::from_rgb(165, 178, 180)),
        );

        let row_h = (plot.height() / items.len() as f32).max(20.0);
        for (index, item) in items.iter().enumerate() {
            let y = plot.top() + row_h * (index as f32 + 0.5);
            let value_x = lerp(
                plot.left(),
                plot.right(),
                ((item.value - min_value) / span) as f32,
            );
            let left = zero_x.min(value_x);
            let right = zero_x.max(value_x);
            let color = if item.value >= 0.0 {
                Color32::from_rgb(42, 128, 122)
            } else {
                Color32::from_rgb(190, 87, 75)
            };
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(left, y - row_h * 0.28),
                    Pos2::new(right.max(left + 2.0), y + row_h * 0.28),
                ),
                3.0,
                color,
            );
            painter.text(
                Pos2::new(rect.left() + 14.0, y),
                Align2::LEFT_CENTER,
                short_label(&item.label, 15),
                egui::FontId::proportional(12.0),
                Color32::from_rgb(67, 78, 82),
            );
            painter.text(
                Pos2::new(plot.right() + 8.0, y),
                Align2::LEFT_CENTER,
                match item.cost {
                    Some(cost) => format!(
                        "{} {} / {}",
                        format_number(item.value),
                        unit,
                        format_cost(cost)
                    ),
                    None => format!("{} {}", format_number(item.value), unit),
                },
                egui::FontId::proportional(12.0),
                Color32::from_rgb(67, 78, 82),
            );
        }
    });
}

fn scatter_chart(
    ui: &mut egui::Ui,
    id: &str,
    result: &EstimateResult,
    metric: EnergyMetric,
    area_m2: f64,
) {
    ui.push_id(id, |ui| {
        let desired = Vec2::new(ui.available_width(), 260.0);
        let (rect, _) = ui.allocate_exact_size(desired, Sense::hover());
        let painter = ui.painter_at(rect);
        chart_background(&painter, rect);
        let plot = plot_rect(rect, 58.0, 28.0, 18.0, 42.0);
        draw_axes(&painter, plot);

        if result.options.is_empty() {
            return;
        }

        let max_cost = result
            .options
            .iter()
            .map(|item| item.cost as f64 / 10_000.0)
            .fold(0.0, f64::max)
            .max(1.0);
        let max_reduction = result
            .options
            .iter()
            .map(|item| metric_display_value(metric, item.reduction.mean, area_m2))
            .fold(0.0, f64::max)
            .max(1.0);

        for item in &result.options {
            let x_value = item.cost as f64 / 10_000.0;
            let y_value = metric_display_value(metric, item.reduction.mean, area_m2);
            let point = Pos2::new(
                lerp(plot.left(), plot.right(), (x_value / max_cost) as f32),
                lerp(plot.bottom(), plot.top(), (y_value / max_reduction) as f32),
            );
            painter.circle_filled(point, 4.5, Color32::from_rgb(39, 118, 124));
            painter.text(
                point + Vec2::new(6.0, -6.0),
                Align2::LEFT_CENTER,
                short_label(&item.label, 12),
                egui::FontId::proportional(11.0),
                Color32::from_rgb(67, 78, 82),
            );
        }

        let factor = metric.factor();
        painter.text(
            Pos2::new(plot.left(), rect.bottom() - 16.0),
            Align2::LEFT_CENTER,
            "공사비 [만원]",
            egui::FontId::proportional(12.0),
            Color32::from_rgb(96, 119, 122),
        );
        painter.text(
            Pos2::new(rect.left() + 10.0, plot.top() - 4.0),
            Align2::LEFT_TOP,
            format!("감축량 [{}]", factor.unit),
            egui::FontId::proportional(12.0),
            Color32::from_rgb(96, 119, 122),
        );
    });
}

fn empty_chart(ui: &mut egui::Ui, id: &str, message: &str) {
    ui.push_id(id, |ui| {
        let desired = Vec2::new(ui.available_width(), 180.0);
        let (rect, _) = ui.allocate_exact_size(desired, Sense::hover());
        let painter = ui.painter_at(rect);
        chart_background(&painter, rect);
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            message,
            egui::FontId::proportional(14.0),
            Color32::from_rgb(96, 119, 122),
        );
    });
}

fn result_table(
    ui: &mut egui::Ui,
    id: &str,
    result: &EstimateResult,
    metric: EnergyMetric,
    area_m2: f64,
) {
    egui::Grid::new(id)
        .striped(true)
        .min_col_width(86.0)
        .show(ui, |ui| {
            ui.strong("Option");
            ui.strong("Before");
            ui.strong("After");
            ui.strong("Reduction");
            ui.strong("Rate");
            ui.strong("Std");
            ui.strong("Cost");
            ui.end_row();

            let before = metric_display_value(metric, result.baseline.mean, area_m2);
            for item in &result.options {
                option_row(ui, item, before, metric, area_m2, true);
            }
        });
}

fn compact_result_table(
    ui: &mut egui::Ui,
    id: &str,
    result: &EstimateResult,
    metric: EnergyMetric,
    area_m2: f64,
) {
    egui::Grid::new(id)
        .striped(true)
        .min_col_width(86.0)
        .show(ui, |ui| {
            ui.strong("요소기술");
            ui.strong("감축량");
            ui.strong("감축률");
            ui.strong("공사비");
            ui.end_row();

            let before = metric_display_value(metric, result.baseline.mean, area_m2);
            for item in &result.options {
                option_row(ui, item, before, metric, area_m2, false);
            }
        });
}

fn option_row(
    ui: &mut egui::Ui,
    item: &OptionEstimate,
    before: f64,
    metric: EnergyMetric,
    area_m2: f64,
    full: bool,
) {
    let after = metric_display_value(metric, item.after.mean, area_m2);
    let reduction = metric_display_value(metric, item.reduction.mean, area_m2);
    let std = metric_display_value(metric, item.reduction.std, area_m2).abs();
    let rate = if before.abs() > f64::EPSILON {
        reduction / before * 100.0
    } else {
        0.0
    };

    ui.push_id(item.id, |ui| {
        ui.label(short_label(&item.label, 24));
    });
    if full {
        ui.label(format_number(before));
        ui.label(format_number(after));
        ui.label(format_number(reduction));
        ui.label(format!("{rate:.1}%"));
        ui.label(format_number(std));
        ui.label(format_cost(item.cost));
    } else {
        ui.label(format_number(reduction));
        ui.label(format!("{rate:.1}%"));
        ui.label(format_cost(item.cost));
    }
    ui.end_row();
}

fn metric_display_value(metric: EnergyMetric, values: EnergyValues, area_m2: f64) -> f64 {
    let per_area = match metric {
        EnergyMetric::Electricity => values.elec,
        EnergyMetric::Gas => values.gas,
        EnergyMetric::FinalEnergy => values.total,
        EnergyMetric::PrimaryEnergy => values.primary,
        EnergyMetric::Ghg => values.co2,
    };
    per_area * area_m2 / 1000.0
}

fn chart_background(painter: &egui::Painter, rect: Rect) {
    painter.rect_filled(rect, 6.0, Color32::WHITE);
    painter.rect_stroke(
        rect,
        6.0,
        Stroke::new(1.0, Color32::from_rgb(216, 226, 226)),
        egui::StrokeKind::Inside,
    );
}

fn plot_rect(rect: Rect, left: f32, right: f32, top: f32, bottom: f32) -> Rect {
    Rect::from_min_max(
        Pos2::new(rect.left() + left, rect.top() + top),
        Pos2::new(rect.right() - right, rect.bottom() - bottom),
    )
}

fn draw_axes(painter: &egui::Painter, rect: Rect) {
    painter.line_segment(
        [
            Pos2::new(rect.left(), rect.bottom()),
            Pos2::new(rect.right(), rect.bottom()),
        ],
        Stroke::new(1.0, Color32::from_rgb(190, 202, 204)),
    );
    painter.line_segment(
        [
            Pos2::new(rect.left(), rect.top()),
            Pos2::new(rect.left(), rect.bottom()),
        ],
        Stroke::new(1.0, Color32::from_rgb(190, 202, 204)),
    );
}

fn normal_pdf(x: f64, mean: f64, std: f64) -> f64 {
    let variance = std * std;
    (-((x - mean).powi(2)) / (2.0 * variance)).exp() / (std * (2.0 * std::f64::consts::PI).sqrt())
}

fn lerp(start: f32, end: f32, t: f32) -> f32 {
    start + (end - start) * t.clamp(0.0, 1.0)
}

fn format_number(value: f64) -> String {
    if value.abs() >= 100.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.2}")
    }
}

fn format_cost(cost: u64) -> String {
    format!("{:.1} 만원", cost as f64 / 10_000.0)
}

fn short_label(label: &str, max_chars: usize) -> String {
    let count = label.chars().count();
    if count <= max_chars {
        return label.to_string();
    }
    let mut output = label
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    output.push('…');
    output
}

fn labeled_combo<'a>(
    ui: &mut egui::Ui,
    label: &str,
    id: &str,
    selected: &mut usize,
    labels: impl Iterator<Item = &'a str>,
) {
    let labels = labels.collect::<Vec<_>>();
    ui.label(
        RichText::new(label)
            .strong()
            .color(Color32::from_rgb(82, 97, 100)),
    );
    egui::ComboBox::from_id_salt(id)
        .selected_text(labels[*selected])
        .width(230.0)
        .show_ui(ui, |ui| {
            for (index, item_label) in labels.iter().enumerate() {
                ui.selectable_value(selected, index, *item_label);
            }
        });
    ui.add_space(8.0);
}

const ENVELOPE_LEVELS: [(u8, &str); 3] = [(0, "기존"), (1, "현행"), (2, "강화")];
const WINDOW_LEVELS: [(u8, &str); 4] = [(0, "기존"), (1, "창호1"), (2, "창호2"), (3, "창호3")];

fn level_combo(ui: &mut egui::Ui, label: &str, selected: &mut u8, levels: &[(u8, &str)]) {
    let selected_text = levels
        .iter()
        .find(|(value, _)| value == selected)
        .map(|(_, label)| *label)
        .unwrap_or("기존");

    ui.horizontal(|ui| {
        ui.label(label);
        egui::ComboBox::from_id_salt((label, selected as *const u8 as usize))
            .selected_text(selected_text)
            .width(96.0)
            .show_ui(ui, |ui| {
                for (value, item_label) in levels {
                    ui.selectable_value(selected, *value, *item_label);
                }
            });
    });
}

fn kpi_cell(ui: &mut egui::Ui, label: &str, value: Option<f64>, unit: &str) {
    egui::Frame::default()
        .stroke(Stroke::new(1.0, Color32::from_rgb(216, 226, 226)))
        .fill(Color32::WHITE)
        .corner_radius(6.0)
        .inner_margin(12.0)
        .show(ui, |ui| {
            ui.label(
                RichText::new(label)
                    .strong()
                    .color(Color32::from_rgb(82, 97, 100)),
            );
            ui.label(
                RichText::new(value.map(format_number).unwrap_or_else(|| "-".to_string()))
                    .size(24.0),
            );
            ui.small(unit);
        });
}

fn apply_theme(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "pretendard".to_string(),
        Arc::new(FontData::from_static(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/fonts/Pretendard-Regular.ttf"
        )))),
    );
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "pretendard".to_string());
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .push("pretendard".to_string());
    ctx.set_fonts(fonts);

    let mut visuals = egui::Visuals::light();
    visuals.panel_fill = Color32::from_rgb(245, 247, 248);
    visuals.window_fill = Color32::WHITE;
    visuals.widgets.active.bg_fill = Color32::from_rgb(40, 122, 112);
    visuals.selection.bg_fill = Color32::from_rgb(40, 122, 112);
    ctx.set_visuals(visuals);
}
