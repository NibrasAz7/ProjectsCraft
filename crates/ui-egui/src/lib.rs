//! `projectcraft_ui_egui` (L6) — ProjectCraft Studio UI.
//!
//! One engine, five interactive lenses over the same [`pmcore::Project`] document:
//! 1. **Table / WBS** — hierarchical WBS grid, inline status/progress/duration edits, baseline variance
//! 2. **Gantt / Schedule** — 60 FPS interactive timeline canvas with critical path, dependencies & baselines
//! 3. **Board / Agile** — WIP-limited Kanban columns, one-click transitions, Sprint selector & burndown
//! 4. **Resources & Workload** — resource pool, rate tables, daily workload heatmap & one-click levelling
//! 5. **EVM & Risk** — ANSI/EIA-748 KPI cards, cumulative PV/EV/AC S-curve & Monte Carlo risk simulator
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod control;

use std::path::PathBuf;
use std::sync::mpsc::Receiver;

use control::ControlRequest;
use craft_settings::Settings;
use craft_ui::theme::WidgetPalette;
use pmcore::{DepKind, ItemKind, TaskStatus};
use pmsched::{compute_baseline_variance, compute_evm, compute_sprint_burndown, compute_workload};
use projectcraft_engine::Session;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};

/// Persistent `config.toml` preferences managed via `craft-settings`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectCraftPrefs {
    pub theme: String,
    pub default_lens: String,
    pub gantt_day_width: f32,
}

impl Default for ProjectCraftPrefs {
    fn default() -> Self {
        ProjectCraftPrefs { theme: "dark".to_string(), default_lens: "gantt".to_string(), gantt_day_width: 30.0 }
    }
}

impl Settings for ProjectCraftPrefs {
    const APP_ID: &'static str = "projectcraft";
    const SCHEMA_VERSION: u32 = 1;
}

/// Apply ProjectCraft's dark or light widget palette using `craft_ui::theme::WidgetPalette`.
pub fn apply_studio_theme(ctx: &egui::Context, mode: ThemeMode) {
    let mut visuals = match mode {
        ThemeMode::Dark => egui::Visuals::dark(),
        ThemeMode::Light => egui::Visuals::light(),
    };
    let (palette, fg) = match mode {
        ThemeMode::Dark => (
            WidgetPalette {
                noninteractive: egui::Color32::from_rgb(22, 27, 34),
                inactive: egui::Color32::from_rgb(33, 38, 45),
                hovered: egui::Color32::from_rgb(48, 54, 61),
                active: egui::Color32::from_rgb(47, 129, 247),
                open: egui::Color32::from_rgb(38, 44, 54),
            },
            egui::Stroke::new(1.0, egui::Color32::from_rgb(230, 237, 243)),
        ),
        ThemeMode::Light => (
            WidgetPalette {
                noninteractive: egui::Color32::from_rgb(246, 248, 250),
                inactive: egui::Color32::from_rgb(234, 238, 242),
                hovered: egui::Color32::from_rgb(208, 215, 222),
                active: egui::Color32::from_rgb(9, 105, 218),
                open: egui::Color32::from_rgb(220, 227, 233),
            },
            egui::Stroke::new(1.0, egui::Color32::from_rgb(31, 35, 40)),
        ),
    };
    palette.apply_to(&mut visuals.widgets, egui::CornerRadius::same(4), fg);
    ctx.set_visuals(visuals);
}

/// Active Studio lens over the project document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lens {
    Table,
    Gantt,
    Board,
    Resources,
    EvmRisk,
}

impl Lens {
    pub const ALL: [Lens; 5] = [Lens::Table, Lens::Gantt, Lens::Board, Lens::Resources, Lens::EvmRisk];

    pub fn label(self) -> &'static str {
        match self {
            Lens::Table => "Table & WBS",
            Lens::Gantt => "Gantt & CPM",
            Lens::Board => "Agile Board",
            Lens::Resources => "Resources & Levelling",
            Lens::EvmRisk => "EVM & Monte Carlo",
        }
    }
}

/// Studio visual theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Dark,
    Light,
}

/// Startup options for [`ProjectApp`].
#[derive(Default)]
pub struct ProjectAppOptions {
    pub control_rx: Option<Receiver<ControlRequest>>,
    pub startup_file: Option<PathBuf>,
}

/// Main ProjectCraft Studio application state.
pub struct ProjectApp {
    pub session: Session,
    pub active_lens: Lens,
    pub theme: ThemeMode,
    pub selected_task: Option<u32>,
    pub selected_sprint: Option<u32>,
    pub critical_only_filter: bool,
    pub gantt_day_width: f32,
    pub new_task_title: String,
    pub new_task_duration: f64,
    pub new_task_wbs: String,
    pub new_dep_pred: u32,
    pub new_dep_kind: DepKind,
    pub new_dep_lag: f64,
    pub mc_trials: usize,
    pub command_line: String,
    pub status_banner: String,
    pub level_log: Vec<String>,
    control_rx: Option<Receiver<ControlRequest>>,
    pub(crate) pending_shots: Vec<(ControlRequest, Option<String>)>,
    #[cfg(not(target_arch = "wasm32"))]
    settings_store: Option<craft_settings::SettingsStore<ProjectCraftPrefs>>,
}

