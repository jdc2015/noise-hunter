//! Tab 2: the sequence query, its options, and the run button.
//!
//! The length filter and the query are one shared value, as in the TUI, so the
//! counts shown on tab 1 and the charts on tab 3 always agree with the search.

use egui::Ui;

use super::{ACCENT, MUTED};
use crate::state::{Actions, AppState, RunKind};

pub fn search_tab(ui: &mut Ui, state: &mut AppState, actions: &mut Actions) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Sequence:").strong());
        let response = ui.add(
            egui::TextEdit::singleline(&mut state.query)
                .hint_text("ACGT, 20 bases or longer")
                .desired_width(ui.available_width() - 20.0),
        );
        if response.changed() {
            state.clear_live();
        }
    });
    if let Some(error) = &state.query_error {
        ui.label(egui::RichText::new(error).color(egui::Color32::from_rgb(240, 120, 120)));
    } else if let Some(query) = state.parsed_query() {
        let rc = noise_core::reverse_complement(&query);
        ui.label(
            egui::RichText::new(format!("query {query}  ·  revcomp {rc}  ·  {} bases", query.len()))
                .small()
                .color(MUTED),
        );
    } else if !state.query.trim().is_empty() {
        ui.label(
            egui::RichText::new("Waiting for a valid A/C/G/T sequence…")
                .small()
                .color(MUTED),
        );
    } else {
        ui.label(
            egui::RichText::new(
                "Type a sequence and the per-sample counts update as you type; Run writes sequencecount.csv.",
            )
            .small()
            .color(MUTED),
        );
    }

    ui.add_space(8.0);
    ui.label(egui::RichText::new("Search options").strong());
    ui.horizontal(|ui| {
        let mut reverse = state.search_options.reverse_complement;
        if ui
            .checkbox(&mut reverse, "Also search reverse complement")
            .on_hover_text("Adds the revcomp and combined columns to the output")
            .changed()
        {
            state.search_options.reverse_complement = reverse;
            state.clear_live();
        }

        let mut tolerant = state.search_options.max_mismatches > 0;
        if ui
            .checkbox(&mut tolerant, "Allow mismatches")
            .on_hover_text("Sliding-window Hamming distance; never changes the exact count")
            .changed()
        {
            state.search_options.max_mismatches = if tolerant { 1 } else { 0 };
            state.clear_live();
        }

        if state.search_options.max_mismatches > 0 {
            let mut mismatches = state.search_options.max_mismatches as u32;
            if ui
                .add(egui::Slider::new(&mut mismatches, 1..=10).text("max mismatches"))
                .changed()
            {
                state.search_options.max_mismatches = mismatches as usize;
                state.clear_live();
            }
        }
    });

    if state.search_options.reverse_complement {
        ui.label(
            egui::RichText::new(
                "Reverse complement is on: the count of a read matching either strand is counted once in `combined`.",
            )
            .small()
            .color(MUTED),
        );
    }

    ui.add_space(8.0);
    egui::Grid::new("search_options_summary")
        .num_columns(2)
        .spacing([16.0, 4.0])
        .show(ui, |ui| {
            for (label, value) in [
                ("Exact count", state.live.as_ref().map(|r| r.total).unwrap_or(0)),
                ("Combined count", state.live.as_ref().map(|r| r.combined_total).unwrap_or(0)),
                (
                    "Within tolerance",
                    state.live.as_ref().map(|r| r.within_tolerance_total).unwrap_or(0),
                ),
            ] {
                ui.label(egui::RichText::new(label).small().color(MUTED));
                ui.label(egui::RichText::new(value.to_string()).monospace());
                ui.end_row();
            }
        });

    ui.add_space(10.0);
    ui.separator();
    ui.horizontal(|ui| {
        let busy = state.job.is_running();
        let enabled = !busy && state.parsed_query().is_some();
        let label = if busy { "Running…" } else { "Run search" };
        ui.add_enabled_ui(enabled, |ui| {
            if ui
                .add(egui::Button::new(
                    egui::RichText::new(label).strong().color(if busy { MUTED } else { ACCENT }),
                ))
                .clicked()
            {
                actions.run = Some(RunKind::Search);
            }
        });
        if state.parsed_query().is_none() {
            ui.label(
                egui::RichText::new("A valid sequence is needed to run.")
                    .small()
                    .color(MUTED),
            );
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(format!(
                    "results → {}",
                    state.results_dir().join("sequencecount.csv").display()
                ))
                .small()
                .color(MUTED),
            );
        });
    });
}
