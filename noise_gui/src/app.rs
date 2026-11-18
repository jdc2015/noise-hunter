//! The window shell: panels, tabs, keyboard shortcuts, and the only place a job
//! or a dialog is ever started.

use eframe::CreationContext;

use crate::jobs::JobRunner;
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

    /// Keeps the live per-sample counts moving toward what is currently typed,
    /// selected and set. The rules about whether a scan may start at all — never
    /// overlapping one already in flight, never for a mismatch-tolerant search,
    /// never while a run is going — live in `AppState::claim_live_scan`.
    fn keep_live_up_to_date(&mut self) {
        if self.state.parsed_query().is_none() {
            if self.state.live.is_some() || self.state.live_pending.is_some() {
                self.state.reset_live();
            }
            return;
        }
        let Some((id, (_folder, files, sequence, options))) = self.state.claim_live_scan() else {
            return;
        };
        let results_dir = self.state.results_dir();
        self.state.job_note = Some(format!("Counting {sequence}…"));
        self.jobs
            .start_live_counts(id, files, sequence, options, results_dir);
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
            self.state.reset_live();
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
        self.state.apply_events(self.jobs.drain());
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
        // a job in flight must keep asking: a run to animate its progress bar, a
        // live scan because its result would otherwise sit in the channel until
        // unrelated input happened to arrive.
        if self.state.job.is_running()
            || self.state.pending.is_some()
            || self.state.live_pending.is_some()
        {
            ctx.request_repaint();
        }
    }
}