impl ProjectApp {
    pub fn new(cc: &eframe::CreationContext<'_>, opts: ProjectAppOptions) -> Self {
        #[cfg_attr(target_arch = "wasm32", allow(unused_mut))]
        let mut app = Self::with_options(opts);
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (store, _report) = craft_settings::SettingsStore::<ProjectCraftPrefs>::open(craft_settings::Location::Auto);
            let prefs = store.get();
            app.theme = if prefs.theme.eq_ignore_ascii_case("light") { ThemeMode::Light } else { ThemeMode::Dark };
            app.gantt_day_width = prefs.gantt_day_width.clamp(16.0, 64.0);
            app.settings_store = Some(store);
        }
        apply_studio_theme(&cc.egui_ctx, app.theme);
        app
    }

    pub fn with_options(opts: ProjectAppOptions) -> Self {
        let mut app = ProjectApp {
            session: Session::new(),
            active_lens: Lens::Gantt,
            theme: ThemeMode::Dark,
            selected_task: Some(1),
            selected_sprint: Some(1),
            critical_only_filter: false,
            gantt_day_width: 30.0,
            new_task_title: String::new(),
            new_task_duration: 3.0,
            new_task_wbs: String::new(),
            new_dep_pred: 1,
            new_dep_kind: DepKind::FinishToStart,
            new_dep_lag: 0.0,
            mc_trials: 500,
            command_line: String::from("project.summary {}"),
            status_banner: String::from("Ready — Orion Edge Compute Launch (8 tasks, CPM scheduled)"),
            level_log: Vec::new(),
            control_rx: opts.control_rx,
            pending_shots: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            settings_store: None,
        };
        if let Some(path) = opts.startup_file {
            let p_str = path.to_string_lossy().to_string();
            let _ = app.exec_command("project.open", json!({"path": p_str}));
        }
        app
    }

    /// Execute an engine command, update the status bar, and return a JSON reply.
    pub fn exec_command(&mut self, id: &str, params: Json) -> Json {
        match self.session.execute(id, params) {
            Ok(result) => {
                self.status_banner = format!("OK `{id}`");
                json!({"ok": true, "command": id, "result": result})
            }
            Err(e) => {
                self.status_banner = format!("Error in `{id}`: {e}");
                json!({"ok": false, "command": id, "error": e.to_string()})
            }
        }
    }

    fn flush_screenshots(&mut self, ctx: &egui::Context) {
        if self.pending_shots.is_empty() {
            return;
        }
        let image = ctx.input(|i| {
            i.raw.events.iter().find_map(|ev| match ev {
                egui::Event::Screenshot { image, .. } => Some(Arc::clone(image)),
                _ => None,
            })
        });
        use std::sync::Arc;
        let Some(img) = image else { return };
        let png = encode_color_image_png(&img);
        for (req, path) in self.pending_shots.drain(..) {
            use base64::Engine;
            let mut out = json!({
                "ok": true,
                "width": img.width(),
                "height": img.height(),
                "bytes": png.len(),
                "pngBase64": base64::engine::general_purpose::STANDARD.encode(&png),
            });
            if let Some(p) = path {
                if let Err(e) = craft_atomic::atomic_write(std::path::Path::new(&p), &png) {
                    out = json!({"ok": false, "error": format!("cannot write {p}: {e}")});
                } else {
                    out["path"] = json!(p);
                }
            }
            req.reply(out);
        }
    }

    fn draw_top_ribbon(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.heading(egui::RichText::new("ProjectCraft").strong());
            ui.separator();

            for lens in Lens::ALL {
                let selected = self.active_lens == lens;
                if ui.selectable_label(selected, lens.label()).clicked() {
                    self.active_lens = lens;
                }
            }

            ui.separator();
            if ui.button("Undo").clicked() {
                let _ = self.exec_command("project.undo", json!({}));
            }
            if ui.button("Redo").clicked() {
                let _ = self.exec_command("project.redo", json!({}));
            }
            if ui.button("Run CPM").clicked() {
                let _ = self.exec_command("schedule.run", json!({}));
            }
            if ui.button("Level Resources").clicked() {
                let reply = self.exec_command("resource.level", json!({}));
                if let Some(actions) = reply.pointer("/result/actions").and_then(Json::as_array) {
                    self.level_log = actions.iter().filter_map(|a| a.get("reason").and_then(Json::as_str).map(str::to_string)).collect();
                }
            }
            if ui.button("Capture Baseline").clicked() {
                let _ = self.exec_command("baseline.capture", json!({}));
            }
            if ui.button("Demo Project").clicked() {
                let _ = self.exec_command("project.seed_demo", json!({}));
            }

            #[cfg(not(target_arch = "wasm32"))]
            {
                ui.separator();
                if ui.button("Open…").clicked() {
                    if let Some(path) = rfd::FileDialog::new().add_filter("Project Files", &["projectcraft", "json", "csv", "xml", "xer"]).pick_file()
                    {
                        let p = path.to_string_lossy().to_string();
                        let _ = self.exec_command("project.open", json!({"path": p}));
                    }
                }
                if ui.button("Save…").clicked() {
                    if let Some(path) = rfd::FileDialog::new().set_file_name("project.projectcraft").save_file() {
                        let p = path.to_string_lossy().to_string();
                        let _ = self.exec_command("project.save", json!({"path": p}));
                    }
                }
            }

            ui.separator();
            let theme_lbl = match self.theme {
                ThemeMode::Dark => "Light Theme",
                ThemeMode::Light => "Dark Theme",
            };
            if ui.button(theme_lbl).clicked() {
                self.theme = match self.theme {
                    ThemeMode::Dark => ThemeMode::Light,
                    ThemeMode::Light => ThemeMode::Dark,
                };
                apply_studio_theme(ctx, self.theme);
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(store) = &mut self.settings_store {
                    let t_str = match self.theme {
                        ThemeMode::Dark => "dark",
                        ThemeMode::Light => "light",
                    };
                    store.update(|p| p.theme = t_str.to_string());
                    let _ = store.flush();
                }
            }
        });
    }

    fn draw_table_lens(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("New Task:");
            ui.add(egui::TextEdit::singleline(&mut self.new_task_title).hint_text("Task title…").desired_width(200.0));
            ui.label("WBS:");
            ui.add(egui::TextEdit::singleline(&mut self.new_task_wbs).hint_text("auto").desired_width(60.0));
            ui.label("Days:");
            ui.add(egui::DragValue::new(&mut self.new_task_duration).speed(0.5).range(0.0..=365.0));
            if ui.button("+ Add Task").clicked() && !self.new_task_title.trim().is_empty() {
                let mut params = json!({
                    "title": self.new_task_title.trim(),
                    "duration_days": self.new_task_duration,
                });
                if !self.new_task_wbs.trim().is_empty() {
                    params["wbs"] = json!(self.new_task_wbs.trim());
                }
                let _ = self.exec_command("task.add", params);
                self.new_task_title.clear();
                self.new_task_wbs.clear();
            }
            ui.separator();
            ui.checkbox(&mut self.critical_only_filter, "Critical path only");
        });
        ui.add_space(6.0);

        let variances = compute_baseline_variance(self.session.project(), None);
        let tasks_snapshot = self.session.project().tasks.clone();
        let mut pending_updates: Vec<(String, Json)> = Vec::new();

        egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
            egui::Grid::new("wbs_table_grid").striped(true).min_col_width(56.0).show(ui, |ui| {
                ui.strong("WBS");
                ui.strong("Key");
                ui.strong("Title");
                ui.strong("Kind");
                ui.strong("Status");
                ui.strong("Priority");
                ui.strong("Dur (d)");
                ui.strong("Progress");
                ui.strong("ES – EF");
                ui.strong("Slack");
                ui.strong("Crit");
                ui.strong("Var (d)");
                ui.strong("Planned $");
                ui.end_row();

                for t in &tasks_snapshot {
                    if self.critical_only_filter && !t.critical {
                        continue;
                    }
                    let is_sel = self.selected_task == Some(t.id);
                    ui.label(&t.wbs);
                    if ui.selectable_label(is_sel, &t.key).clicked() {
                        self.selected_task = Some(t.id);
                    }
                    let indent = "  ".repeat(t.wbs_depth());
                    if ui.selectable_label(is_sel, format!("{indent}{}", t.title)).clicked() {
                        self.selected_task = Some(t.id);
                    }
                    ui.label(t.kind.as_str());

                    let mut cur_status = t.status;
                    egui::ComboBox::from_id_salt(("st", t.id)).selected_text(cur_status.label()).width(96.0).show_ui(ui, |ui| {
                        for st in TaskStatus::ALL {
                            ui.selectable_value(&mut cur_status, st, st.label());
                        }
                    });
                    if cur_status != t.status {
                        pending_updates.push(("task.set_status".to_string(), json!({"id": t.id, "status": cur_status.as_str()})));
                    }

                    ui.label(t.priority.as_str());

                    let mut dur = t.duration_days;
                    if ui.add(egui::DragValue::new(&mut dur).speed(0.5).range(0.0..=365.0)).changed() {
                        pending_updates.push(("task.update".to_string(), json!({"id": t.id, "duration_days": dur})));
                    }

                    let mut pct = (t.progress * 100.0).round() as i32;
                    if ui.add(egui::Slider::new(&mut pct, 0..=100).suffix("%")).changed() {
                        pending_updates.push(("task.set_progress".to_string(), json!({"id": t.id, "progress": f64::from(pct) / 100.0})));
                    }

                    ui.label(format!("{:.1} – {:.1}", t.early_start, t.early_finish));
                    ui.label(format!("{:.1}d", t.total_slack));
                    if t.critical {
                        ui.colored_label(egui::Color32::from_rgb(248, 81, 73), "CRIT");
                    } else {
                        ui.label("—");
                    }
                    let finish_var = variances.iter().find(|v| v.task_id == t.id).map(|v| v.finish_variance_days).unwrap_or(0.0);
                    ui.label(format!("{finish_var:+.1}d"));
                    ui.label(format!("${:.0}", self.session.project().planned_task_cost(t.id)));
                    ui.end_row();
                }
            });
        });

        for (cmd, p) in pending_updates {
            let _ = self.exec_command(&cmd, p);
        }
    }

    fn draw_gantt_lens(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Timeline Zoom (px/day):");
            ui.add(egui::Slider::new(&mut self.gantt_day_width, 16.0..=64.0));
            ui.separator();
            ui.colored_label(egui::Color32::from_rgb(248, 81, 73), "■ Critical Path");
            ui.colored_label(egui::Color32::from_rgb(88, 166, 255), "■ Non-Critical");
            ui.colored_label(egui::Color32::from_rgb(139, 148, 158), "▬ Baseline 0");
            ui.colored_label(egui::Color32::from_rgb(227, 179, 65), "┆ Status Day");
        });
        ui.add_space(4.0);

        let proj = self.session.project();
        let row_h = 30.0_f32;
        let header_h = 34.0_f32;
        let left_w = 270.0_f32;
        let day_w = self.gantt_day_width;
        let max_day = proj.tasks.iter().map(|t| t.early_finish.ceil() as usize).max().unwrap_or(10).max(12) + 2;
        let canvas_w = left_w + (max_day as f32) * day_w + 24.0;
        let canvas_h = header_h + (proj.tasks.len().max(1) as f32) * row_h + 20.0;

        egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
            let (rect, response) = ui.allocate_exact_size(egui::vec2(canvas_w, canvas_h), egui::Sense::click());
            let painter = ui.painter_at(rect);

            painter.rect_filled(rect, 4.0, egui::Color32::from_rgb(15, 20, 28));
            let hdr_rect = egui::Rect::from_min_size(rect.min, egui::vec2(canvas_w, header_h));
            painter.rect_filled(hdr_rect, 4.0, egui::Color32::from_rgb(22, 29, 39));

            painter.text(
                rect.min + egui::vec2(10.0, 9.0),
                egui::Align2::LEFT_TOP,
                "WBS · Key · Task Title",
                egui::FontId::proportional(12.5),
                egui::Color32::from_rgb(230, 237, 243),
            );

            // Day grid lines & headers
            for d in 0..=max_day {
                let x = rect.min.x + left_w + (d as f32) * day_w;
                let stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(33, 41, 54));
                painter.line_segment([egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)], stroke);
                if d % 2 == 0 {
                    painter.text(
                        egui::pos2(x + 3.0, rect.min.y + 9.0),
                        egui::Align2::LEFT_TOP,
                        format!("D{d}"),
                        egui::FontId::proportional(11.0),
                        egui::Color32::from_rgb(139, 148, 158),
                    );
                }
            }

            let baseline = proj.baselines.first();
            let mut row_by_id = std::collections::BTreeMap::new();

            for (idx, t) in proj.tasks.iter().enumerate() {
                row_by_id.insert(t.id, idx);
                let y = rect.min.y + header_h + (idx as f32) * row_h;
                let row_rect = egui::Rect::from_min_size(egui::pos2(rect.min.x, y), egui::vec2(canvas_w, row_h));
                if self.selected_task == Some(t.id) {
                    painter.rect_filled(row_rect, 0.0, egui::Color32::from_rgba_unmultiplied(56, 139, 253, 36));
                } else if idx % 2 == 1 {
                    painter.rect_filled(row_rect, 0.0, egui::Color32::from_rgba_unmultiplied(22, 27, 34, 140));
                }

                if response.clicked() {
                    if let Some(pos) = response.interact_pointer_pos() {
                        if row_rect.contains(pos) {
                            self.selected_task = Some(t.id);
                        }
                    }
                }

                painter.text(
                    egui::pos2(rect.min.x + 10.0, y + 7.0),
                    egui::Align2::LEFT_TOP,
                    format!("{} {} {}", t.wbs, t.key, t.title),
                    egui::FontId::proportional(12.0),
                    egui::Color32::from_rgb(208, 215, 222),
                );

                // Baseline bar
                if let Some(bt) = baseline.and_then(|b| b.tasks.iter().find(|bt| bt.task_id == t.id)) {
                    if bt.duration_days > 0.0 {
                        let bx = rect.min.x + left_w + (bt.start_day as f32) * day_w;
                        let bw = ((bt.duration_days as f32) * day_w).max(4.0);
                        let brect = egui::Rect::from_min_size(egui::pos2(bx, y + 21.0), egui::vec2(bw, 4.0));
                        painter.rect_filled(brect, 2.0, egui::Color32::from_rgb(110, 118, 129));
                    }
                }

                let x0 = rect.min.x + left_w + (t.early_start as f32) * day_w;
                if t.kind == ItemKind::Milestone || t.duration_days <= 0.0 {
                    let cy = y + 14.0;
                    let col = if t.critical { egui::Color32::from_rgb(248, 81, 73) } else { egui::Color32::from_rgb(210, 153, 34) };
                    let pts = vec![egui::pos2(x0, cy - 6.0), egui::pos2(x0 + 6.0, cy), egui::pos2(x0, cy + 6.0), egui::pos2(x0 - 6.0, cy)];
                    painter.add(egui::Shape::convex_polygon(pts, col, egui::Stroke::new(1.0, egui::Color32::WHITE)));
                } else {
                    let bw = ((t.duration_days as f32) * day_w).max(6.0);
                    let bar_rect = egui::Rect::from_min_size(egui::pos2(x0, y + 6.0), egui::vec2(bw, 13.0));
                    let (bg, fg) = if t.critical {
                        (egui::Color32::from_rgba_unmultiplied(218, 54, 51, 110), egui::Color32::from_rgb(255, 123, 114))
                    } else {
                        (egui::Color32::from_rgba_unmultiplied(47, 129, 247, 110), egui::Color32::from_rgb(88, 166, 255))
                    };
                    painter.rect_filled(bar_rect, 3.0, bg);
                    painter.rect_stroke(bar_rect, 3.0, egui::Stroke::new(1.0, fg), egui::StrokeKind::Middle);
                    let prog_w = bw * (t.progress.clamp(0.0, 1.0) as f32);
                    if prog_w > 1.0 {
                        let p_rect = egui::Rect::from_min_size(bar_rect.min, egui::vec2(prog_w, bar_rect.height()));
                        painter.rect_filled(p_rect, 3.0, fg);
                    }
                }
            }

            // Dependency connector lines
            for dep in &proj.dependencies {
                if let (Some(&p_row), Some(&s_row), Some(p_task), Some(s_task)) =
                    (row_by_id.get(&dep.predecessor_id), row_by_id.get(&dep.successor_id), proj.task(dep.predecessor_id), proj.task(dep.successor_id))
                {
                    let x1 = rect.min.x + left_w + (p_task.early_finish as f32) * day_w;
                    let y1 = rect.min.y + header_h + (p_row as f32) * row_h + 12.5;
                    let x2 = rect.min.x + left_w + (s_task.early_start as f32) * day_w;
                    let y2 = rect.min.y + header_h + (s_row as f32) * row_h + 12.5;
                    let mid = (x1 + 6.0).min(x2 - 3.0).max(x1 + 3.0);
                    let stroke = egui::Stroke::new(
                        1.2,
                        if p_task.critical && s_task.critical {
                            egui::Color32::from_rgb(255, 123, 114)
                        } else {
                            egui::Color32::from_rgb(139, 148, 158)
                        },
                    );
                    painter.line_segment([egui::pos2(x1, y1), egui::pos2(mid, y1)], stroke);
                    painter.line_segment([egui::pos2(mid, y1), egui::pos2(mid, y2)], stroke);
                    painter.line_segment([egui::pos2(mid, y2), egui::pos2(x2, y2)], stroke);
                }
            }

            // Status day vertical line
            let sx = rect.min.x + left_w + (proj.status_day as f32) * day_w;
            painter.line_segment(
                [egui::pos2(sx, rect.min.y + header_h), egui::pos2(sx, rect.max.y)],
                egui::Stroke::new(1.5, egui::Color32::from_rgb(227, 179, 65)),
            );
        });
    }

    fn draw_board_lens(&mut self, ui: &mut egui::Ui) {
        let sprints = self.session.project().sprints.clone();
        ui.horizontal(|ui| {
            ui.label("Sprint:");
            if ui.selectable_label(self.selected_sprint.is_none(), "All Sprints").clicked() {
                self.selected_sprint = None;
            }
            for sp in &sprints {
                if ui.selectable_label(self.selected_sprint == Some(sp.id), &sp.name).clicked() {
                    self.selected_sprint = Some(sp.id);
                }
            }
        });

        if let Some(bd) = compute_sprint_burndown(self.session.project(), self.selected_sprint) {
            ui.horizontal(|ui| {
                ui.strong(format!("{} — Capacity: {:.0} pts", bd.sprint_name, bd.capacity_points));
                ui.separator();
                ui.label(format!("Committed: {:.1} pts", bd.committed_points));
                ui.label(format!("Completed: {:.1} pts", bd.completed_points));
                ui.label(format!("Remaining: {:.1} pts", bd.remaining_points));
                let frac = if bd.committed_points > 0.0 { (bd.completed_points / bd.committed_points).clamp(0.0, 1.0) as f32 } else { 0.0 };
                ui.add(egui::ProgressBar::new(frac).desired_width(160.0).show_percentage());
            });
        }
        ui.separator();

        let tasks = self.session.project().tasks.clone();
        let wip_limits = self.session.project().wip_limits.clone();
        let mut moves: Vec<(u32, TaskStatus)> = Vec::new();

        egui::ScrollArea::horizontal().auto_shrink([false, false]).show(ui, |ui| {
            ui.columns(TaskStatus::ALL.len(), |cols| {
                for (col_idx, status) in TaskStatus::ALL.iter().copied().enumerate() {
                    let Some(col_ui) = cols.get_mut(col_idx) else { continue };
                    let cards: Vec<&pmcore::Task> =
                        tasks.iter().filter(|t| t.status == status && self.selected_sprint.map(|s| t.sprint_id == Some(s)).unwrap_or(true)).collect();
                    let limit = wip_limits.get(status.as_str()).copied();
                    let over_wip = limit.map(|l| cards.len() > l).unwrap_or(false);

                    let hdr = match limit {
                        Some(l) => format!("{} ({}/{l})", status.label(), cards.len()),
                        None => format!("{} ({})", status.label(), cards.len()),
                    };
                    if over_wip {
                        col_ui.colored_label(egui::Color32::from_rgb(248, 81, 73), egui::RichText::new(hdr).strong());
                    } else {
                        col_ui.strong(hdr);
                    }
                    col_ui.separator();

                    for card in cards {
                        egui::Frame::group(col_ui.style()).show(col_ui, |ui| {
                            ui.horizontal(|ui| {
                                if ui.selectable_label(self.selected_task == Some(card.id), &card.key).clicked() {
                                    self.selected_task = Some(card.id);
                                }
                                if card.critical {
                                    ui.colored_label(egui::Color32::from_rgb(248, 81, 73), "CRIT");
                                }
                                if let Some(pts) = card.story_points {
                                    ui.label(format!("{pts:.0}pt"));
                                }
                            });
                            ui.label(&card.title);
                            ui.horizontal(|ui| {
                                if col_idx > 0 {
                                    if let Some(&prev_st) = TaskStatus::ALL.get(col_idx - 1) {
                                        if ui.small_button("◀").on_hover_text(prev_st.label()).clicked() {
                                            moves.push((card.id, prev_st));
                                        }
                                    }
                                }
                                ui.label(format!("{:.0}%", card.progress * 100.0));
                                if let Some(&next_st) = TaskStatus::ALL.get(col_idx + 1) {
                                    if ui.small_button("▶").on_hover_text(next_st.label()).clicked() {
                                        moves.push((card.id, next_st));
                                    }
                                }
                            });
                        });
                        col_ui.add_space(4.0);
                    }
                }
            });
        });

        for (id, st) in moves {
            let _ = self.exec_command("task.set_status", json!({"id": id, "status": st.as_str()}));
        }
    }

    fn draw_resources_lens(&mut self, ui: &mut egui::Ui) {
        let wl = compute_workload(self.session.project());
        ui.horizontal(|ui| {
            ui.strong(format!("Resource Pool ({}) — Horizon: {}d", wl.resources.len(), wl.horizon_days));
            ui.separator();
            if wl.total_overallocated_person_days > 0 {
                ui.colored_label(
                    egui::Color32::from_rgb(248, 81, 73),
                    format!("⚠ {} overallocated person-days detected", wl.total_overallocated_person_days),
                );
            } else {
                ui.colored_label(egui::Color32::from_rgb(63, 185, 80), "✓ Zero resource overallocations");
            }
            if ui.button("Auto-Level Overallocations").clicked() {
                let reply = self.exec_command("resource.level", json!({}));
                if let Some(actions) = reply.pointer("/result/actions").and_then(Json::as_array) {
                    self.level_log = actions.iter().filter_map(|a| a.get("reason").and_then(Json::as_str).map(str::to_string)).collect();
                }
            }
        });

        if !self.level_log.is_empty() {
            ui.add_space(4.0);
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.strong("Resource Levelling Explanations:");
                for line in &self.level_log {
                    ui.label(format!("• {line}"));
                }
            });
        }
        ui.add_space(8.0);

        egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
            egui::Grid::new("resource_heatmap_grid").striped(true).show(ui, |ui| {
                ui.strong("Resource");
                ui.strong("Role");
                ui.strong("Max");
                ui.strong("Peak");
                let days_shown = wl.horizon_days.min(28);
                for d in 0..days_shown {
                    ui.strong(format!("D{d}"));
                }
                ui.end_row();

                for r in &wl.resources {
                    ui.label(&r.name);
                    ui.label(&r.role);
                    ui.label(format!("{:.0}%", r.max_units * 100.0));
                    ui.label(format!("{:.0}%", r.peak_units * 100.0));
                    for d in 0..days_shown {
                        let u = r.daily_units.get(d).copied().unwrap_or(0.0);
                        let txt = format!("{:.0}%", u * 100.0);
                        if u > r.max_units + 1e-6 {
                            ui.colored_label(egui::Color32::from_rgb(248, 81, 73), txt);
                        } else if u > 0.0 {
                            ui.colored_label(egui::Color32::from_rgb(88, 166, 255), txt);
                        } else {
                            ui.label("—");
                        }
                    }
                    ui.end_row();
                }
            });
        });
    }

    fn draw_evm_risk_lens(&mut self, ui: &mut egui::Ui) {
        let mut status_day = self.session.project().status_day;
        ui.horizontal(|ui| {
            ui.label("Status Day (working days):");
            if ui.add(egui::Slider::new(&mut status_day, 0.0..=30.0)).changed() {
                let _ = self.exec_command("project.set_status_day", json!({"status_day": status_day}));
            }
            ui.separator();
            ui.label("Monte Carlo Trials:");
            ui.add(egui::DragValue::new(&mut self.mc_trials).range(50..=5000).speed(50));
            if ui.button("Run Monte Carlo Risk Simulation").clicked() {
                let _ = self.exec_command("schedule.monte_carlo", json!({"trials": self.mc_trials, "seed": 42}));
            }
        });
        ui.separator();

        let evm = compute_evm(self.session.project(), None);
        ui.horizontal_wrapped(|ui| {
            kpi_card(ui, "BAC (Budget)", &format!("${:.0}", evm.bac));
            kpi_card(ui, "PV (Planned)", &format!("${:.0}", evm.pv));
            kpi_card(ui, "EV (Earned)", &format!("${:.0}", evm.ev));
            kpi_card(ui, "AC (Actual)", &format!("${:.0}", evm.ac));
            kpi_card(ui, "SV / CV", &format!("${:+.0} / ${:+.0}", evm.sv, evm.cv));
            kpi_card(ui, "SPI / CPI", &format!("{:.2} / {:.2}", evm.spi, evm.cpi));
            kpi_card(ui, "EAC (Forecast)", &format!("${:.0}", evm.eac));
            kpi_card(ui, "VAC / TCPI", &format!("${:+.0} / {:.2}", evm.vac, evm.tcpi));
        });

        ui.add_space(8.0);
        if let Some(mc) = self.session.last_monte_carlo().cloned() {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.strong(format!(
                    "Monte Carlo Schedule-Risk Distribution ({} trials) — Deterministic: {:.1}d | P10: {:.1}d | P50: {:.1}d | P80: {:.1}d | P90: {:.1}d",
                    mc.trials, mc.deterministic_duration_days, mc.p10_days, mc.p50_days, mc.p80_days, mc.p90_days
                ));
                ui.add_space(4.0);
                egui::Grid::new("mc_crit_grid").striped(true).show(ui, |ui| {
                    ui.strong("Key");
                    ui.strong("Task");
                    ui.strong("Mean Sim Duration");
                    ui.strong("Criticality Index");
                    ui.end_row();
                    for c in &mc.criticality {
                        ui.label(&c.key);
                        ui.label(&c.title);
                        ui.label(format!("{:.2}d", c.mean_duration_days));
                        ui.label(format!("{:.0}%", c.criticality_index * 100.0));
                        ui.end_row();
                    }
                });
            });
        }
    }

    fn draw_inspector(&mut self, ui: &mut egui::Ui) {
        ui.heading("Task Inspector");
        ui.separator();
        let Some(sel_id) = self.selected_task else {
            ui.label("Select a task to inspect or edit.");
            return;
        };
        let Some(task) = self.session.project().task(sel_id).cloned() else {
            ui.label("Selected task no longer exists.");
            return;
        };

        ui.strong(format!("{} — {}", task.key, task.title));
        ui.label(format!(
            "WBS: {} | Early: {:.1}d – {:.1}d | Late: {:.1}d – {:.1}d",
            task.wbs, task.early_start, task.early_finish, task.late_start, task.late_finish
        ));
        ui.label(format!("Total Slack: {:.1}d | Free Slack: {:.1}d | Constraint: {}", task.total_slack, task.free_slack, task.constraint.label()));
        ui.separator();

        let mut fixed_cost = task.fixed_cost;
        let mut actual_cost = task.actual_cost;
        let mut opt = task.optimistic_days.unwrap_or(task.duration_days * 0.8);
        let mut ml = task.most_likely_days.unwrap_or(task.duration_days);
        let mut pess = task.pessimistic_days.unwrap_or(task.duration_days * 1.4);

        ui.horizontal(|ui| {
            ui.label("Fixed Cost ($):");
            if ui.add(egui::DragValue::new(&mut fixed_cost).speed(25.0).range(0.0..=1_000_000.0)).changed() {
                let _ = self.exec_command("task.update", json!({"id": sel_id, "fixed_cost": fixed_cost}));
            }
        });
        ui.horizontal(|ui| {
            ui.label("Actual Cost ($):");
            if ui.add(egui::DragValue::new(&mut actual_cost).speed(25.0).range(0.0..=1_000_000.0)).changed() {
                let _ = self.exec_command("task.update", json!({"id": sel_id, "actual_cost": actual_cost}));
            }
        });

        ui.separator();
        ui.strong("Three-Point PERT Estimates (days)");
        let mut pert_changed = false;
        ui.horizontal(|ui| {
            ui.label("O:");
            pert_changed |= ui.add(egui::DragValue::new(&mut opt).speed(0.25).range(0.0..=365.0)).changed();
            ui.label("M:");
            pert_changed |= ui.add(egui::DragValue::new(&mut ml).speed(0.25).range(0.0..=365.0)).changed();
            ui.label("P:");
            pert_changed |= ui.add(egui::DragValue::new(&mut pess).speed(0.25).range(0.0..=365.0)).changed();
        });
        if pert_changed {
            let _ = self.exec_command(
                "task.update",
                json!({"id": sel_id, "optimistic_days": opt, "most_likely_days": ml.max(opt), "pessimistic_days": pess.max(ml)}),
            );
        }

        ui.separator();
        ui.strong("Predecessors");
        let preds: Vec<(u32, DepKind, f64)> =
            self.session.project().dependencies.iter().filter(|d| d.successor_id == sel_id).map(|d| (d.predecessor_id, d.kind, d.lag_days)).collect();
        let mut to_remove: Option<u32> = None;
        for (pid, kind, lag) in preds {
            ui.horizontal(|ui| {
                ui.label(format!("#{pid} ({}, lag {lag:+.1}d)", kind.code()));
                if ui.small_button("✕").clicked() {
                    to_remove = Some(pid);
                }
            });
        }
        if let Some(pid) = to_remove {
            let _ = self.exec_command("dep.remove", json!({"predecessor_id": pid, "successor_id": sel_id}));
        }

        ui.horizontal(|ui| {
            ui.label("Pred ID:");
            ui.add(egui::DragValue::new(&mut self.new_dep_pred).range(1..=9999));
            egui::ComboBox::from_id_salt("dep_kind_combo").selected_text(self.new_dep_kind.code()).width(52.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.new_dep_kind, DepKind::FinishToStart, "FS");
                ui.selectable_value(&mut self.new_dep_kind, DepKind::StartToStart, "SS");
                ui.selectable_value(&mut self.new_dep_kind, DepKind::FinishToFinish, "FF");
                ui.selectable_value(&mut self.new_dep_kind, DepKind::StartToFinish, "SF");
            });
            if ui.button("+ Link").clicked() {
                let _ = self.exec_command(
                    "dep.add",
                    json!({
                        "predecessor_id": self.new_dep_pred,
                        "successor_id": sel_id,
                        "kind": self.new_dep_kind.code(),
                        "lag_days": self.new_dep_lag,
                    }),
                );
            }
        });

        ui.separator();
        if ui.button("Delete Task").clicked() {
            let _ = self.exec_command("task.delete", json!({"id": sel_id}));
            self.selected_task = self.session.project().tasks.first().map(|t| t.id);
        }
    }
}

