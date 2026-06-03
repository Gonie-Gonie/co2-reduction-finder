use std::{
    collections::BTreeSet,
    sync::mpsc::{self, Receiver},
    time::Duration,
};

use eframe::egui::{self, Color32, RichText, Stroke};

use crate::domain::{
    estimate_reduction, run_preview_pareto_job, EmbeddedModelStore, EstimateRequest, EstimateResult,
    ParetoProgressEvent, ReferenceData, RetrofitMeasure, RetrofitOption, BUILDING_TYPES, CLIMATES,
    ERAS,
};

pub struct Co2App {
    selected_building: usize,
    selected_climate: usize,
    selected_era: usize,
    area_m2: f64,
    options: Vec<RetrofitOption>,
    next_option_id: u64,
    result: Option<EstimateResult>,
    error: Option<String>,
    model_store: Option<EmbeddedModelStore>,
    model_error: Option<String>,
    reference_data: Option<ReferenceData>,
    reference_error: Option<String>,
    model_blend_check: Option<String>,
    progress: f32,
    progress_message: String,
    progress_rx: Option<Receiver<ParetoProgressEvent>>,
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
                let result = data
                    .model1_weight_for_base("Office")
                    .and_then(|weight| {
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

        Self {
            selected_building: 3,
            selected_climate: 0,
            selected_era: 2,
            area_m2: 1000.0,
            options: vec![
                RetrofitOption {
                    id: 1,
                    label: "Option 1".to_string(),
                    measures: BTreeSet::from([
                        RetrofitMeasure::Wall,
                        RetrofitMeasure::Window,
                        RetrofitMeasure::Lights,
                    ]),
                },
                RetrofitOption {
                    id: 2,
                    label: "Option 2".to_string(),
                    measures: BTreeSet::from([
                        RetrofitMeasure::Roof,
                        RetrofitMeasure::Cooling,
                        RetrofitMeasure::Pv,
                    ]),
                },
            ],
            next_option_id: 3,
            result: None,
            error: None,
            model_store,
            model_error,
            reference_data,
            reference_error,
            model_blend_check,
            progress: 0.0,
            progress_message: "대기".to_string(),
            progress_rx: None,
            is_running: false,
        }
    }

    fn request(&self) -> EstimateRequest {
        EstimateRequest {
            building_type: BUILDING_TYPES[self.selected_building].code.to_string(),
            climate: CLIMATES[self.selected_climate].code.to_string(),
            era: ERAS[self.selected_era].code.to_string(),
            area_m2: self.area_m2,
            options: self.options.clone(),
        }
    }

    fn calculate(&mut self) {
        let request = self.request();
        self.error = None;
        self.progress = 0.0;
        self.progress_message = "계산 준비".to_string();

        match estimate_reduction(&request) {
            Ok(result) => {
                self.result = Some(result);
                let (sender, receiver) = mpsc::channel();
                run_preview_pareto_job(sender);
                self.progress_rx = Some(receiver);
                self.is_running = true;
            }
            Err(error) => {
                self.error = Some(error);
                self.is_running = false;
            }
        }
    }

    fn poll_progress(&mut self, ctx: &egui::Context) {
        if let Some(receiver) = &self.progress_rx {
            while let Ok(event) = receiver.try_recv() {
                self.progress = event.progress;
                self.progress_message = event.message;
                if self.progress >= 1.0 {
                    self.is_running = false;
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
            label: format!("Option {}", self.options.len() + 1),
            measures: BTreeSet::new(),
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

        egui::Panel::top("topbar").show_inside(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("CO2 Reduction Finder").size(24.0).strong());
                    ui.label(RichText::new("에너지 감축계수 조회 Dashboard").color(Color32::from_rgb(96, 119, 122)));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if self.is_running { "계산 중" } else { "계산" };
                    if ui
                        .add_enabled(!self.is_running, egui::Button::new(label).min_size([92.0, 38.0].into()))
                        .clicked()
                    {
                        self.calculate();
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
    }
}

impl Co2App {
    fn controls_ui(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.heading("기준 조건");
        ui.add_space(8.0);
        self.model_status_ui(ui);
        ui.separator();

        labeled_combo(ui, "본과제용도분류", "building-type", &mut self.selected_building, BUILDING_TYPES.iter().map(|item| item.label));
        let selected_type = BUILDING_TYPES[self.selected_building];
        ui.label(if selected_type.residential { "주거" } else { "비주거" });
        ui.add_space(8.0);
        labeled_combo(ui, "기후권", "climate-zone", &mut self.selected_climate, CLIMATES.iter().map(|item| item.label));
        labeled_combo(ui, "준공연도", "era-band", &mut self.selected_era, ERAS.iter().map(|item| item.label));

        ui.add_space(8.0);
        ui.label(RichText::new("연면적 m2").strong().color(Color32::from_rgb(82, 97, 100)));
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
                    for chunk in RetrofitMeasure::ALL.chunks(3) {
                        ui.horizontal(|ui| {
                            for measure in chunk {
                                let mut enabled = option.measures.contains(measure);
                                if ui.checkbox(&mut enabled, measure.label()).changed() {
                                    if enabled {
                                        option.measures.insert(*measure);
                                    } else {
                                        option.measures.remove(measure);
                                    }
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
        ui.add_space(8.0);
        self.kpi_ui(ui);
        ui.add_space(16.0);

        ui.horizontal(|ui| {
            ui.heading("비교 결과");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(&self.progress_message);
            });
        });
        ui.add_space(8.0);

        if let Some(error) = &self.error {
            ui.colored_label(Color32::from_rgb(160, 54, 45), error);
        }

        egui::Frame::default()
            .stroke(Stroke::new(1.0, Color32::from_rgb(216, 226, 226)))
            .corner_radius(6.0)
            .inner_margin(8.0)
            .show(ui, |ui| {
                egui::Grid::new("result-table")
                    .striped(true)
                    .min_col_width(120.0)
                    .show(ui, |ui| {
                        ui.strong("Option");
                        ui.strong("전기 after");
                        ui.strong("가스 after");
                        ui.strong("CO2 감축");
                        ui.strong("공사비");
                        ui.end_row();

                        if let Some(result) = &self.result {
                            for item in &result.options {
                                ui.push_id(item.id, |ui| {
                                    ui.label(&item.label);
                                });
                                ui.label(format!("{:.1}", item.elec_after));
                                ui.label(format!("{:.1}", item.gas_after));
                                ui.label(format!("{:.1}", item.co2_reduction));
                                ui.label(format!("{}", item.cost));
                                ui.end_row();
                            }
                        } else {
                            for option in &self.options {
                                ui.label(&option.label);
                                ui.label("-");
                                ui.label("-");
                                ui.label("-");
                                ui.label("-");
                                ui.end_row();
                            }
                        }
                    });
            });

        ui.add_space(16.0);
        ui.add(egui::ProgressBar::new(self.progress).show_percentage());
    }

    fn model_status_ui(&self, ui: &mut egui::Ui) {
        if let Some(store) = &self.model_store {
            ui.label(RichText::new("모델 asset").strong().color(Color32::from_rgb(82, 97, 100)));
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
                        .and_then(|data| Some((data.get(&model_1)?.weight, data.get(&model_2)?.weight)))
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
                                if metadata.residential { "주거" } else { "비주거" },
                                if metadata.gas_heating { "gas heat" } else { "non-gas heat" },
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
            ui.colored_label(Color32::from_rgb(160, 54, 45), format!("model load failed: {error}"));
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
            ui.colored_label(Color32::from_rgb(160, 54, 45), format!("metadata load failed: {error}"));
            ui.add_space(8.0);
        }
    }

    fn kpi_ui(&self, ui: &mut egui::Ui) {
        let baseline = self.result.as_ref().map(|result| &result.baseline);
        let best = self
            .result
            .as_ref()
            .and_then(|result| result.options.iter().max_by(|a, b| a.co2_reduction.total_cmp(&b.co2_reduction)));

        ui.columns(4, |columns| {
            kpi_cell(&mut columns[0], "전기 before", baseline.map(|value| value.elec));
            kpi_cell(&mut columns[1], "가스 before", baseline.map(|value| value.gas));
            kpi_cell(&mut columns[2], "온실가스 before", baseline.map(|value| value.co2));
            kpi_cell(&mut columns[3], "최대 감축", best.map(|value| value.co2_reduction));
        });
    }
}

fn labeled_combo<'a>(
    ui: &mut egui::Ui,
    label: &str,
    id: &str,
    selected: &mut usize,
    labels: impl Iterator<Item = &'a str>,
) {
    let labels = labels.collect::<Vec<_>>();
    ui.label(RichText::new(label).strong().color(Color32::from_rgb(82, 97, 100)));
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

fn kpi_cell(ui: &mut egui::Ui, label: &str, value: Option<f64>) {
    egui::Frame::default()
        .stroke(Stroke::new(1.0, Color32::from_rgb(216, 226, 226)))
        .fill(Color32::WHITE)
        .corner_radius(6.0)
        .inner_margin(12.0)
        .show(ui, |ui| {
            ui.label(RichText::new(label).strong().color(Color32::from_rgb(82, 97, 100)));
            ui.label(RichText::new(value.map(|v| format!("{v:.1}")).unwrap_or_else(|| "-".to_string())).size(26.0));
        });
}

fn apply_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::light();
    visuals.panel_fill = Color32::from_rgb(245, 247, 248);
    visuals.window_fill = Color32::WHITE;
    visuals.widgets.active.bg_fill = Color32::from_rgb(40, 122, 112);
    visuals.selection.bg_fill = Color32::from_rgb(40, 122, 112);
    ctx.set_visuals(visuals);
}
