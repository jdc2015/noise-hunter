//! Layout guard for the tabs that hold tables.
//!
//! Fixed-height charts and long tables used to overflow the central panel: on a
//! normal window the table was laid out past the bottom edge and clipped. This
//! runs the real tab layouts headlessly and fails if their content spills out of
//! the panel they were given.

use std::path::PathBuf;

use noise_core::{
    CountResult, PatternOptions, PatternSearch, RunTotals, SampleHit, SampleIndices, SampleSheet,
    SearchOptions, SequenceQueryResult, SharedKmer,
};
use noise_gui::state::{Actions, AppState, ResultsView, Tab};
use noise_gui::ui;

/// The end state of using the counting and search jobs: every chart is drawn.
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

fn sheet() -> SampleSheet {
    SampleSheet::parse("sample1,ATATGCGC,CTGATCGT\nsample2,AGAACGAG,CGGCTAAT\n")
        .expect("test sheet")
}

/// A finished profile run over two samples, one of them empty.
fn state_with_profile() -> AppState {
    let mut state = AppState::new();
    state.active_tab = Tab::Profile;
    state.sheet = Some(sheet());
    state.sheet_path = Some(PathBuf::from("/repo/docs/sample_sheet.csv"));
    state.profiles = vec![
        noise_core::profile_text(
            "CACCAGACACGTCTGAACTCCAGTCACATATGCGCATCTGGGGGGGGGGGG\nACGTACGTACGT\n",
            "sample1",
            &SampleIndices {
                name: "sample1".to_string(),
                index1: "ATATGCGC".to_string(),
                index2: "CTGATCGT".to_string(),
            },
            "CACCAGACACGTCTGAACTCCAGTCAC",
            &sheet(),
        ),
        noise_core::profile_text(
            "",
            "sample2",
            &SampleIndices {
                name: "sample2".to_string(),
                index1: "AGAACGAG".to_string(),
                index2: "CGGCTAAT".to_string(),
            },
            "CACCAGACACGTCTGAACTCCAGTCAC",
            &sheet(),
        ),
    ];
    state
}

/// A finished pattern search, with one row selected so the drill-down is drawn.
fn state_with_patterns() -> AppState {
    let mut state = AppState::new();
    state.active_tab = Tab::Patterns;
    state.pattern_options = PatternOptions::new(12);
    state.pattern_samples = vec!["sample1".to_string(), "sample2".to_string()];
    state.patterns = Some(PatternSearch {
        patterns: (0..20)
            .map(|index| SharedKmer {
                kmer: format!("ACGTTGCAAGGC{}", index % 10),
                per_sample: vec![10 + index, 5 + index],
                expected: 0.001 * (index + 1) as f64,
            })
            .collect(),
        samples_compared: 2,
        excluded: 3,
    });
    state.selected_pattern = Some(0);
    state
}

/// Runs one tab the way the window does — same panels, same order — and returns
/// how far the content reached past the bottom of the central panel.
fn overflow_px(width: f32, height: f32, tab: Tab) -> f32 {
    let mut state = match tab {
        Tab::Results => state_with_every_result(),
        Tab::Profile => state_with_profile(),
        Tab::Patterns => state_with_patterns(),
        _ => state_with_every_result(),
    };
    state.active_tab = tab;
    let mut actions = Actions::default();
    let mut input = egui::RawInput::default();
    input.screen_rect = Some(egui::Rect::from_min_size(
        egui::Pos2::ZERO,
        egui::vec2(width, height),
    ));

    let mut overflow = f32::NEG_INFINITY;
    let mut output = ctx_run(input, |ui| {
        egui::Panel::top("status").show(ui, |ui| ui::status_bar(ui, &mut state));
        egui::Panel::top("tabs").show(ui, |ui| ui::tab_bar(ui, &mut state));
        egui::Panel::bottom("footer").show(ui, |ui| ui::footer(ui, &mut state));
        egui::CentralPanel::default().show(ui, |ui| {
            let bottom = ui.max_rect().bottom();
            match tab {
                Tab::Results => ui::results::results_tab(ui, &mut state, &mut actions),
                Tab::Profile => ui::profile::profile_tab(ui, &mut state, &mut actions),
                Tab::Patterns => ui::patterns::patterns_tab(ui, &mut state, &mut actions),
                _ => {}
            }
            overflow = ui.min_rect().bottom() - bottom;
        });
    });
    // Nothing here draws: there is no renderer to hand the font atlas to.
    output.textures_delta.clear();
    overflow
}

fn ctx_run(
    input: egui::RawInput,
    run: impl FnMut(&mut egui::Ui),
) -> egui::FullOutput {
    egui::Context::default().run_ui(input, run)
}

#[test]
fn every_table_tab_fits_the_window() {
    // The default window, and the smallest one the app allows.
    for (width, height) in [(1220.0, 780.0), (880.0, 520.0)] {
        for tab in [Tab::Results, Tab::Profile, Tab::Patterns] {
            let overflow = overflow_px(width, height, tab);
            assert!(
                overflow <= 1.0,
                "{tab:?} overflows a {width}x{height} window by {overflow:.0} px — \
                 the table would be clipped"
            );
        }
    }
}