fn kpi_card(ui: &mut egui::Ui, label: &str, value: &str) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_min_width(120.0);
        ui.small(label);
        ui.strong(egui::RichText::new(value).size(16.0));
    });
}

impl eframe::App for ProjectApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Some(req) = self.control_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            control::handle(self, ctx, req);
        }
        self.flush_screenshots(ctx);
        if self.control_rx.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        craft_quit::on_close_request(ctx);
    }

    fn on_exit(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(store) = &mut self.settings_store {
            let gw = self.gantt_day_width;
            store.update(|p| p.gantt_day_width = gw);
            let _ = store.flush();
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        egui::Panel::top("top_ribbon").show(ui, |ui| {
            self.draw_top_ribbon(&ctx, ui);
        });

        egui::Panel::bottom("bottom_command_bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Cmd:");
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.command_line)
                        .hint_text("e.g. task.add {\"title\":\"Audit\",\"duration_days\":2}")
                        .desired_width(420.0),
                );
                if (resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) || ui.button("Execute").clicked() {
                    let raw = self.command_line.trim().to_string();
                    let (id, rest) = raw.split_once(char::is_whitespace).unwrap_or((raw.as_str(), "{}"));
                    if let Ok(params) = serde_json::from_str::<Json>(rest.trim()) {
                        let _ = self.exec_command(id, params);
                    } else {
                        self.status_banner = format!("Invalid JSON parameters for `{id}`");
                    }
                }
                ui.separator();
                ui.label(&self.status_banner);
            });
        });

        egui::Panel::right("right_task_inspector").default_size(290.0).resizable(true).show(ui, |ui| {
            self.draw_inspector(ui);
        });

        egui::CentralPanel::default().show(ui, |ui| match self.active_lens {
            Lens::Table => self.draw_table_lens(ui),
            Lens::Gantt => self.draw_gantt_lens(ui),
            Lens::Board => self.draw_board_lens(ui),
            Lens::Resources => self.draw_resources_lens(ui),
            Lens::EvmRisk => self.draw_evm_risk_lens(ui),
        });
    }
}

fn encode_color_image_png(img: &egui::ColorImage) -> Vec<u8> {
    let mut out = Vec::new();
    let w = u32::try_from(img.width()).unwrap_or(1);
    let h = u32::try_from(img.height()).unwrap_or(1);
    let mut rgba = Vec::with_capacity(img.pixels.len().saturating_mul(4));
    for p in &img.pixels {
        rgba.extend_from_slice(&p.to_array());
    }
    let mut enc = png::Encoder::new(&mut out, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    if let Ok(mut writer) = enc.write_header() {
        let _ = writer.write_image_data(&rgba);
    }
    out
}
