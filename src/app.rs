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
    ParetoProgressEvent, ParetoResultSet, ReferenceData, RetrofitOption, RetrofitSpec,
    estimate_reduction, run_pareto_job,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DashboardTab {
    Alternatives,
    BaselineAnalysis,
}

pub struct Co2App {
    selected_building: usize,
    selected_climate: usize,
    selected_era: usize,
    selected_metric: EnergyMetric,
    area_m2: f64,
    options: Vec<RetrofitOption>,
    next_option_id: u64,
    active_tab: DashboardTab,
    user_result: Option<EstimateResult>,
    single_result: Option<EstimateResult>,
    pareto_results: Option<ParetoResultSet>,
    error: Option<String>,
    model_store: Option<EmbeddedModelStore>,
    model_error: Option<String>,
    reference_data: Option<ReferenceData>,
    reference_error: Option<String>,
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
            active_tab: DashboardTab::Alternatives,
            user_result: None,
            single_result: None,
            pareto_results: None,
            error: None,
            model_store,
            model_error,
            reference_data,
            reference_error,
            progress: 0.0,
            progress_message: "Pareto 미실행".to_string(),
            progress_rx: None,
            progress_cancel: None,
            is_running: false,
        };
        app.refresh_user_outputs();
        app.start_pareto();
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
            "{}|{}|{}|{:.3}",
            self.selected_building, self.selected_climate, self.selected_era, self.area_m2
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
        self.pareto_results = None;
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

    fn refresh_pareto(&mut self) {
        if self.is_running {
            self.cancel_pareto();
        }
        self.start_pareto();
    }

    fn poll_progress(&mut self, ctx: &egui::Context) {
        if let Some(receiver) = &self.progress_rx {
            while let Ok(event) = receiver.try_recv() {
                self.progress = event.progress;
                self.progress_message = event.message;
                if let Some(result) = event.result {
                    self.pareto_results = Some(result);
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

    fn pareto_result_for_metric(&self) -> Option<&EstimateResult> {
        self.pareto_results
            .as_ref()
            .and_then(|results| results.result_for(self.selected_metric))
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
                    if self.is_running {
                        ui.label(RichText::new(&self.progress_message).color(MUTED_TEXT));
                    }
                });
            });
            ui.add_space(6.0);
        });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            self.dashboard_ui(ui);
        });

        if self.user_signature() != previous_user_signature {
            self.refresh_user_outputs();
        }
        if self.pareto_context_signature() != previous_pareto_signature {
            self.refresh_pareto();
        }
    }
}

impl Co2App {
    fn input_area_ui(&mut self, ui: &mut egui::Ui) {
        ui.add_space(10.0);
        ui.horizontal_top(|ui| {
            ui.set_min_height(270.0);
            ui.vertical(|ui| {
                ui.set_width(340.0);
                self.building_info_ui(ui);
            });
            ui.add_space(16.0);
            ui.vertical(|ui| {
                self.comparison_options_ui(ui);
            });
        });
        ui.add_space(12.0);
    }

    fn building_info_ui(&mut self, ui: &mut egui::Ui) {
        section_header(ui, "건물 정보");
        ui.add_space(8.0);
        self.data_status_ui(ui);

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
    }

