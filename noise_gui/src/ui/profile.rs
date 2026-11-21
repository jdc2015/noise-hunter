//! Tab 4: the per-sample noise profile — one row per sample, and the numbers the
//! profile job wrote to `noise_profile.csv`.

use egui::Ui;

use super::{ACCENT, MUTED, OK, WARN};
use crate::state::{Actions, AppState, ProfileSort};

const ROW: f32 = 20.0;
const HEADER: f32 = 22.0;

pub fn profile_tab(ui: &mut Ui, state: &mut AppState, actions: &mut Actions) {
    ui.horizontal(|ui| {
        if ui.button("Choose sample sheet…").clicked() {
            actions.choose_sheet = true;
        }
        match (&state.sheet, &state.sheet_path) {
            (Some(sheet), Some(path)) => {
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("sheet");
                ui.label(
                    egui::RichText::new(format!("{} samples from {name}", sheet.len()))
                        .small()
                        .color(MUTED),
                );
            }
            _ => {
                ui.label(
                    egui::RichText::new("no sample sheet loaded — the profile needs the indices")
                        .small()
                        .color(WARN),
                );
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let busy = state.job.is_running();
            let enabled = state.sheet.is_some() && !busy;
            ui.add_enabled_ui(enabled, |ui| {
                let label = if busy { "Profiling…" } else { "Profile selected" };
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
                    actions.run_profile = true;
                }
            });
        });
    });
    ui.add_space(4.0);

    if state.profiles.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(
                egui::RichText::new(
                    "No profile yet.\n\
                     Choose the sample sheet, then Profile selected: each sample's reads are\n\
                     counted by class — barcode, index, padding — and written to noise_results/.",
                )
                .color(MUTED),
            );
        });
        return;
    }

    let reads: u64 = state.profiles.iter().map(|profile| profile.reads).sum();
    let index_positive: u64 = state.profiles.iter().map(|profile| profile.index1).sum();
    let poly_g: u64 = state.profiles.iter().map(|profile| profile.poly_g).sum();
    let share = |part: u64| {
        if reads == 0 {
            0.0
        } else {
            part as f64 * 100.0 / reads as f64
        }
    };
    ui.label(
        egui::RichText::new(format!(
            "{} samples · {reads} reads · {:.0}% carry index1 · {:.0}% end in G padding",
            state.profiles.len(),
            share(index_positive),
            share(poly_g),
        ))
        .small()
        .color(MUTED),
    );
    ui.add_space(2.0);

    let sort = state.profile_sort;
    let order = state.sorted_profiles();
    let clicked = {
        let mut clicked: Option<ProfileSort> = None;
        let profiles = &state.profiles;
        egui_extras::TableBuilder::new(ui)
            .striped(true)
            .column(egui_extras::Column::remainder().at_least(110.0))
            .column(egui_extras::Column::exact(70.0))
            .column(egui_extras::Column::exact(70.0))
            .column(egui_extras::Column::exact(70.0))
            .column(egui_extras::Column::exact(70.0))
            .column(egui_extras::Column::exact(80.0))
            .column(egui_extras::Column::exact(80.0))
            .column(egui_extras::Column::exact(70.0))
            .column(egui_extras::Column::exact(70.0))
            .header(HEADER, |mut header| {
                let mut column = |ui: &mut Ui, label: &str, key: ProfileSort| {
                    if super::header_cell(ui, label, sort.column == key, sort.descending).clicked()
                    {
                        clicked = Some(key);
                    }
                };
                header.col(|ui| column(ui, "Sample", ProfileSort::Sample));
                header.col(|ui| column(ui, "Reads", ProfileSort::Reads));
                header.col(|ui| column(ui, "Barcode", ProfileSort::Barcode));
                header.col(|ui| column(ui, "Index1", ProfileSort::Index1));
                header.col(|ui| column(ui, "In slot", ProfileSort::Index1Slot));
                header.col(|ui| column(ui, "Other", ProfileSort::OtherIndex));
                header.col(|ui| column(ui, "Poly-G", ProfileSort::PolyG));
                header.col(|ui| column(ui, "Median", ProfileSort::Median));
                header.col(|ui| {
                    ui.label(egui::RichText::new("Empty").strong().color(MUTED));
                });
            })
            .body(|mut body| {
                for &index in &order {
                    let profile = &profiles[index];
                    body.row(ROW, |mut row| {
                        row.col(|ui| {
                            ui.label(egui::RichText::new(&profile.sample_name).strong());
                        });
                        row.col(|ui| super::number(ui, profile.reads));
                        row.col(|ui| super::number(ui, profile.barcode));
                        row.col(|ui| super::number(ui, profile.index1));
                        row.col(|ui| super::number(ui, profile.index1_slot));
                        row.col(|ui| super::number(ui, profile.other_index1));
                        row.col(|ui| super::number(ui, profile.poly_g));
                        row.col(|ui| super::number(ui, profile.median_effective_len()));
                        row.col(|ui| {
                            if profile.reads == 0 {
                                ui.label(egui::RichText::new("no reads").small().color(OK));
                            }
                        });
                    });
                }
            });
        clicked
    };
    if let Some(key) = clicked {
        state.profile_sort.toggle(key);
    }
}
