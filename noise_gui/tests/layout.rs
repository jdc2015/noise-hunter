//! Layout guard for the Results tab.
//!
//! Three charts plus a table used to overflow the central panel: the charts are
//! fixed-height plots, so on a normal window the table was laid out past the
//! bottom edge and clipped away. This runs the real tab layout headlessly and
//! fails if its content spills out of the panel it was given.

use std::path::PathBuf;

use noise_core::{CountResult, RunTotals, SampleHit, SearchOptions, SequenceQueryResult};
use noise_gui::state::{Actions, AppState, ResultsView, Tab};
use noise_gui::ui;

/// The end state of using both jobs: a count run and a live query, so every
/// chart is drawn.
fn state_with_every_result() -> AppState {
    let mut state = AppState::new();
    state.active_tab = Tab::Results;
    state.results_view = ResultsView::Counts;
    state.search_options = SearchOptions {
        reverse_complement: true,
        max_mismatches: 0,
    };

    state.counts = (1..=12)
        .map(|index| CountResult {
            read_path: PathBuf::from(format!("/samples/sample{index}_unbinned_reads")),
            output_path: PathBuf::from(format!("/noise_results/noisecounts_sample{index}.csv")),
            sample_name: format!("sample{index}"),
            lines_read: 1_200 + index,
            sequences_used: 1_180,
            lines_skipped: 20,
            unique_sequences: 340,
            preview_lines: Vec::new(),
        })
        .collect();
    state.totals = Some(RunTotals {
        file_count: 12,
        total_lines: 14_412,
        total_used: 14_160,
        total_skipped: 240,
        total_unique: 4_080,
    });

    state.live = Some(SequenceQueryResult {
        sequence: "CACCAGACACGTCTGAACTCCAGTCAC".to_string(),
        options: state.search_options,
        hits: (1..=12)
            .map(|index| SampleHit {
                sample_name: format!("sample{index}"),
                count: 100 + index,
                closest: None,
                reverse: 0,
                combined: 100 + index,
                within_tolerance: 0,
                best_distance: None,
            })
            .collect(),
        total: 1_266,
        reverse_total: 0,
        combined_total: 1_266,
        within_tolerance_total: 0,
        output_path: PathBuf::from("/noise_results/sequencecount.csv"),
    });
    state
}

/// Runs the tab the way the window does — same panels, same order — and returns
/// how far the content reached past the bottom of the central panel.
fn overflow_px(width: f32, height: f32) -> f32 {
    let ctx = egui::Context::default();
    let mut state = state_with_every_result();
    let mut actions = Actions::default();
    let mut input = egui::RawInput::default();
    input.screen_rect = Some(egui::Rect::from_min_size(
        egui::Pos2::ZERO,
        egui::vec2(width, height),
    ));

    let mut overflow = f32::NEG_INFINITY;
    let mut output = ctx.run_ui(input, |ui| {
        egui::Panel::top("status").show(ui, |ui| ui::status_bar(ui, &mut state));
        egui::Panel::top("tabs").show(ui, |ui| ui::tab_bar(ui, &mut state));
        egui::Panel::bottom("footer").show(ui, |ui| ui::footer(ui, &mut state));
        egui::CentralPanel::default().show(ui, |ui| {
            let bottom = ui.max_rect().bottom();
            ui::results::results_tab(ui, &mut state, &mut actions);
            overflow = ui.min_rect().bottom() - bottom;
        });
    });
    // Nothing here draws: there is no renderer to hand the font atlas to.
    output.textures_delta.clear();
    overflow
}

#[test]
fn the_results_tab_fits_the_window_with_every_chart_open() {
    // The default window, and the smallest one the app allows.
    for (width, height) in [(1220.0, 780.0), (880.0, 520.0)] {
        let overflow = overflow_px(width, height);
        assert!(
            overflow <= 1.0,
            "Results tab overflows a {width}x{height} window by {overflow:.0} px — \
             the table would be clipped"
        );
    }
}