    fn comparison_options_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            section_header(ui, "비교안");
            if ui.button("+").on_hover_text("비교안 추가").clicked() {
                self.add_option();
            }
        });
        ui.add_space(8.0);

        let mut duplicate_index = None;
        let mut remove_id = None;
        let can_remove = self.options.len() > 1;

        egui::Frame::default()
            .stroke(Stroke::new(1.0, Color32::from_rgb(216, 226, 226)))
            .fill(Color32::WHITE)
            .corner_radius(6.0)
            .inner_margin(8.0)
            .show(ui, |ui| {
                egui::ScrollArea::horizontal().show(ui, |ui| {
                    egui::Grid::new("option-input-table")
                        .striped(true)
                        .min_col_width(96.0)
                        .show(ui, |ui| {
                            ui.strong("항목");
                            for option in &mut self.options {
                                ui.add(
                                    egui::TextEdit::singleline(&mut option.label)
                                        .desired_width(104.0),
                                );
                            }
                            ui.end_row();

                            ui.label("");
                            for index in 0..self.options.len() {
                                ui.horizontal(|ui| {
                                    if ui.small_button("Copy").clicked() {
                                        duplicate_index = Some(index);
                                    }
                                    if ui
                                        .add_enabled(can_remove, egui::Button::new("Del").small())
                                        .clicked()
                                    {
                                        remove_id = Some(self.options[index].id);
                                    }
                                });
                            }
                            ui.end_row();

                            option_level_row(
                                ui,
                                "벽체",
                                "wall",
                                &mut self.options,
                                spec_wall,
                                &ENVELOPE_LEVELS,
                            );
                            option_level_row(
                                ui,
                                "지붕",
                                "roof",
                                &mut self.options,
                                spec_roof,
                                &ENVELOPE_LEVELS,
                            );
                            option_level_row(
                                ui,
                                "바닥",
                                "floor",
                                &mut self.options,
                                spec_floor,
                                &ENVELOPE_LEVELS,
                            );
                            option_level_row(
                                ui,
                                "창호",
                                "window",
                                &mut self.options,
                                spec_window,
                                &WINDOW_LEVELS,
                            );

                            for measure in BinaryRetrofitMeasure::ALL {
                                option_measure_row(ui, measure, &mut self.options);
                            }
                        });
                });
            });

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
            self.input_area_ui(ui);
            ui.separator();
            ui.add_space(10.0);
            self.metric_selector_ui(ui);

            if let Some(error) = &self.error {
                ui.add_space(8.0);
                ui.colored_label(Color32::from_rgb(160, 54, 45), error);
            }

            ui.add_space(12.0);
            self.tab_selector_ui(ui);
            ui.add_space(12.0);
            match self.active_tab {
                DashboardTab::Alternatives => self.alternatives_tab_ui(ui),
                DashboardTab::BaselineAnalysis => self.baseline_analysis_tab_ui(ui),
            }
            ui.add_space(18.0);
        });
    }

    fn tab_selector_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            tab_button(
                ui,
                &mut self.active_tab,
                DashboardTab::Alternatives,
                "리모델링 대안 비교",
            );
            tab_button(
                ui,
                &mut self.active_tab,
                DashboardTab::BaselineAnalysis,
                "기준 조건 분석",
            );
        });
    }

    fn alternatives_tab_ui(&self, ui: &mut egui::Ui) {
        section_header(ui, "리모델링 대안 비교");
        ui.add_space(8.0);
        self.kpi_ui(ui);
        ui.add_space(16.0);
        self.user_result_section(ui);
    }

    fn baseline_analysis_tab_ui(&self, ui: &mut egui::Ui) {
        section_header(ui, "기준 조건 분석");
        ui.add_space(10.0);
        self.single_effect_section(ui);
        ui.add_space(18.0);
        self.pareto_section(ui);
    }

    fn metric_selector_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("지표").size(16.0).strong());
            for metric in EnergyMetric::ALL {
                let factor = metric.factor();
                let selected = self.selected_metric == metric;
                let text = if selected {
                    RichText::new(factor.label).strong().color(Color32::WHITE)
                } else {
                    RichText::new(factor.label).color(Color32::from_rgb(46, 58, 62))
                };
                let button = egui::Button::new(text)
                    .fill(if selected {
                        Color32::from_rgb(30, 109, 103)
                    } else {
                        Color32::from_rgb(232, 238, 238)
                    })
                    .stroke(Stroke::new(
                        1.0,
                        if selected {
                            Color32::from_rgb(30, 109, 103)
                        } else {
                            Color32::from_rgb(196, 211, 212)
                        },
                    ));
                if ui.add(button).clicked() {
                    self.selected_metric = metric;
                }
            }
        });
    }

    fn user_result_section(&self, ui: &mut egui::Ui) {
        if let Some(result) = &self.user_result {
            subsection_label(ui, "확률분포");
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

            subsection_label(ui, "감축량 및 공사비");
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
        } else {
            ui.label("결과를 계산할 수 없습니다.");
        }
    }

    fn single_effect_section(&self, ui: &mut egui::Ui) {
        section_header(ui, "단일 요소기술 적용 효과");
        if let Some(result) = &self.single_result {
            let baseline =
                metric_display_value(self.selected_metric, result.baseline.mean, self.area_m2);
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
                    cost: None,
                })
                .map(|bar| EffectBarItem {
                    rate: reduction_rate(baseline, bar.value),
                    reduction: bar.value,
                    label: bar.label,
                })
                .collect::<Vec<_>>();
            single_effect_bar_chart(
                ui,
                "single-effect-bars",
                &bars,
                self.selected_metric.factor().unit,
                baseline,
                360.0,
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

        if let Some(result) = self.pareto_result_for_metric() {
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
            let message = if self.is_running {
                "Pareto 계산 중입니다."
            } else {
                "건물 조건이 바뀌면 Pareto가 자동 갱신됩니다."
            };
            empty_chart(ui, "pareto-empty", message);
        }
    }

    fn data_status_ui(&self, ui: &mut egui::Ui) {
        if let Some(error) = &self.model_error {
            ui.colored_label(
                Color32::from_rgb(160, 54, 45),
                format!("model load failed: {error}"),
            );
            ui.add_space(8.0);
        }

        if let Some(error) = &self.reference_error {
            ui.colored_label(
                Color32::from_rgb(160, 54, 45),
                format!("metadata load failed: {error}"),
            );
            ui.add_space(8.0);
        }

        if self.model_error.is_none() && self.reference_error.is_none() {
            ui.small(RichText::new("계산 데이터 준비됨").color(Color32::from_rgb(96, 119, 122)));
            ui.add_space(8.0);
        }
    }

    fn kpi_ui(&self, ui: &mut egui::Ui) {
        let factor = self.selected_metric.factor();
        egui::Frame::default()
            .stroke(Stroke::new(1.0, Color32::from_rgb(216, 226, 226)))
            .fill(Color32::WHITE)
            .corner_radius(6.0)
            .inner_margin(8.0)
            .show(ui, |ui| {
                egui::ScrollArea::horizontal()
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        egui::Grid::new("summary-result-table")
                            .striped(true)
                            .min_col_width(88.0)
                            .show(ui, |ui| {
                                ui.strong("구분");
                                numeric_header(ui, &format!("전 [{}]", factor.unit), 92.0);
                                numeric_header(ui, &format!("후 [{}]", factor.unit), 92.0);
                                numeric_header(ui, &format!("감축 [{}]", factor.unit), 104.0);
                                numeric_header(ui, "감축률", 76.0);
                                numeric_header(ui, "표준편차", 88.0);
                                numeric_header(ui, "공사비", 112.0);
                                ui.end_row();

                                if let Some(result) = &self.user_result {
                                    let before = metric_display_value(
                                        self.selected_metric,
                                        result.baseline.mean,
                                        self.area_m2,
                                    );
                                    let before_std = metric_display_value(
                                        self.selected_metric,
                                        result.baseline.std,
                                        self.area_m2,
                                    );
                                    ui.strong("리모델링 이전");
                                    numeric_cell(ui, format_number(before), 92.0);
                                    numeric_cell(ui, "-", 92.0);
                                    numeric_cell(ui, "-", 104.0);
                                    numeric_cell(ui, "-", 76.0);
                                    numeric_cell(ui, format_number(before_std), 88.0);
                                    numeric_cell(ui, "-", 112.0);
                                    ui.end_row();

                                    for item in &result.options {
                                        summary_option_row(
                                            ui,
                                            item,
                                            before,
                                            self.selected_metric,
                                            self.area_m2,
                                        );
                                    }
                                } else {
                                    ui.label("결과 없음");
                                    numeric_cell(ui, "-", 92.0);
                                    numeric_cell(ui, "-", 92.0);
                                    numeric_cell(ui, "-", 104.0);
                                    numeric_cell(ui, "-", 76.0);
                                    numeric_cell(ui, "-", 88.0);
                                    numeric_cell(ui, "-", 112.0);
                                    ui.end_row();
                                }
                            });
                    });
            });
    }
}

