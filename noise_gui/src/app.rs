//! The window shell: panels, tabs, keyboard shortcuts, and the only place a job
//! or a dialog is ever started.

use eframe::CreationContext;

use crate::jobs::{JobEvent, JobRunner};
use crate::state::{
    Actions, AppState, ExportKind, JobState, PendingRun, RunKind, Tab,
};
use crate::ui;

pub const APP_TITLE: &str = concat!("Noise Hunter ", env!("CARGO_PKG_VERSION"));

pub struct NoiseApp {
    state: AppState,
    jobs: JobRunner,
}

impl NoiseApp {
    pub fn new(cc: &CreationContext<'_>, folder: Option<std::path::PathBuf>) -> Self {
        // egui's default look is left alone; the only start-up work is opening
        // a folder. An explicit `--folder` reports what it found; falling back
        // to the working directory stays quiet, so the window never opens with
        // a warning about the directory it was started in.
        let _ = cc;
        let explicit = folder.is_some();
        let mut state = AppState::new();
        let start = folder.unwrap_or_else(|| state.input_folder.clone());
        state.open_folder(&start, explicit);
        Self {
            state,
            jobs: JobRunner::new(),
        }
    }

    /// Folds everything the worker thread has reported into the app state.
    fn take_events(&mut self) {
        for event in self.jobs.drain() {
            match event {
                JobEvent::Progress { done, total } => {
                    if let JobState::Running { .. } = self.state.job {
                        self.state.job = JobState::Running { done, total };
                    }
                }
                JobEvent::LiveCounts(update) => {
                    self.state.live_source =
                        Some((self.state.query.clone(), self.state.search_options, self.state.selected.len()));
                    let total = update.result.total;
                    self.state.live = Some(update.result);
                    if self.state.job.is_running() {
                        self.state.job = JobState::Idle;
                    }
                    if self.state.active_tab != Tab::Search {
                        self.state.info(format!("Live: {total} reads matched"));
                    }
                }
                JobEvent::Counts {
                    results,
                    totals,
                    elapsed,
                } => {
                    let files = results.len();
                    self.state.info(format!(
                        "Counted {files} sample{} in {:.2}s",
                        if files == 1 { "" } else { "s" },
                        elapsed.as_secs_f64()
                    ));
                    self.state.totals = Some(totals);
                    self.state.counts = results;
                    self.state.results_view = crate::state::ResultsView::Counts;
                    self.state.job = JobState::Finished { elapsed, files };
                    self.state.active_tab = Tab::Results;
                }
                JobEvent::SearchFinished {
                    result,
                    written,
                    elapsed,
                } => {
                    let files = result.hits.len();
                    let mut summary = format!(
                        "Search finished in {:.2}s: {} reads matched across {files} sample{}",
                        elapsed.as_secs_f64(),
                        result.total,
                        if files == 1 { "" } else { "s" }
                    );
                    if result.options.reverse_complement {
                        summary.push_str(&format!(" (combined {})", result.combined_total));
                    }
                    if result.options.max_mismatches > 0 {
                        summary.push_str(&format!(
                            ", {} within {} mismatches",
                            result.within_tolerance_total, result.options.max_mismatches
                        ));
                    }
                    self.state.info(summary);
                    self.state.live = Some(result);
                    for path in &written {
                        self.state.info(format!("Wrote {}", path.display()));
                    }
                    self.state.job = JobState::Finished { elapsed, files };
                    self.state.results_view = crate::state::ResultsView::Live;
                    self.state.active_tab = Tab::Results;
                }
                JobEvent::SequenceRows {
                    source,
                    rows,
                    truncated,
                } => {
                    self.state.info(format!(
                        "Loaded {} sequences from {}",
                        rows.len(),
                        source.file_name().and_then(|n| n.to_str()).unwrap_or("?")
                    ));
                    self.state.sequence_source = Some(source);
                    self.state.sequence_rows = rows;
                    self.state.sequence_truncated = truncated;
                    self.state.results_view = crate::state::ResultsView::Sequences;
                    self.state.active_tab = Tab::Results;
                }
                JobEvent::Error(message) => self.state.error(message),
            }
        }
    }

    /// Runs a validated job, asking first if it would replace existing output.
    fn launch(&mut self, run: PendingRun) {
        let existing = self.state.outputs_to_overwrite(&run);
        if existing.is_empty() {
            self.start(run);
        } else {
            self.state.pending_existing = existing;
            self.state.pending = Some(run);
        }
    }

    fn start(&mut self, run: PendingRun) {
        let summary = match &run {
            PendingRun::Count { files, .. } => {
                format!("Counting {} sample file{}", files.len(), if files.len() == 1 { "" } else { "s" })
            }
            PendingRun::Search { files, options, .. } => format!(
                "Searching {} file{}: {} strand{}, {} mismatches",
                files.len(),
                if files.len() == 1 { "" } else { "s" },
                if options.reverse_complement { "both" } else { "as-typed" },
                if options.reverse_complement { "s" } else { "" },
                options.max_mismatches
            ),
        };
        self.jobs.start(run);
        self.state.job = JobState::Running { done: 0, total: 0 };
        self.state.job_note = Some(summary);
        self.state.pending = None;
        self.state.pending_existing.clear();
    }

