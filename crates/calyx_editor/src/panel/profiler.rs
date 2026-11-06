//! The profiler panel: frame time, the CPU and GPU time of recent frames,
//! and every CPU scope and GPU pass averaged over a window of frames.

use std::any::Any;
use std::collections::HashSet;
use std::time::{Duration, Instant};

use calyx_engine::profiler::{self, ProfileReport, ScopeStats, TimeStats};
use egui::{Color32, RichText, Sense, Stroke, Ui};

use crate::panel::Panel;
use crate::EditorAppState;

const DEFAULT_WINDOW: usize = 120;
/// How often the shown report is rebuilt from the history.
const REFRESH: Duration = Duration::from_millis(250);
const GRAPH_HEIGHT: f32 = 60.0;
const FRAME_COLOR: Color32 = Color32::from_rgb(90, 90, 90);
const CPU_COLOR: Color32 = Color32::from_rgb(80, 150, 230);
const GPU_COLOR: Color32 = Color32::from_rgb(235, 150, 60);

pub struct PanelProfiler {
    window: usize,
    report: Option<ProfileReport>,
    /// `(frame, cpu, gpu)` milliseconds of the frames in the window, oldest
    /// first.
    recent: Vec<(f64, f64, Option<f64>)>,
    refreshed: Option<Instant>,
    /// Rows the user collapsed, by their path of names.
    collapsed: HashSet<String>,
}

impl Default for PanelProfiler {
    fn default() -> Self {
        Self {
            window: DEFAULT_WINDOW,
            report: None,
            recent: Vec::new(),
            refreshed: None,
            collapsed: HashSet::new(),
        }
    }
}

impl Panel for PanelProfiler {
    fn name() -> &'static str
    where
        Self: Sized,
    {
        "Profiler"
    }