#[derive(Debug, Clone)]
struct BarItem {
    label: String,
    value: f64,
    cost: Option<u64>,
}

#[derive(Debug, Clone)]
struct EffectBarItem {
    label: String,
    reduction: f64,
    rate: f64,
}

#[derive(Debug, Clone, Copy)]
struct AxisRange {
    min: f64,
    max: f64,
}

#[derive(Debug, Clone)]
struct DisplayScale {
    unit: String,
    divisor: f64,
}

const GRID_COLOR: Color32 = Color32::from_rgb(229, 235, 235);
const AXIS_COLOR: Color32 = Color32::from_rgb(146, 162, 164);
const TEXT_COLOR: Color32 = Color32::from_rgb(52, 65, 69);
const MUTED_TEXT: Color32 = Color32::from_rgb(96, 119, 122);
const ACCENT: Color32 = Color32::from_rgb(35, 128, 122);
const ACCENT_2: Color32 = Color32::from_rgb(217, 100, 73);

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
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(4.0, 22.0), Sense::hover());
        ui.painter().rect_filled(rect, 2.0, ACCENT);
        ui.label(RichText::new(title).size(20.0).strong().color(TEXT_COLOR));
    });
}

fn subsection_label(ui: &mut egui::Ui, title: &str) {
    ui.label(
        RichText::new(title)
            .size(15.0)
            .strong()
            .color(Color32::from_rgb(82, 97, 100)),
    );
    ui.add_space(4.0);
}

fn tab_button(ui: &mut egui::Ui, selected: &mut DashboardTab, value: DashboardTab, label: &str) {
    let is_selected = *selected == value;
    let text = if is_selected {
        RichText::new(label).strong().color(Color32::WHITE)
    } else {
        RichText::new(label).color(TEXT_COLOR)
    };
    let button = egui::Button::new(text)
        .min_size(Vec2::new(150.0, 34.0))
        .fill(if is_selected {
            ACCENT
        } else {
            Color32::from_rgb(232, 238, 238)
        })
        .stroke(Stroke::new(
            1.0,
            if is_selected {
                ACCENT
            } else {
                Color32::from_rgb(196, 211, 212)
            },
        ));
    if ui.add(button).clicked() {
        *selected = value;
    }
}

fn numeric_cell(ui: &mut egui::Ui, text: impl Into<String>, width: f32) {
    let text = text.into();
    ui.allocate_ui_with_layout(
        Vec2::new(width, 18.0),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.label(text);
        },
    );
}

fn numeric_header(ui: &mut egui::Ui, text: impl Into<String>, width: f32) {
    let text = text.into();
    ui.allocate_ui_with_layout(
        Vec2::new(width, 18.0),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.strong(text);
        },
    );
}

