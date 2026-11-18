//! Tab 1: pick a folder, choose samples, set the length filter, run the count.

use egui::Ui;

use super::{ACCENT, ERR, MUTED, OK};
use crate::state::{Actions, AppState, RunKind};


pub fn browse_tab(ui: &mut Ui, state: &mut AppState, actions: &mut Actions) {
    folder_row(ui, state, actions);
    filter_row(ui, state);

    let files = state.visible_files().to_vec();
    let shown: Vec<usize> = files
        .iter()
        .enumerate()
        .filter(|(_, path)| state.is_selected_by_filter(path))
        .map(|(index, _)| index)
        .collect();

    ui.horizontal(|ui| {
        ui.label(format!("{} sample files", files.len()));
        ui.label(egui::RichText::new(format!("{} selected", state.selected.len())).color(ACCENT));
        if ui.button("Select all").clicked() {
            state.select_all_visible();
        }
        if ui.button("Select none").clicked() {
            state.select_none();
        }
        if ui.button("Select filtered").clicked() {
            for &index in &shown {
                state.selected.insert(files[index].clone());
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            length_filter(ui, state);
        });
    });
    ui.add_space(4.0);

    if files.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(
                egui::RichText::new(
                    "No *_unbinned_reads files here. Press O to choose another folder.",
                )
                .color(MUTED),
            );
        });
        return;
    }

    ui.label(
        egui::RichText::new(format!(
            "Files are listed in the natural order the TUI uses (sample2 before sample10)."
        ))
        .small()
        .color(MUTED),
    );
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for &index in &shown {
                let path = &files[index];
                let sample = noise_core::sample_name_from_path(path);
                let mut toggle = state.selected.contains(path);
                let file_name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("")
                    .to_string();
                ui.horizontal(|ui| {
                    if ui
                        .checkbox(&mut toggle, "")
                        .on_hover_text("Include this sample in the run")
                        .changed()
                    {
                        state.toggle_select(path);
                    }
                    ui.label(egui::RichText::new(sample).strong());
                    ui.label(egui::RichText::new(file_name).small().color(MUTED));
                });
            }
        });

    run_row(ui, state, actions);
}

fn folder_row(ui: &mut Ui, state: &mut AppState, actions: &mut Actions) {
    ui.horizontal(|ui| {
        if ui.button("Open folder…").clicked() {
            actions.pick_folder = true;
        }
        ui.label(
            egui::RichText::new(state.input_folder.display().to_string())
                .monospace()
                .small(),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let results = state.results_dir();
            ui.label(
                egui::RichText::new(format!("outputs → {}", results.display()))
                    .small()
                    .color(MUTED),
            );
        });
    });
    ui.add_space(4.0);
}

fn filter_row(ui: &mut Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        ui.label("Filter:");
        let response = ui.add(
            egui::TextEdit::singleline(&mut state.filter_text)
                .hint_text("sample name or part of a file name")
                .desired_width(260.0),
        );
        if !state.filter_text.is_empty() && ui.button("clear").clicked() {
            state.filter_text.clear();
        }
        let _ = response;
    });
    ui.add_space(4.0);
}

/// The length filter applies live to both tables — the TUI made you press Enter,
/// which is easy to forget, so here it is validated on every keystroke.
fn length_filter(ui: &mut Ui, state: &mut AppState) {
    ui.label("Length filter:");
    let response = ui.add(
        egui::TextEdit::singleline(&mut state.length_filter)
            .desired_width(90.0)
            .hint_text("≥ 0, or empty"),
    );
    if let Err(message) = noise_core::parse_length_filter(&state.length_filter) {
        state.filter_error = Some(message);
        ui.label(egui::RichText::new(state.filter_error.clone().unwrap_or_default()).color(ERR));
    } else {
        state.filter_error = None;
        if let Some(filter) = state.parsed_length_filter() {
            ui.label(
                egui::RichText::new(format!("keeping reads of {filter} bases"))
                    .small()
                    .color(OK),
            );
        }
    }
    // Changing the filter changes every count, so the live numbers go stale.
    if response.changed() {
        state.clear_live();
    }
}

fn run_row(ui: &mut Ui, state: &mut AppState, actions: &mut Actions) {
    ui.add_space(6.0);
    ui.separator();
    ui.horizontal(|ui| {
        let busy = state.job.is_running();
        let button = egui::Button::new(
            egui::RichText::new(if busy { "Counting…" } else { "Count selected" })
                .strong()
                .color(if busy { MUTED } else { ACCENT }),
        );
        ui.add_enabled_ui(!busy, |ui| {
            if ui.add(button).clicked() {
                actions.run = Some(RunKind::Count);
            }
        });
        ui.label(
            egui::RichText::new(format!(
                "{} file{} selected",
                state.selected.len(),
                if state.selected.len() == 1 { "" } else { "s" }
            ))
            .small()
            .color(MUTED),
        );
    });
}