    fn ui(&mut self, ui: &mut Ui, _state: &mut EditorAppState) {
        egui::Frame::NONE
            .inner_margin(egui::Margin::symmetric(10, 6))
            .show(ui, |ui| {
                self.toolbar(ui);
                self.refresh();
                ui.separator();
                let Some(report) = self.report.take() else {
                    ui.label("Turn capture on to record frames.");
                    return;
                };
                Self::summary(ui, &report);
                self.graph(ui);
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt("profiler_scopes")
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.breakdown(ui, &report));
                self.report = Some(report);
            });
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl PanelProfiler {
    fn toolbar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let mut capture = profiler::capture_requested();
            if ui.checkbox(&mut capture, "Capture").changed() {
                profiler::set_capture(capture);
                self.refreshed = None;
            }
            ui.label("Window");
            ui.add(
                egui::DragValue::new(&mut self.window)
                    .range(1..=profiler::HISTORY_FRAMES)
                    .suffix(" frames"),
            );
        });
    }

    /// Rebuilds the report from the history every [`REFRESH`], so reading
    /// it stays cheap and its numbers readable.
    fn refresh(&mut self) {
        if self
            .refreshed
            .is_some_and(|refreshed| refreshed.elapsed() < REFRESH)
        {
            return;
        }
        self.refreshed = Some(Instant::now());
        let report = profiler::report(self.window);
        if report.frames == 0 {
            self.report = None;
            self.recent.clear();
            return;
        }
        self.recent = profiler::with_history(|history| {
            let skip = history.len().saturating_sub(self.window);
            history
                .iter()
                .skip(skip)
                .map(|frame| {
                    (
                        ms(frame.frame_ns),
                        ms(frame.cpu_ns()),
                        frame.gpu_ns().map(ms),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
        self.report = Some(report);
    }

    fn summary(ui: &mut Ui, report: &ProfileReport) {
        egui::Grid::new("profiler_summary")
            .num_columns(4)
            .spacing([16.0, 2.0])
            .show(ui, |ui| {
                ui.label("");
                ui.strong("avg ms");
                ui.strong("max ms");
                ui.strong("last ms");
                ui.end_row();
                let fps = if report.frame.avg_ms > 0.0 {
                    1000.0 / report.frame.avg_ms
                } else {
                    0.0
                };
                stat_row(
                    ui,
                    &format!("Frame ({fps:.0} fps)"),
                    FRAME_COLOR,
                    &report.frame,
                );
                stat_row(ui, "CPU (main thread)", CPU_COLOR, &report.cpu);
                match &report.gpu {
                    Some(gpu) => stat_row(ui, "GPU", GPU_COLOR, gpu),
                    None => {
                        ui.colored_label(GPU_COLOR, "GPU");
                        ui.label("—");
                        ui.label("—");
                        ui.label("—");
                        ui.end_row();
                    }
                }
            });
        let bound = match &report.gpu {
            Some(gpu) if gpu.avg_ms > report.cpu.avg_ms => "GPU-bound",
            Some(_) => "CPU-bound",
            None => "GPU time unknown",
        };
        ui.label(format!(
            "{} frames from frame {}, {} with GPU timings: {bound}",
            report.frames, report.first_frame, report.gpu_frames
        ));
        if let Some(reason) = &report.gpu_timing.reason {
            ui.label(RichText::new(format!("GPU timing: {reason}")).weak());
        }
    }

    /// One column per recent frame: its frame time in grey, its main
    /// thread's CPU time in blue over it, and a tick at its GPU time.
    fn graph(&self, ui: &mut Ui) {
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, GRAPH_HEIGHT), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);
        let max = self
            .recent
            .iter()
            .map(|(frame, cpu, gpu)| frame.max(*cpu).max(gpu.unwrap_or(0.0)))
            .fold(1.0_f64, f64::max);
        let count = self.recent.len().max(1) as f32;
        let column = rect.width() / count;
        let height = |value: f64| (value / max) as f32 * rect.height();
        for (index, (frame, cpu, gpu)) in self.recent.iter().enumerate() {
            let left = rect.left() + index as f32 * column;
            let right = (left + column - 1.0).max(left + 1.0);
            let bar = |value: f64| {
                egui::Rect::from_min_max(
                    egui::pos2(left, rect.bottom() - height(value)),
                    egui::pos2(right, rect.bottom()),
                )
            };
            painter.rect_filled(bar(*frame), 0.0, FRAME_COLOR);
            painter.rect_filled(bar(*cpu), 0.0, CPU_COLOR);
            if let Some(gpu) = gpu {
                let y = rect.bottom() - height(*gpu);
                painter.line_segment(
                    [egui::pos2(left, y), egui::pos2(right, y)],
                    Stroke::new(2.0_f32, GPU_COLOR),
                );
            }
        }
        painter.text(
            rect.left_top() + egui::vec2(4.0, 2.0),
            egui::Align2::LEFT_TOP,
            format!("{max:.1} ms"),
            egui::FontId::monospace(10.0),
            ui.visuals().weak_text_color(),
        );
    }

    fn breakdown(&mut self, ui: &mut Ui, report: &ProfileReport) {
        egui::Grid::new("profiler_scopes_grid")
            .num_columns(5)
            .striped(true)
            .spacing([16.0, 2.0])
            .show(ui, |ui| {
                ui.strong("Scope");
                ui.strong("avg ms");
                ui.strong("max ms");
                ui.strong("last ms");
                ui.strong("calls");
                ui.end_row();
                for thread in &report.threads {
                    let path = format!("cpu/{}", thread.name);
                    if self.section_row(ui, &path, &format!("CPU: {}", thread.name)) {
                        self.scope_rows(ui, &path, &thread.scopes, 1);
                    }
                }
                if !report.gpu_passes.is_empty() && self.section_row(ui, "gpu", "GPU") {
                    self.scope_rows(ui, "gpu", &report.gpu_passes, 1);
                }
            });
    }

    /// A row heading one thread or the GPU. Returns whether it is expanded.
    fn section_row(&mut self, ui: &mut Ui, path: &str, title: &str) -> bool {
        let expanded = self.toggle(ui, path, title, 0);
        for _ in 0..4 {
            ui.label("");
        }
        ui.end_row();
        expanded
    }

    fn scope_rows(&mut self, ui: &mut Ui, parent: &str, scopes: &[ScopeStats], depth: usize) {
        for scope in scopes {
            let path = format!("{parent}/{}", scope.name);
            let expanded = if scope.children.is_empty() {
                ui.horizontal(|ui| {
                    ui.add_space(indent(depth) + 14.0);
                    ui.label(&scope.name);
                });
                false
            } else {
                self.toggle(ui, &path, &scope.name, depth)
            };
            ui.label(format!("{:.3}", scope.time.avg_ms));
            ui.label(format!("{:.3}", scope.time.max_ms));
            ui.label(format!("{:.3}", scope.time.last_ms));
            ui.label(format!("{:.1}", scope.calls));
            ui.end_row();
            if expanded {
                self.scope_rows(ui, &path, &scope.children, depth + 1);
            }
        }
    }

    /// A row title with an expand arrow. Returns whether it is expanded.
    fn toggle(&mut self, ui: &mut Ui, path: &str, title: &str, depth: usize) -> bool {
        let expanded = !self.collapsed.contains(path);
        ui.horizontal(|ui| {
            ui.add_space(indent(depth));
            let arrow = if expanded { "⏷" } else { "⏵" };
            if ui
                .add(egui::Label::new(format!("{arrow} {title}")).sense(Sense::click()))
                .clicked()
            {
                if expanded {
                    self.collapsed.insert(path.to_owned());
                } else {
                    self.collapsed.remove(path);
                }
            }
        });
        expanded
    }
}

fn stat_row(ui: &mut Ui, label: &str, color: Color32, stats: &TimeStats) {
    ui.colored_label(color, label);
    ui.label(format!("{:.2}", stats.avg_ms));
    ui.label(format!("{:.2}", stats.max_ms));
    ui.label(format!("{:.2}", stats.last_ms));
    ui.end_row();
}

fn indent(depth: usize) -> f32 {
    depth as f32 * 12.0
}

fn ms(ns: u64) -> f64 {
    ns as f64 / 1_000_000.0
}