fn density_chart(
    ui: &mut egui::Ui,
    id: &str,
    series: &[(String, EnergyStats)],
    metric: EnergyMetric,
    area_m2: f64,
) {
    ui.push_id(id, |ui| {
        let legend_rows = ((series.len().max(1) + 2) / 3) as f32;
        let desired = Vec2::new(
            chart_available_width(ui),
            240.0 + (legend_rows - 1.0) * 18.0,
        );
        let (rect, _) = ui.allocate_exact_size(desired, Sense::hover());
        let painter = ui.painter_at(rect);
        chart_background(&painter, rect);

        if series.is_empty() {
            return;
        }

        let mut raw_max_x = f64::NEG_INFINITY;
        for (_, stats) in series {
            let mean = metric_display_value(metric, stats.mean, area_m2);
            let std = metric_display_value(metric, stats.std, area_m2)
                .abs()
                .max(0.001);
            raw_max_x = raw_max_x.max(mean + std * 3.0);
        }

        if !raw_max_x.is_finite() {
            return;
        }

        let scale = metric_chart_scale(metric, raw_max_x);
        let max_x = raw_max_x / scale.divisor;
        let mut max_y = 0.0_f64;
        for (_, stats) in series {
            let mean = metric_display_value(metric, stats.mean, area_m2) / scale.divisor;
            let std = (metric_display_value(metric, stats.std, area_m2) / scale.divisor)
                .abs()
                .max(0.001);
            max_y = max_y.max(normal_pdf(mean, mean, std));
        }

        let x_axis = axis_from_zero(max_x);
        let y_axis = axis_from_zero(max_y);
        let legend_height = 18.0 * legend_rows + 18.0;
        let plot = plot_rect(rect, 64.0, 22.0, legend_height + 18.0, 52.0);
        draw_chart_grid(
            &painter,
            plot,
            x_axis,
            y_axis,
            &format!("배출/사용량 [{}]", scale.unit),
            "밀도",
        );
        let colors = [
            ACCENT,
            ACCENT_2,
            Color32::from_rgb(72, 132, 83),
            Color32::from_rgb(145, 91, 166),
            Color32::from_rgb(191, 143, 49),
        ];

        for (index, (label, stats)) in series.iter().enumerate() {
            let mean = metric_display_value(metric, stats.mean, area_m2) / scale.divisor;
            let std = (metric_display_value(metric, stats.std, area_m2) / scale.divisor)
                .abs()
                .max(0.001);
            let color = colors[index % colors.len()];
            let mut points = Vec::with_capacity(96);
            for step in 0..96 {
                let t = step as f64 / 95.0;
                let x = x_axis.min + (x_axis.max - x_axis.min) * t;
                let y = normal_pdf(x, mean, std);
                points.push(Pos2::new(map_x(plot, x_axis, x), map_y(plot, y_axis, y)));
            }
            painter.add(Shape::line(points, Stroke::new(2.0, color)));

            let legend_y = rect.top() + 16.0 + (index / 3) as f32 * 18.0;
            let legend_x = rect.left() + 18.0 + (index % 3) as f32 * 180.0;
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
                TEXT_COLOR,
            );
        }
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
        let desired = Vec2::new(chart_available_width(ui), height);
        let (rect, _) = ui.allocate_exact_size(desired, Sense::hover());
        let painter = ui.painter_at(rect);
        chart_background(&painter, rect);

        if items.is_empty() {
            return;
        }

        let max_abs = items
            .iter()
            .map(|item| item.value.abs())
            .fold(0.0_f64, f64::max);
        let scale = chart_value_scale(unit, max_abs);
        let plot = plot_rect(rect, 132.0, 172.0, 28.0, 46.0);
        let min_value = items
            .iter()
            .map(|item| item.value / scale.divisor)
            .fold(0.0_f64, f64::min);
        let max_value = items
            .iter()
            .map(|item| item.value / scale.divisor)
            .fold(0.0_f64, f64::max);
        let x_axis = signed_axis(min_value, max_value);
        for tick in ticks(x_axis, 5) {
            let x = map_x(plot, x_axis, tick);
            painter.line_segment(
                [Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())],
                Stroke::new(1.0, GRID_COLOR),
            );
            painter.text(
                Pos2::new(x, plot.bottom() + 16.0),
                Align2::CENTER_CENTER,
                format_tick(tick),
                egui::FontId::proportional(11.0),
                MUTED_TEXT,
            );
        }
        painter.line_segment(
            [
                Pos2::new(plot.left(), plot.bottom()),
                Pos2::new(plot.right(), plot.bottom()),
            ],
            Stroke::new(1.2, AXIS_COLOR),
        );
        let zero_x = map_x(plot, x_axis, 0.0);
        painter.line_segment(
            [
                Pos2::new(zero_x, plot.top()),
                Pos2::new(zero_x, plot.bottom()),
            ],
            Stroke::new(1.2, AXIS_COLOR),
        );
        painter.text(
            Pos2::new(plot.center().x, plot.bottom() + 34.0),
            Align2::CENTER_CENTER,
            format!("감축량 [{}]", scale.unit),
            egui::FontId::proportional(12.0),
            MUTED_TEXT,
        );

        let row_h = (plot.height() / items.len() as f32).max(20.0);
        for (index, item) in items.iter().enumerate() {
            let y = plot.top() + row_h * (index as f32 + 0.5);
            let display_value = item.value / scale.divisor;
            let value_x = map_x(plot, x_axis, display_value);
            let left = zero_x.min(value_x);
            let right = zero_x.max(value_x);
            let color = if item.value >= 0.0 {
                ACCENT
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
                TEXT_COLOR,
            );
            let value_label = format_number(display_value);
            let label_text = match item.cost {
                Some(cost) => format!("{value_label} / {}", format_cost(cost)),
                None => value_label,
            };
            let estimated_label_width = label_text.chars().count() as f32 * 7.0;
            let (label_x, label_align, label_color) = if item.value >= 0.0 {
                let x = right + 8.0;
                if display_value >= x_axis.max * 0.65 {
                    (plot.right() - 8.0, Align2::RIGHT_CENTER, Color32::WHITE)
                } else if x + estimated_label_width > rect.right() - 10.0 {
                    (rect.right() - 10.0, Align2::RIGHT_CENTER, TEXT_COLOR)
                } else {
                    (x, Align2::LEFT_CENTER, TEXT_COLOR)
                }
            } else {
                let x = left - 8.0;
                if x - estimated_label_width < rect.left() + 10.0 {
                    (rect.left() + 10.0, Align2::LEFT_CENTER, TEXT_COLOR)
                } else {
                    (x, Align2::RIGHT_CENTER, TEXT_COLOR)
                }
            };
            painter.text(
                Pos2::new(label_x, y),
                label_align,
                label_text,
                egui::FontId::proportional(12.0),
                label_color,
            );
        }
    });
}

