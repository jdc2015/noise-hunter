//! Tab 3: the tables. Sorting is per column, the totals row stays last, and
//! every number shown here is the number a CSV run wrote.

use egui::Ui;

use super::{MUTED, number};
use crate::state::{Actions, AppState, CountSort, ExportKind, PreviewSort, ResultsView, SequenceSort};

const ROW: f32 = 20.0;
const HEADER: f32 = 22.0;

pub fn results_tab(ui: &mut Ui, state: &mut AppState, actions: &mut Actions) {
    if state.counts.is_empty() && state.live.is_none() {
        ui.centered_and_justified(|ui| {
            ui.label(
                egui::RichText::new(
                    "Nothing counted yet.\nTab 1 counts duplicate sequences per sample; tab 2 searches one sequence.",
                )
                .color(MUTED),
            );
        });
        return;
    }

    let mut view = state.results_view;
    ui.horizontal(|ui| {
        for (label, candidate, available) in [
            ("Counts", ResultsView::Counts, !state.counts.is_empty()),
            (
                "Counts for query",
                ResultsView::Live,
                state.live.is_some(),
            ),
            (
                "Sequences",
                ResultsView::Sequences,
                !state.sequence_rows.is_empty(),
            ),
        ] {
            if available {
                view = view_button(ui, view, label, candidate);
            }
        }
        state.results_view = view;

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Export CSV…").clicked() {
                actions.export = Some(match view {
                    ResultsView::Counts => ExportKind::Counts,
                    ResultsView::Live => ExportKind::Live,
                    ResultsView::Sequences => ExportKind::Sequences,
                });
            }
        });
    });
    ui.add_space(4.0);
    super::charts::charts_section(ui, state);

    match view {
        ResultsView::Counts if !state.counts.is_empty() => counts_table(ui, state, actions),
        ResultsView::Live if state.live.is_some() => live_table(ui, state),
        ResultsView::Sequences if !state.sequence_rows.is_empty() => sequence_table(ui, state),
        // A view can still be selected when its data is gone (a new folder
        // clears the old results), so fall back to whatever is there.
        _ if state.live.is_some() => live_table(ui, state),
        _ => counts_table(ui, state, actions),
    }
}

fn view_button(ui: &mut Ui, current: ResultsView, label: &str, view: ResultsView) -> ResultsView {
    let selected = current == view;
    let response = ui.add(
        egui::Button::new(
            egui::RichText::new(label)
                .strong()
                .color(if selected { super::ACCENT } else { MUTED }),
        )
        .selected(selected),
    );
    if response.clicked() {
        view
    } else {
        current
    }
}

fn counts_table(ui: &mut Ui, state: &mut AppState, actions: &mut Actions) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("{} samples", state.counts.len()))
                .small()
                .color(MUTED),
        );
        if let Some(totals) = &state.totals {
            ui.label(
                egui::RichText::new(format!(
                    "read {} · used {} · skipped {} · unique {}",
                    totals.total_lines,
                    totals.total_used,
                    totals.total_skipped,
                    totals.total_unique
                ))
                .small()
                .color(MUTED),
            );
        }
    });

    let sort = state.count_sort;
    let order = state.sorted_counts();
    // Header clicks are collected and applied after the table is built: the body
    // borrows the results, so nothing inside the table may mutate state.
    let clicked = {
        let mut clicked: Option<CountSort> = None;
        let counts = &state.counts;
        let open = &mut actions.open_sequence_file;
        egui_extras::TableBuilder::new(ui)
            .striped(true)
            .column(egui_extras::Column::remainder().at_least(140.0))
            .column(egui_extras::Column::exact(90.0))
            .column(egui_extras::Column::exact(70.0))
            .column(egui_extras::Column::exact(70.0))
            .column(egui_extras::Column::exact(70.0))
            .column(egui_extras::Column::exact(90.0))
            .header(HEADER, |mut header| {
                let mut column = |ui: &mut Ui, label: &str, key: CountSort| {
                    if super::header_cell(ui, label, sort.column == key, sort.descending).clicked()
                    {
                        clicked = Some(key);
                    }
                };
                header.col(|ui| column(ui, "Sample", CountSort::Sample));
                header.col(|ui| column(ui, "Reads", CountSort::Read));
                header.col(|ui| column(ui, "Used", CountSort::Used));
                header.col(|ui| column(ui, "Skipped", CountSort::Skipped));
                header.col(|ui| column(ui, "Unique", CountSort::Unique));
                header.col(|ui| {
                    ui.label(egui::RichText::new("Sequences").strong().color(MUTED));
                });
            })
            .body(|mut body| {
                for &index in &order {
                    let result = &counts[index];
                    body.row(ROW, |mut row| {
                        row.col(|ui| {
                            ui.label(egui::RichText::new(&result.sample_name).strong());
                        });
                        row.col(|ui| number(ui, result.lines_read));
                        row.col(|ui| number(ui, result.sequences_used));
                        row.col(|ui| number(ui, result.lines_skipped));
                        row.col(|ui| number(ui, result.unique_sequences));
                        row.col(|ui| {
                            if ui
                                .button("open")
                                .on_hover_text("Browse this sample's sequences")
                                .clicked()
                            {
                                *open = Some(result.output_path.clone());
                            }
                        });
                    });
                }
            });
        clicked
    };
    if let Some(key) = clicked {
        state.count_sort.toggle(key);
    }
}

