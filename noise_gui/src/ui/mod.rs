//! Widgets. Each tab is one file; everything shared lives here.

use egui::{Color32, Response, Ui};

pub mod browse;
pub mod charts;
pub mod patterns;
pub mod profile;
pub mod results;
pub mod search;

use crate::state::{AppState, JobState, MessageKind, Tab};

pub const MUTED: Color32 = Color32::from_gray(150);
pub const ACCENT: Color32 = Color32::from_rgb(90, 170, 250);
pub const WARN: Color32 = Color32::from_rgb(230, 190, 120);
pub const ERR: Color32 = Color32::from_rgb(240, 120, 120);
pub const OK: Color32 = Color32::from_rgb(140, 210, 140);

/// Table headers are buttons: clicking one re-sorts, and the arrow says which
/// column is doing the sorting.
pub fn header_cell(ui: &mut Ui, label: &str, active: bool, descending: bool) -> Response {
    let arrow = if active {
        if descending {
            " ▾"
        } else {
            " ▴"
        }
    } else {
        ""
    };
    let button = egui::Button::new(
        egui::RichText::new(format!("{label}{arrow}"))
            .strong()
            .color(if active { ACCENT } else { MUTED }),
    )
    .frame(active)
    .fill(if active {
        ui.visuals().faint_bg_color
    } else {
        Color32::TRANSPARENT
    });
    ui.add(button)
}

/// One line of the numbers a table shows, right-aligned so columns of digits
/// read as columns of digits.
pub fn number(ui: &mut Ui, value: impl std::fmt::Display) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.label(value.to_string());
    });
}

pub fn status_bar(ui: &mut Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        let folder = state.input_folder.display().to_string();
        ui.label(
            egui::RichText::new(shorten(&folder, 60))
                .small()
                .color(MUTED),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            match &state.job {
                JobState::Running { done, total } => {
                    if *total > 0 {
                        ui.add(
                            egui::ProgressBar::new(*done as f32 / *total as f32)
                                .text(format!("{done}/{total}"))
                                .animate(true),
                        );
                    } else {
                        ui.spinner();
                    }
                    if let Some(note) = &state.job_note {
                        ui.label(egui::RichText::new(note).small().color(ACCENT));
                    }
                }
                JobState::Finished { elapsed, files } => {
                    ui.label(
                        egui::RichText::new(format!(
                            "{files} file{} in {:.2}s",
                            if *files == 1 { "" } else { "s" },
                            elapsed.as_secs_f64()
                        ))
                        .small()
                        .color(OK),
                    );
                }
                JobState::Idle => {
                    if let Some(note) = &state.job_note {
                        ui.label(egui::RichText::new(note).small().color(MUTED));
                    }
                }
            }
        });
    });
    ui.separator();
}

pub fn tab_bar(ui: &mut Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        for tab in Tab::ALL {
            let selected = state.active_tab == tab;
            let text = egui::RichText::new(tab.title()).strong().color(if selected {
                ACCENT
            } else {
                MUTED
            });
            if ui.add(egui::Button::new(text).selected(selected)).clicked() {
                state.active_tab = tab;
            }
            ui.add_space(8.0);
        }
    });
    ui.separator();
}

/// The last few messages, always visible at the bottom, so an error can never
/// be missed just because it scrolled away.
pub fn footer(ui: &mut Ui, state: &mut AppState) {
    ui.separator();
    ui.horizontal_wrapped(|ui| {
        if state.messages.is_empty() {
            ui.label(
                egui::RichText::new(
                    "Ctrl+O folder  ·  Ctrl+R run  ·  Ctrl+F filter  ·  1–5 tabs  ·  R refresh  ·  Esc clear",
                )
                .small()
                .color(MUTED),
            );
            return;
        }
        for message in state.messages.iter().rev() {
            let color = match message.kind {
                MessageKind::Info => MUTED,
                MessageKind::Warning => WARN,
                MessageKind::Error => ERR,
            };
            let mark = match message.kind {
                MessageKind::Info => "",
                MessageKind::Warning => "⚠ ",
                MessageKind::Error => "✕ ",
            };
            ui.label(egui::RichText::new(format!("{mark}{}", message.text)).small().color(color));
            ui.label(egui::RichText::new("·").small().color(MUTED));
        }
    });
}

/// Confirms a run that would replace existing output. The TUI has always reused
/// fixed file names, so this guard protects data the old front end would have
/// silently clobbered.
pub fn confirm_overwrite(ctx: &egui::Context, files: &[std::path::PathBuf]) -> Option<bool> {
    let count = files.len();
    let mut answer = None;
    egui::Window::new("overwrite_confirmation")
        .collapsible(false)
        .resizable(false)
        .title_bar(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(format!(
                "{} output file{} already exist. Overwrite?",
                count,
                if count == 1 { "" } else { "s" }
            ));
            let shown = files
                .iter()
                .take(6)
                .map(|path| format!("  {}", path.display()))
                .collect::<Vec<_>>()
                .join("\n");
            let more = if count > 6 {
                format!("\n  … and {} more", count - 6)
            } else {
                String::new()
            };
            ui.label(
                egui::RichText::new(format!("{shown}{more}"))
                    .small()
                    .monospace()
                    .color(MUTED),
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Overwrite").clicked() {
                    answer = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    answer = Some(false);
                }
            });
        });
    answer
}

fn shorten(text: &str, max: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max {
        return text.to_string();
    }
    format!("…{}", chars[chars.len() - max..].iter().collect::<String>())
}