fn single_effect_bar_chart(
    ui: &mut egui::Ui,
    id: &str,
    items: &[EffectBarItem],
    unit: &str,
    baseline: f64,
    min_height: f32,
) {
    ui.push_id(id, |ui| {
        let height = min_height.max(items.len() as f32 * 28.0 + 74.0);
        let desired = Vec2::new(chart_available_width(ui), height);
        let (rect, _) = ui.allocate_exact_size(desired, Sense::hover());
        let painter = ui.painter_at(rect);
        chart_background(&painter, rect);

        if items.is_empty() {
            return;
        }

        let max_reduction = items
            .iter()
            .map(|item| item.reduction)
            .fold(0.0_f64, f64::max);
        let scale = chart_value_scale(unit, max_reduction);
        let x_axis = axis_from_zero(max_reduction / scale.divisor);
        let plot = plot_rect(rect, 144.0, 152.0, 46.0, 54.0);

        for tick in ticks(x_axis, 5) {
            let x = map_x(plot, x_axis, tick);
            painter.line_segment(
                [Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())],
                Stroke::new(1.0, GRID_COLOR),
            );
            painter.text(
                Pos2::new(x, plot.top() - 18.0),
                Align2::CENTER_CENTER,
                format_tick(tick),
                egui::FontId::proportional(11.0),
                MUTED_TEXT,
            );
            let rate_tick = reduction_rate(baseline, tick * scale.divisor) * 100.0;
            painter.text(
                Pos2::new(x, plot.bottom() + 18.0),
                Align2::CENTER_CENTER,
                format!("{rate_tick:.0}"),
                egui::FontId::proportional(11.0),
                MUTED_TEXT,
            );
        }

        painter.line_segment(
            [
                Pos2::new(plot.left(), plot.bottom()),
                Pos2::new(plot.right(), plot.bottom()),
            ],
            Stroke::new(1.2, AXIS_COLOR),
        );
        painter.line_segment(
            [
                Pos2::new(plot.left(), plot.top()),
                Pos2::new(plot.left(), plot.bottom()),
            ],
            Stroke::new(1.2, AXIS_COLOR),
        );
        painter.text(
            Pos2::new(plot.center().x, rect.top() + 18.0),
            Align2::CENTER_CENTER,
            format!("감축량 [{}]", scale.unit),
            egui::FontId::proportional(12.0),
            MUTED_TEXT,
        );
        painter.text(
            Pos2::new(plot.center().x, rect.bottom() - 16.0),
            Align2::CENTER_CENTER,
            "감축률 [%]",
            egui::FontId::proportional(12.0),
            MUTED_TEXT,
        );

        let row_h = (plot.height() / items.len() as f32).max(22.0);
        for (index, item) in items.iter().enumerate() {
            let y = plot.top() + row_h * (index as f32 + 0.5);
            let display_reduction = item.reduction / scale.divisor;
            let value_x = map_x(plot, x_axis, display_reduction);
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(plot.left(), y - row_h * 0.28),
                    Pos2::new(value_x.max(plot.left() + 2.0), y + row_h * 0.28),
                ),
                3.0,
                ACCENT,
            );
            painter.text(
                Pos2::new(rect.left() + 14.0, y),
                Align2::LEFT_CENTER,
                short_label(&item.label, 17),
                egui::FontId::proportional(12.0),
                TEXT_COLOR,
            );
            let label_text = format!(
                "{} / {}",
                format_number(display_reduction),
                format_percent(item.rate)
            );
            let estimated_label_width = label_text.chars().count() as f32 * 7.0;
            let label_x = value_x + 8.0;
            let (label_x, label_align, label_color) = if display_reduction >= x_axis.max * 0.65 {
                (plot.right() - 8.0, Align2::RIGHT_CENTER, Color32::WHITE)
            } else if label_x + estimated_label_width > rect.right() - 10.0 {
                (rect.right() - 10.0, Align2::RIGHT_CENTER, TEXT_COLOR)
            } else {
                (label_x, Align2::LEFT_CENTER, TEXT_COLOR)
            };
            painter.text(
                Pos2::new(label_x, y),
                label_align,
                label_text,
                egui::FontId::proportional(12.0),
                label_color,
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
        let desired = Vec2::new(chart_available_width(ui), 260.0);
        let (rect, _) = ui.allocate_exact_size(desired, Sense::hover());
        let painter = ui.painter_at(rect);
        chart_background(&painter, rect);
        let plot = plot_rect(rect, 68.0, 32.0, 36.0, 54.0);

        if result.options.is_empty() {
            return;
        }

        let max_cost_manwon = result
            .options
            .iter()
            .map(|item| item.cost as f64 / 10_000.0)
            .fold(0.0, f64::max)
            .max(0.0);
        let max_reduction = result
            .options
            .iter()
            .map(|item| metric_display_value(metric, item.reduction.mean, area_m2))
            .fold(0.0, f64::max)
            .max(0.0);
        let cost_scale = cost_chart_scale(max_cost_manwon);
        let reduction_scale = metric_chart_scale(metric, max_reduction);
        let x_axis = axis_from_zero(max_cost_manwon / cost_scale.divisor);
        let y_axis = axis_from_zero(max_reduction / reduction_scale.divisor);
        draw_chart_grid(
            &painter,
            plot,
            x_axis,
            y_axis,
            &format!("공사비 [{}]", cost_scale.unit),
            &format!("감축량 [{}]", reduction_scale.unit),
        );

        for item in &result.options {
            let x_value = (item.cost as f64 / 10_000.0) / cost_scale.divisor;
            let y_value = metric_display_value(metric, item.reduction.mean, area_m2)
                / reduction_scale.divisor;
            let point = Pos2::new(map_x(plot, x_axis, x_value), map_y(plot, y_axis, y_value));
            painter.circle_filled(point, 4.5, ACCENT);
            let label_pos = Pos2::new(
                (point.x + 6.0).min(plot.right() - 6.0),
                (point.y - 6.0).max(plot.top() + 6.0),
            );
            painter.text(
                label_pos,
                Align2::LEFT_CENTER,
                short_label(&item.label, 12),
                egui::FontId::proportional(11.0),
                TEXT_COLOR,
            );
        }
    });
}