fn live_table(ui: &mut Ui, state: &mut AppState) {
    let sort = state.preview_sort;
    // The columns describe the numbers on screen, not the options the form is
    // set to: with mismatches on the live counts are paused, so what is showing
    // may be an earlier exact-only scan or an explicit tolerant run.
    let shown = state.live.as_ref().map(|result| result.options);
    let tolerant = shown.is_some_and(|options| options.max_mismatches > 0);
    let reverse = shown.is_some_and(|options| options.reverse_complement);

    ui.horizontal(|ui| {
        if let Some(result) = &state.live {
            let mut summary = format!(
                "{} samples · total {}",
                result.hits.len(),
                result.total
            );
            if reverse {
                summary.push_str(&format!(" · combined {}", result.combined_total));
            }
            if tolerant {
                summary.push_str(&format!(" · within tolerance {}", result.within_tolerance_total));
            }
            ui.label(egui::RichText::new(summary).small().color(MUTED));
        }
    });

    let order = state.sorted_preview();
    let clicked = {
        let mut clicked: Option<PreviewSort> = None;
        let hits = state.live.clone();
        let hits = hits.map(|result| result.hits).unwrap_or_default();
        egui_extras::TableBuilder::new(ui)
            .striped(true)
            .column(egui_extras::Column::remainder().at_least(160.0))
            .column(egui_extras::Column::exact(110.0))
            .column(egui_extras::Column::exact(110.0))
            .column(egui_extras::Column::exact(140.0))
            .header(HEADER, |mut header| {
                let mut column = |ui: &mut Ui, label: &str, key: PreviewSort| {
                    if super::header_cell(ui, label, sort.column == key, sort.descending).clicked()
                    {
                        clicked = Some(key);
                    }
                };
                header.col(|ui| column(ui, "Sample", PreviewSort::Sample));
                header.col(|ui| column(ui, "Count", PreviewSort::Count));
                header.col(|ui| column(ui, "Closest", PreviewSort::Closest));
                header.col(|ui| {
                    let label = match shown.filter(|options| options.max_mismatches > 0) {
                        Some(options) => format!("Within {}", options.max_mismatches),
                        None => "Nearest window".to_string(),
                    };
                    ui.label(egui::RichText::new(label).strong().color(MUTED));
                });
            })
            .body(|mut body| {
                for &index in &order {
                    let hit = &hits[index];
                    body.row(ROW, |mut row| {
                        row.col(|ui| {
                            ui.label(egui::RichText::new(&hit.sample_name).strong());
                        });
                        row.col(|ui| number(ui, crate::state::shown_count(hit)));
                        row.col(|ui| match &hit.closest {
                            Some(closest) => number(ui, format!("{} mism.", closest.distance)),
                            None => number(ui, "—"),
                        });
                        row.col(|ui| {
                            if tolerant {
                                number(ui, hit.within_tolerance);
                            } else if let Some(closest) = &hit.closest {
                                ui.label(
                                    egui::RichText::new(shorten_middle(&closest.window, 28))
                                        .small()
                                        .monospace()
                                        .color(MUTED),
                                )
                                .on_hover_text(format!(
                                    "closest window is {} mismatches from the query",
                                    closest.distance
                                ));
                            } else {
                                ui.label(egui::RichText::new("—").small().color(MUTED));
                            }
                        });
                    });
                }
            });
        clicked
    };
    if let Some(key) = clicked {
        state.preview_sort.toggle(key);
    }
}

fn sequence_table(ui: &mut Ui, state: &mut AppState) {
    let sort = state.sequence_sort;
    let order = state.sorted_sequences();
    let clicked = {
        let mut clicked: Option<SequenceSort> = None;
        let rows = &state.sequence_rows;
        egui_extras::TableBuilder::new(ui)
            .striped(true)
            .column(egui_extras::Column::exact(60.0))
            .column(egui_extras::Column::remainder().at_least(220.0))
            .column(egui_extras::Column::exact(90.0))
            .column(egui_extras::Column::exact(70.0))
            .header(HEADER, |mut header| {
                let mut column = |ui: &mut Ui, label: &str, key: SequenceSort| {
                    if super::header_cell(ui, label, sort.column == key, sort.descending).clicked()
                    {
                        clicked = Some(key);
                    }
                };
                header.col(|ui| {
                    ui.label(egui::RichText::new("#").strong().color(MUTED));
                });
                header.col(|ui| column(ui, "Sequence", SequenceSort::Sequence));
                header.col(|ui| column(ui, "Counts", SequenceSort::Counts));
                header.col(|ui| column(ui, "Length", SequenceSort::Length));
            })
            .body(|mut body| {
                for (position, &index) in order.iter().enumerate() {
                    let entry = &rows[index];
                    body.row(ROW, |mut row| {
                        row.col(|ui| {
                            ui.label(
                                egui::RichText::new((position + 1).to_string())
                                    .small()
                                    .color(MUTED),
                            );
                        });
                        row.col(|ui| {
                            ui.label(egui::RichText::new(&entry.sequence).monospace());
                        });
                        row.col(|ui| number(ui, entry.counts));
                        row.col(|ui| number(ui, entry.length));
                    });
                }
            });
        clicked
    };
    if let Some(key) = clicked {
        state.sequence_sort.toggle(key);
    }
}

fn shorten_middle(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let keep = max / 2;
    format!("{}…{}", &text[..keep], &text[text.len() - keep..])
}