    /// The live per-sample counts, recomputed whenever the query, the options,
    /// the length filter, or the selection changes.
    fn keep_live_up_to_date(&mut self) {
        if self.state.job.is_running() || !self.state.live_is_stale() {
            return;
        }
        let Some(sequence) = self.state.parsed_query() else {
            self.state.live = None;
            self.state.live_source = None;
            return;
        };
        let files = self.state.selected_files();
        if files.is_empty() {
            return;
        }
        let results_dir = self.state.results_dir();
        self.state.job_note = Some(format!("Counting {sequence}…"));
        self.jobs
            .start_live_counts(files, sequence, self.state.search_options, results_dir);
    }

    fn pick_folder(&mut self) {
        let start = self.state.input_folder.clone();
        let Some(path) = rfd::FileDialog::new()
            .set_title("Choose a folder of sample files")
            .set_directory(&start)
            .pick_folder()
        else {
            return;
        };
        self.state.open_folder(&path, true);
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        // A dialog on screen or a text field with focus means keys belong to
        // that, not to the shortcut table.
        if self.state.pending.is_some() || ctx.memory(|memory| memory.focused().is_some()) {
            return;
        }
        let (r, o, one, two, three, esc) = ctx.input(|input| {
            (
                input.key_pressed(egui::Key::R),
                input.key_pressed(egui::Key::O),
                input.key_pressed(egui::Key::Num1),
                input.key_pressed(egui::Key::Num2),
                input.key_pressed(egui::Key::Num3),
                input.key_pressed(egui::Key::Escape),
            )
        });
        if r {
            self.state.clear_live();
        } else if o {
            self.pick_folder();
        } else if one {
            self.state.active_tab = Tab::BrowseCount;
        } else if two {
            self.state.active_tab = Tab::Search;
        } else if three {
            self.state.active_tab = Tab::Results;
        } else if esc {
            self.state.pending = None;
            self.state.pending_existing.clear();
            self.state.messages.clear();
        }
    }

    /// Writes a table the user is looking at, to a path they choose. These never
    /// touch the fixed names in `noise_results/`.
    fn export(&mut self, kind: ExportKind) {
        let default_name = match kind {
            ExportKind::Counts => "noisecounts_summary.csv",
            ExportKind::Live => "sequencecount_live.csv",
            ExportKind::Sequences => "sequences_export.csv",
        };
        let start = self.state.results_dir().join(default_name);
        let Some(path) = rfd::FileDialog::new()
            .set_title("Export a CSV")
            .set_file_name(default_name)
            .set_directory(&start)
            .save_file()
        else {
            return;
        };

        let result = match kind {
            ExportKind::Counts => {
                let order = self.state.sorted_counts();
                crate::export::write_counts_table(
                    &path,
                    &order,
                    &self.state.counts,
                    self.state.totals.as_ref(),
                )
            }
            ExportKind::Live => match &self.state.live {
                Some(live) => crate::export::write_sequence_table(&path, live),
                None => Err("Nothing to export.".to_string()),
            },
            ExportKind::Sequences => {
                let order = self.state.sorted_sequences();
                crate::export::write_sequences_table(&path, &order, &self.state.sequence_rows)
            }
        };
        match result {
            Ok(()) => self.state.info(format!("Wrote {}", path.display())),
            Err(message) => self.state.error(message),
        }
    }
}

impl eframe::App for NoiseApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.take_events();
        self.shortcuts(&ctx);
        self.keep_live_up_to_date();

        egui::Panel::top("status").show(ui, |ui| ui::status_bar(ui, &mut self.state));
        egui::Panel::top("tabs").show(ui, |ui| ui::tab_bar(ui, &mut self.state));
        egui::Panel::bottom("footer").show(ui, |ui| ui::footer(ui, &mut self.state));

        let mut actions = Actions::default();
        egui::CentralPanel::default().show(ui, |ui| match self.state.active_tab {
            Tab::BrowseCount => ui::browse::browse_tab(ui, &mut self.state, &mut actions),
            Tab::Search => ui::search::search_tab(ui, &mut self.state, &mut actions),
            Tab::Results => ui::results::results_tab(ui, &mut self.state, &mut actions),
        });

        if actions.pick_folder {
            self.pick_folder();
        }
        if let Some(kind) = actions.export {
            self.export(kind);
        }
        if let Some(path) = actions.open_sequence_file {
            self.jobs.start_sequence_rows(path);
        }
        if let Some(kind) = actions.run {
            let run = match kind {
                RunKind::Count => self.state.prepare_count_run(),
                RunKind::Search => self.state.prepare_search_run(),
            };
            if let Some(run) = run {
                self.launch(run);
            }
        }

        if !self.state.pending_existing.is_empty() {
            let files = std::mem::take(&mut self.state.pending_existing);
            match ui::confirm_overwrite(&ctx, &files) {
                Some(true) => {
                    if let Some(run) = self.state.pending.take() {
                        self.start(run);
                    }
                }
                Some(false) => {
                    self.state.pending = None;
                    self.state.warn("Run cancelled — nothing was overwritten.");
                }
                None => {
                    // Still unanswered: keep the run pending for the next frame.
                    self.state.pending_existing = files;
                }
            }
        }

        // The window only needs repainting when something actually changed, but
        // a job in flight must keep animating its progress bar.
        if self.state.job.is_running() || self.state.pending.is_some() {
            ctx.request_repaint();
        }
    }
}