fn empty_chart(ui: &mut egui::Ui, id: &str, message: &str) {
    ui.push_id(id, |ui| {
        let desired = Vec2::new(chart_available_width(ui), 180.0);
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
    let factor = metric.factor();
    egui::ScrollArea::horizontal()
        .auto_shrink([false, true])
        .show(ui, |ui| {
            egui::Grid::new(id)
                .striped(true)
                .min_col_width(86.0)
                .show(ui, |ui| {
                    ui.strong("구분");
                    numeric_header(ui, &format!("전 [{}]", factor.unit), 92.0);
                    numeric_header(ui, &format!("후 [{}]", factor.unit), 92.0);
                    numeric_header(ui, &format!("감축 [{}]", factor.unit), 104.0);
                    numeric_header(ui, "감축률", 76.0);
                    numeric_header(ui, "표준편차", 88.0);
                    numeric_header(ui, "공사비", 112.0);
                    ui.end_row();

                    let before = metric_display_value(metric, result.baseline.mean, area_m2);
                    for item in &result.options {
                        option_row(ui, item, before, metric, area_m2, true);
                    }
                });
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
    let rate = reduction_rate(before, reduction);

    ui.push_id(item.id, |ui| {
        ui.label(short_label(&item.label, 24));
    });
    if full {
        numeric_cell(ui, format_number(before), 92.0);
        numeric_cell(ui, format_number(after), 92.0);
        numeric_cell(ui, format_number(reduction), 104.0);
        numeric_cell(ui, format_percent(rate), 76.0);
        numeric_cell(ui, format_number(std), 88.0);
        numeric_cell(ui, format_cost(item.cost), 112.0);
    } else {
        numeric_cell(ui, format_number(reduction), 104.0);
        numeric_cell(ui, format_percent(rate), 76.0);
        numeric_cell(ui, format_cost(item.cost), 112.0);
    }
    ui.end_row();
}

fn summary_option_row(
    ui: &mut egui::Ui,
    item: &OptionEstimate,
    before: f64,
    metric: EnergyMetric,
    area_m2: f64,
) {
    let after = metric_display_value(metric, item.after.mean, area_m2);
    let reduction = metric_display_value(metric, item.reduction.mean, area_m2);
    let std = metric_display_value(metric, item.reduction.std, area_m2).abs();
    let rate = reduction_rate(before, reduction);

    ui.push_id(item.id, |ui| {
        ui.label(short_label(&item.label, 22));
    });
    numeric_cell(ui, format_number(before), 92.0);
    numeric_cell(ui, format_number(after), 92.0);
    numeric_cell(ui, format_number(reduction), 104.0);
    numeric_cell(ui, format_percent(rate), 76.0);
    numeric_cell(ui, format_number(std), 88.0);
    numeric_cell(ui, format_cost(item.cost), 112.0);
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

fn chart_available_width(ui: &egui::Ui) -> f32 {
    let available = ui.available_rect_before_wrap();
    let clip = ui.clip_rect();
    let left = available.left().max(clip.left());
    let right = available.right().min(clip.right());
    let visible = (right - left).max(0.0);
    if visible >= 320.0 {
        visible
    } else {
        ui.available_width().max(320.0)
    }
}

fn plot_rect(rect: Rect, left: f32, right: f32, top: f32, bottom: f32) -> Rect {
    Rect::from_min_max(
        Pos2::new(rect.left() + left, rect.top() + top),
        Pos2::new(rect.right() - right, rect.bottom() - bottom),
    )
}

fn axis_from_zero(max_value: f64) -> AxisRange {
    AxisRange {
        min: 0.0,
        max: nice_upper(max_value.max(0.0)),
    }
}

fn signed_axis(min_value: f64, max_value: f64) -> AxisRange {
    let min = min_value.min(0.0);
    let max = max_value.max(0.0);
    if min.abs() < f64::EPSILON {
        return axis_from_zero(max);
    }

    let span = nice_upper((max - min).abs());
    AxisRange {
        min,
        max: min + span,
    }
}

fn nice_upper(value: f64) -> f64 {
    if !value.is_finite() || value <= 0.0 {
        return 1.0;
    }
    let exponent = value.log10().floor();
    let base = 10_f64.powf(exponent);
    let normalized = value / base;
    let nice = if normalized <= 1.0 {
        1.0
    } else if normalized <= 2.0 {
        2.0
    } else if normalized <= 5.0 {
        5.0
    } else {
        10.0
    };
    nice * base
}

fn ticks(axis: AxisRange, count: usize) -> Vec<f64> {
    if count <= 1 {
        return vec![axis.min];
    }
    let step = (axis.max - axis.min) / (count - 1) as f64;
    (0..count)
        .map(|index| axis.min + step * index as f64)
        .collect()
}

fn map_x(plot: Rect, axis: AxisRange, value: f64) -> f32 {
    let span = (axis.max - axis.min).max(f64::EPSILON);
    lerp(
        plot.left(),
        plot.right(),
        ((value - axis.min) / span) as f32,
    )
}

fn map_y(plot: Rect, axis: AxisRange, value: f64) -> f32 {
    let span = (axis.max - axis.min).max(f64::EPSILON);
    lerp(
        plot.bottom(),
        plot.top(),
        ((value - axis.min) / span) as f32,
    )
}

fn draw_chart_grid(
    painter: &egui::Painter,
    plot: Rect,
    x_axis: AxisRange,
    y_axis: AxisRange,
    x_unit: &str,
    y_unit: &str,
) {
    for tick in ticks(x_axis, 5) {
        let x = map_x(plot, x_axis, tick);
        painter.line_segment(
            [Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())],
            Stroke::new(1.0, GRID_COLOR),
        );
        painter.text(
            Pos2::new(x, plot.bottom() + 16.0),
            Align2::CENTER_CENTER,
            format_tick(tick),
            egui::FontId::proportional(11.0),
            MUTED_TEXT,
        );
    }

    for tick in ticks(y_axis, 5) {
        let y = map_y(plot, y_axis, tick);
        painter.line_segment(
            [Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)],
            Stroke::new(1.0, GRID_COLOR),
        );
        painter.text(
            Pos2::new(plot.left() - 8.0, y),
            Align2::RIGHT_CENTER,
            format_tick(tick),
            egui::FontId::proportional(11.0),
            MUTED_TEXT,
        );
    }

    painter.line_segment(
        [
            Pos2::new(plot.left(), plot.bottom()),
            Pos2::new(plot.right(), plot.bottom()),
        ],
        Stroke::new(1.2, AXIS_COLOR),
    );
    painter.line_segment(
        [
            Pos2::new(plot.left(), plot.top()),
            Pos2::new(plot.left(), plot.bottom()),
        ],
        Stroke::new(1.2, AXIS_COLOR),
    );

    painter.text(
        Pos2::new(plot.center().x, plot.bottom() + 34.0),
        Align2::CENTER_CENTER,
        x_unit,
        egui::FontId::proportional(12.0),
        MUTED_TEXT,
    );
    painter.text(
        Pos2::new(plot.left(), plot.top() - 14.0),
        Align2::LEFT_CENTER,
        y_unit,
        egui::FontId::proportional(12.0),
        MUTED_TEXT,
    );
}

fn metric_chart_scale(metric: EnergyMetric, max_abs: f64) -> DisplayScale {
    chart_value_scale(metric.factor().unit, max_abs)
}

fn chart_value_scale(unit: &str, max_abs: f64) -> DisplayScale {
    let max_abs = max_abs.abs();
    match unit {
        "MWh" if max_abs < 1.0 => DisplayScale {
            unit: "kWh".to_string(),
            divisor: 0.001,
        },
        "MWh" if max_abs >= 1000.0 => DisplayScale {
            unit: "GWh".to_string(),
            divisor: 1000.0,
        },
        "tCO2eq" if max_abs < 1.0 => DisplayScale {
            unit: "kgCO2eq".to_string(),
            divisor: 0.001,
        },
        "tCO2eq" if max_abs >= 1000.0 => DisplayScale {
            unit: "ktCO2eq".to_string(),
            divisor: 1000.0,
        },
        _ => DisplayScale {
            unit: unit.to_string(),
            divisor: 1.0,
        },
    }
}

fn cost_chart_scale(max_manwon: f64) -> DisplayScale {
    if max_manwon >= 10_000.0 {
        DisplayScale {
            unit: "억원".to_string(),
            divisor: 10_000.0,
        }
    } else {
        DisplayScale {
            unit: "만원".to_string(),
            divisor: 1.0,
        }
    }
}

fn normal_pdf(x: f64, mean: f64, std: f64) -> f64 {
    let variance = std * std;
    (-((x - mean).powi(2)) / (2.0 * variance)).exp() / (std * (2.0 * std::f64::consts::PI).sqrt())
}

fn lerp(start: f32, end: f32, t: f32) -> f32 {
    start + (end - start) * t.clamp(0.0, 1.0)
}

fn format_number(value: f64) -> String {
    let abs = value.abs();
    if abs >= 1000.0 {
        format!("{value:.0}")
    } else if abs >= 100.0 {
        format!("{value:.1}")
    } else if abs >= 10.0 {
        format!("{value:.1}")
    } else if abs >= 1.0 {
        format!("{value:.2}")
    } else if abs > 0.0 {
        format!("{value:.3}")
    } else {
        "0".to_string()
    }
}

fn format_tick(value: f64) -> String {
    let abs = value.abs();
    if abs >= 1000.0 {
        format!("{value:.0}")
    } else if abs >= 100.0 {
        format!("{value:.0}")
    } else if abs >= 10.0 {
        format!("{value:.0}")
    } else if abs >= 1.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.2}")
    }
}

fn format_percent(rate: f64) -> String {
    format!("{:.1}%", rate * 100.0)
}

fn reduction_rate(before: f64, reduction: f64) -> f64 {
    if before.abs() > f64::EPSILON {
        reduction / before
    } else {
        0.0
    }
}

fn format_cost(cost: u64) -> String {
    format!(
        "{} 만원",
        format_number_with_commas(cost as f64 / 10_000.0, 0)
    )
}

fn format_number_with_commas(value: f64, decimals: usize) -> String {
    let formatted = format!("{value:.decimals$}");
    let (integer, fractional) = formatted
        .split_once('.')
        .map(|(integer, fractional)| (integer, Some(fractional)))
        .unwrap_or((&formatted, None));
    let negative = integer.starts_with('-');
    let digits = if negative { &integer[1..] } else { integer };
    let mut output = String::new();
    for (index, ch) in digits.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            output.push(',');
        }
        output.push(ch);
    }
    let integer = output.chars().rev().collect::<String>();
    let sign = if negative { "-" } else { "" };
    match fractional {
        Some(fractional) => format!("{sign}{integer}.{fractional}"),
        None => format!("{sign}{integer}"),
    }
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

