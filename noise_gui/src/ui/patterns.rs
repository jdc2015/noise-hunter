//! Tab 5: k-mers shared by every selected sample — the cross-sample view the
//! whole refinement exists for.

use egui::Ui;

use super::{ACCENT, MUTED, OK, WARN};
use crate::state::{Actions, AppState, PatternSort, PROFILE_BARCODE};

const ROW: f32 = 20.0;
const HEADER: f32 = 22.0;

pub fn patterns_tab(ui: &mut Ui, state: &mut AppState, actions: &mut Actions) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("k").strong());
        ui.add(
            egui::DragValue::new(&mut state.pattern_options.k)
                .range(4..=60)
                .speed(1.0),
        )
        .on_hover_text("Window length. Arbitrary: a shared k-mer may hold part of the barcode, an index, or nothing recognisable.");

        let mut exclude_barcode = state.pattern_options.exclude_barcode.is_some();
        if ui
            .checkbox(&mut exclude_barcode, "exclude barcode")
            .on_hover_text("Drop shared k-mers that are part of the 27-base barcode")
            .changed()
        {
            state.pattern_options.exclude_barcode =
                exclude_barcode.then(|| PROFILE_BARCODE.to_string());
        }

        let mut exclude_g = state.pattern_options.exclude_g_rich.is_some();
        if ui
            .checkbox(&mut exclude_g, "exclude G-rich")
            .on_hover_text("Drop shared k-mers that are mostly G — the padding boundary")
            .changed()
        {
            state.pattern_options.exclude_g_rich = exclude_g.then_some(8);
        }
        if let Some(minimum) = state.pattern_options.exclude_g_rich.as_mut() {
            ui.add(egui::DragValue::new(minimum).range(1..=60))
                .on_hover_text("Minimum number of G bases to exclude");
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let busy = state.job.is_running();
            ui.add_enabled_ui(!busy, |ui| {
                let label = if busy { "Comparing…" } else { "Find shared patterns" };
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new(label)
                                .strong()
                                .color(if busy { MUTED } else { ACCENT }),
                        ),
                    )
                    .clicked()
                {
                    actions.run_patterns = true;
                }
            });
        });
    });

    let Some(search) = &state.patterns else {
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(
                "No search yet.\n\
                 This compares the selected samples and keeps the k-mers they all contain.\n\
                 With the barcode and G-rich k-mers excluded, what remains is the noise.",
            )
            .color(MUTED),
        );
        return;
    };

    ui.add_space(2.0);
    let mut summary = format!(
        "{} shared k-mer{} at k={} across {} sample{}",
        search.patterns.len(),
        if search.patterns.len() == 1 { "" } else { "s" },
        state.pattern_options.k,
        search.samples_compared,
        if search.samples_compared == 1 { "" } else { "s" },
    );
    if search.excluded > 0 {
        summary.push_str(&format!(
            " · {} excluded ({})",
            search.excluded,
            state.pattern_options.describe()
        ));
    }
    ui.label(egui::RichText::new(summary).small().color(if search.patterns.is_empty() {
        WARN
    } else {
        OK
    }));

    if search.patterns.is_empty() {
        ui.label(
            egui::RichText::new(
                "Nothing is shared by every compared sample at this k with these exclusions.\n\
                 That is an answer: try a smaller k, or fewer samples.",
            )
            .small()
            .color(MUTED),
        );
        return;
    }

    let sort = state.pattern_sort;
    let order = state.sorted_patterns();
    // The per-sample drill-down sits under the table, so the table gives it room
    // when one is open — the same lesson as the charts, one tab over.
    const DRILL_DOWN_HEIGHT: f32 = 170.0;
    let table_height = if state.selected_pattern.is_some() {
        (ui.available_height() - DRILL_DOWN_HEIGHT).max(140.0)
    } else {
        ui.available_height()
    };
    let clicked = {
        let mut clicked: Option<PatternSort> = None;
        let mut selected: Option<usize> = None;
        let patterns = &search.patterns;
        egui_extras::TableBuilder::new(ui)
            .striped(true)
            .max_scroll_height(table_height)
            .column(egui_extras::Column::remainder().at_least(220.0))
            .column(egui_extras::Column::exact(80.0))
            .column(egui_extras::Column::exact(100.0))
            .column(egui_extras::Column::exact(90.0))
            .column(egui_extras::Column::exact(90.0))
            .header(HEADER, |mut header| {
                let mut column = |ui: &mut Ui, label: &str, key: PatternSort| {
                    if super::header_cell(ui, label, sort.column == key, sort.descending).clicked()
                    {
                        clicked = Some(key);
                    }
                };
                header.col(|ui| column(ui, "k-mer", PatternSort::Kmer));
                header.col(|ui| column(ui, "Present", PatternSort::Kmer));
                header.col(|ui| column(ui, "Total", PatternSort::Total));
                header.col(|ui| {
                    ui.label(egui::RichText::new("Expected").strong().color(MUTED));
                });
                header.col(|ui| column(ui, "O/E", PatternSort::ObservedOverExpected));
            })
            .body(|mut body| {
                for &index in &order {
                    let pattern = &patterns[index];
                    let is_selected = state.selected_pattern == Some(index);
                    body.row(ROW, |mut row| {
                        row.col(|ui| {
                            if ui
                                .selectable_label(
                                    is_selected,
                                    egui::RichText::new(&pattern.kmer).monospace(),
                                )
                                .clicked()
                            {
                                selected = Some(index);
                            }
                        });
                        row.col(|ui| {
                            super::number(ui, format!("{}/{}", pattern.samples_present(), search.samples_compared))
                        });
                        row.col(|ui| super::number(ui, pattern.total()));
                        row.col(|ui| super::number(ui, format!("{:.3}", pattern.expected)));
                        row.col(|ui| {
                            let ratio = pattern
                                .observed_over_expected()
                                .map(|value| format!("{value:.1}"))
                                .unwrap_or_else(|| "n/a".to_string());
                            super::number(ui, ratio);
                        });
                    });
                }
            });
        if let Some(index) = selected {
            state.selected_pattern = Some(index);
        }
        clicked
    };
    if let Some(key) = clicked {
        state.pattern_sort.toggle(key);
    }

    if let Some(index) = state.selected_pattern {
        if let Some(pattern) = state.patterns.as_ref().and_then(|search| search.patterns.get(index)) {
            ui.separator();
            ui.label(
                egui::RichText::new(format!("{} in each sample", pattern.kmer))
                    .monospace()
                    .strong(),
            );
            egui::ScrollArea::vertical()
                .id_salt("pattern_by_sample")
                .max_height(120.0)
                .show(ui, |ui| {
                    for (sample, count) in state.pattern_samples.iter().zip(&pattern.per_sample) {
                        if *count > 0 {
                            ui.label(
                                egui::RichText::new(format!("{sample:<10} {count}")).monospace(),
                            );
                        }
                    }
                });
        }
    }
}