fn option_level_row(
    ui: &mut egui::Ui,
    label: &str,
    row_id: &str,
    options: &mut [RetrofitOption],
    selector: for<'a> fn(&'a mut RetrofitSpec) -> &'a mut u8,
    levels: &[(u8, &str)],
) {
    ui.strong(label);
    for option in options {
        let selected = selector(&mut option.spec);
        let selected_text = levels
            .iter()
            .find(|(value, _)| value == selected)
            .map(|(_, label)| *label)
            .unwrap_or("기존");
        egui::ComboBox::from_id_salt((row_id, option.id))
            .selected_text(selected_text)
            .width(84.0)
            .show_ui(ui, |ui| {
                for (value, item_label) in levels {
                    ui.selectable_value(selected, *value, *item_label);
                }
            });
    }
    ui.end_row();
}

fn option_measure_row(
    ui: &mut egui::Ui,
    measure: BinaryRetrofitMeasure,
    options: &mut [RetrofitOption],
) {
    ui.strong(measure.label());
    for option in options {
        let mut enabled = option.spec.is_enabled(measure);
        ui.centered_and_justified(|ui| {
            if ui.checkbox(&mut enabled, "").changed() {
                option.spec.set_enabled(measure, enabled);
            }
        });
    }
    ui.end_row();
}

fn spec_wall(spec: &mut RetrofitSpec) -> &mut u8 {
    &mut spec.wall
}

fn spec_roof(spec: &mut RetrofitSpec) -> &mut u8 {
    &mut spec.roof
}

fn spec_floor(spec: &mut RetrofitSpec) -> &mut u8 {
    &mut spec.floor
}

fn spec_window(spec: &mut RetrofitSpec) -> &mut u8 {
    &mut spec.window
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
