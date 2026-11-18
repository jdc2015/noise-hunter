//! All GUI state: folder, selection, filters, sort state, messages, and the
//! results of each kind of run.
//!
//! This type is data plus cheap transforms — nothing here opens a file or
//! starts a thread, which is what lets the widgets mutate it directly.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use noise_core::{
    CountResult, DirectoryListing, RunTotals, SampleHit, SearchOptions, SequenceQueryResult,
    list_directory_for_browse, output_path, parse_length_filter, parse_target_sequence,
    results_dir, sample_name_from_path,
};

use crate::jobs::{JobEvent, JobId};

/// What a live scan was computed from: the folder, the selected files in browse
/// order, the query, and the options. Comparing keys is how a result that is no
/// longer about what is on screen gets thrown away.
pub type LiveKey = (PathBuf, Vec<PathBuf>, String, SearchOptions);

/// How many rows the sequence browser loads from one sample's `*_counts.csv`.
pub const SEQUENCE_ROW_LIMIT: usize = 5000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    BrowseCount,
    Search,
    Results,
}

impl Tab {
    pub const ALL: [Tab; 3] = [Tab::BrowseCount, Tab::Search, Tab::Results];

    pub fn title(self) -> &'static str {
        match self {
            Tab::BrowseCount => "1  Browse & count",
            Tab::Search => "2  Search sequence",
            Tab::Results => "3  Results",
        }
    }
}

/// What the last job did, for the status bar.
#[derive(Clone, Debug)]
pub enum JobState {
    Idle,
    Running { done: usize, total: usize },
    Finished { elapsed: Duration, files: usize },
}

pub type Duration = std::time::Duration;

impl JobState {
    pub fn is_running(&self) -> bool {
        matches!(self, JobState::Running { .. })
    }

}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CountSort {
    Sample,
    Read,
    Used,
    Skipped,
    Unique,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PreviewSort {
    Sample,
    Count,
    Closest,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SequenceSort {
    Sequence,
    Counts,
    Length,
}

#[derive(Clone, Copy, Debug)]
pub struct SortState<C> {
    pub column: C,
    pub descending: bool,
}

impl<C: Copy + PartialEq> SortState<C> {
    pub fn new(column: C) -> Self {
        Self {
            column,
            descending: true,
        }
    }

    /// Same column, smallest first — what sample-name order wants.
    pub fn ascending(column: C) -> Self {
        Self {
            column,
            descending: false,
        }
    }

    /// Clicking the active column flips the direction; a new column starts
    /// descending, which is what a "biggest first" reader expects.
    pub fn toggle(&mut self, column: C) {
        if self.column == column {
            self.descending = !self.descending;
        } else {
            self.column = column;
            self.descending = true;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageKind {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug)]
pub struct Message {
    pub kind: MessageKind,
    pub text: String,
}

/// A row in the sequence browser: one line of a sample's `*_counts.csv`.
#[derive(Clone, Debug)]
pub struct SequenceRow {
    pub sequence: String,
    pub counts: u64,
    pub length: usize,
}

/// A validated run, kept as data so an overwrite confirmation can outlive the
/// frame that created it — egui re-runs `update` every frame and a closure
/// can't be stored across frames.
#[derive(Clone, Debug)]
pub enum PendingRun {
    Count {
        files: Vec<PathBuf>,
        results_dir: PathBuf,
        filter: Option<usize>,
    },
    Search {
        files: Vec<PathBuf>,
        sequence: String,
        options: SearchOptions,
        results_dir: PathBuf,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum ResultsView {
    #[default]
    Counts,
    Live,
    Sequences,
}

#[derive(Clone, Copy, Debug)]
pub enum RunKind {
    Count,
    Search,
}

#[derive(Clone, Copy, Debug)]
pub enum ExportKind {
    Counts,
    Live,
    Sequences,
}

/// What a tab asked the app to do this frame. Jobs and dialogs are started only
/// from `app.rs`, which keeps every entry point under the same overwrite guard.
#[derive(Default)]
pub struct Actions {
    pub run: Option<RunKind>,
    pub pick_folder: bool,
    pub export: Option<ExportKind>,
    pub open_sequence_file: Option<PathBuf>,
}

pub struct AppState {
    pub active_tab: Tab,
    pub input_folder: PathBuf,
    pub listing: Option<DirectoryListing>,
    pub selected: HashSet<PathBuf>,
    pub filter_text: String,
    pub results_override: Option<PathBuf>,

    /// Count tab.
    pub length_filter: String,
    pub filter_error: Option<String>,

    /// The one query, shared by the search tab and the live counts, exactly as
    /// the TUI shares its filter across screens.
    pub query: String,
    pub query_error: Option<String>,
    pub search_options: SearchOptions,

    pub counts: Vec<CountResult>,
    pub totals: Option<RunTotals>,
    /// Live per-sample counts for the current query and options, in browse
    /// order. Recomputed whenever any input changes; feeds the Results charts.
    pub live: Option<SequenceQueryResult>,
    /// What `live` was computed from, to detect staleness.
    pub live_source: Option<LiveKey>,
    /// The live scan in flight, if any, as its id and the inputs it was started
    /// with. While this is set no other live scan may start, which is what keeps
    /// typing a sequence from starting one scan per keystroke.
    pub live_pending: Option<(JobId, LiveKey)>,
    /// Ids are minted here, not in the runner: the decision to start a scan and
    /// the record of what it meant belong in the same place.
    next_job_id: JobId,

    pub count_sort: SortState<CountSort>,
    pub preview_sort: SortState<PreviewSort>,
    pub results_view: ResultsView,
    pub sequence_source: Option<PathBuf>,
    pub sequence_rows: Vec<SequenceRow>,
    pub sequence_truncated: bool,
    pub sequence_sort: SortState<SequenceSort>,

    pub job: JobState,
    pub job_note: Option<String>,
    pub pending: Option<PendingRun>,
    pub pending_existing: Vec<PathBuf>,
    pub messages: Vec<Message>,
}

impl AppState {
    pub fn new() -> Self {
        let folder = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self {
            active_tab: Tab::BrowseCount,
            input_folder: folder.clone(),
            listing: None,
            selected: HashSet::new(),
            filter_text: String::new(),
            results_override: None,
            length_filter: String::new(),
            filter_error: None,
            query: String::new(),
            query_error: None,
            search_options: SearchOptions::default(),
            counts: Vec::new(),
            totals: None,
            live: None,
            live_source: None,
            live_pending: None,
            next_job_id: 0,
            count_sort: SortState::ascending(CountSort::Sample),
            preview_sort: SortState::ascending(PreviewSort::Sample),
            results_view: ResultsView::Counts,
            sequence_source: None,
            sequence_rows: Vec::new(),
            sequence_truncated: false,
            sequence_sort: SortState::new(SequenceSort::Counts),
            job: JobState::Idle,
            job_note: None,
            pending: None,
            pending_existing: Vec::new(),
            messages: Vec::new(),
        }
    }

    /// Reads `folder` and makes it the working folder. `announce` keeps the
    /// start-up attempt quiet so the window doesn't open with a warning.
    pub fn open_folder(&mut self, folder: &Path, announce: bool) {
        match list_directory_for_browse(folder) {
            Ok(listing) => {
                let found = listing.read_files.len();
                self.input_folder = listing.current.clone();
                self.filter_text.clear();
                self.listing = Some(listing.clone());
                // Opening a folder of samples almost always means "these ones",
                // matching the TUI's Space-to-count-everything shortcut.
                self.selected = listing.read_files.iter().cloned().collect();
                self.reset_live();
                self.sequence_source = None;
                self.sequence_rows.clear();
                if !announce {
                    return;
                }
                if found == 0 {
                    self.warn(format!(
                        "No *_unbinned_reads files in '{}'",
                        listing.current.display()
                    ));
                } else {
                    self.info(format!(
                        "{found} sample files in '{}'; all selected",
                        listing.current.display()
                    ));
                }
            }
            Err(message) => self.error(message),
        }
    }

    pub fn visible_files(&self) -> &[PathBuf] {
        match &self.listing {
            Some(listing) => &listing.read_files,
            None => &[],
        }
    }

    pub fn selected_files(&self) -> Vec<PathBuf> {
        self.visible_files()
            .iter()
            .filter(|path| self.selected.contains(*path))
            .cloned()
            .collect()
    }

    /// The filter box accepts a file name, a sample name, or a bare string,
    /// exactly as the TUI's does.
    pub fn is_selected_by_filter(&self, path: &Path) -> bool {
        if self.filter_text.is_empty() {
            return true;
        }
        let filter = self.filter_text.to_lowercase();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("")
            .to_lowercase();
        if name.contains(&filter) {
            return true;
        }
        if !filter.contains("unbinned") {
            if let Some(sample) = name.strip_suffix("_unbinned_reads") {
                return sample.contains(&filter);
            }
        }
        false
    }

    pub fn toggle_select(&mut self, path: &Path) {
        if !self.selected.remove(path) {
            self.selected.insert(path.to_path_buf());
        }
    }

    pub fn select_all_visible(&mut self) {
        let paths: Vec<PathBuf> = self
            .visible_files()
            .iter()
            .filter(|path| self.is_selected_by_filter(path))
            .cloned()
            .collect();
        self.selected.extend(paths);
    }

    pub fn select_none(&mut self) {
        self.selected.clear();
        self.clear_live();
    }

    pub fn results_dir(&self) -> PathBuf {
        self.results_override
            .clone()
            .unwrap_or_else(|| results_dir(&self.input_folder))
    }

    pub fn parsed_length_filter(&self) -> Option<usize> {
        parse_length_filter(&self.length_filter).ok().flatten()
    }

    /// A valid query, if one was typed. Empty means "no sequence counts".
    pub fn parsed_query(&self) -> Option<String> {
        if self.query.trim().is_empty() {
            return None;
        }
        parse_target_sequence(&self.query).ok()
    }

    /// Clears the live counts, so the next claim recomputes them. The scan in
    /// flight keeps its claim: its result still lands, and the newest inputs are
    /// scanned after it, which is what makes typing a sequence one scan after
    /// another rather than one scan per keystroke.
    pub fn clear_live(&mut self) {
        self.live = None;
        self.live_source = None;
    }

    /// Clears the live counts *and* drops the scan in flight, so its result is
    /// thrown away. Used when the samples themselves change — a new folder, an
    /// explicit refresh — where counts for files that are no longer on screen
    /// must never flash into view.
    pub fn reset_live(&mut self) {
        self.clear_live();
        self.live_pending = None;
    }

    /// Mismatch searching is O(reads × query length) per file, so it never runs
    /// live: a slider drag or one typed base must not trigger a full tolerant
    /// re-scan of every selected sample. It stays available as an explicit Run,
    /// which is also where its elapsed time is shown.
    pub fn live_is_paused_for_mismatches(&self) -> bool {
        self.search_options.max_mismatches > 0
    }

    /// True when the numbers on screen no longer match what is opened, selected,
    /// typed, and set — so a live scan is needed.
    pub fn live_is_stale(&self) -> bool {
        match (&self.live_source, self.parsed_query()) {
            (None, None) => false,
            (None, Some(_)) => true,
            (Some(_), None) => true,
            (Some(key), Some(query)) => {
                key.0 != self.input_folder
                    || key.1 != self.selected_files()
                    || key.2 != query
                    || key.3 != self.search_options
            }
        }
    }

    /// Decides whether a live scan should start now, and if so mints its id and
    /// records it as in flight. Never overlaps scans: while one is in flight no
    /// other starts, even if the inputs have changed since it began.
    pub fn claim_live_scan(&mut self) -> Option<(JobId, LiveKey)> {
        if self.job.is_running()
            || self.live_pending.is_some()
            || self.live_is_paused_for_mismatches()
        {
            return None;
        }
        let sequence = self.parsed_query()?;
        let files = self.selected_files();
        if files.is_empty() {
            return None;
        }
        let key: LiveKey = (self.input_folder.clone(), files, sequence, self.search_options);
        if !self.live_is_stale() {
            return None;
        }
        let id = self.next_job_id;
        self.next_job_id += 1;
        self.live_pending = Some((id, key.clone()));
        Some((id, key))
    }

    /// Folds everything the workers have reported into the state. Kept here,
    /// rather than in the window, so the rules about which results count — and
    /// which must never touch a run in progress — are testable without one.
    pub fn apply_events(&mut self, events: Vec<JobEvent>) {
        for event in events {
            match event {
                JobEvent::Progress { done, total } => {
                    if let JobState::Running { .. } = self.job {
                        self.job = JobState::Running { done, total };
                    }
                }
                JobEvent::LiveCounts { id, result } => {
                    let Some((claimed, key)) = self.live_pending.take() else {
                        continue;
                    };
                    if claimed != id {
                        // Superseded: the samples or the query changed, so these
                        // numbers are about something no longer on screen.
                        continue;
                    }
                    self.live_source = Some(key);
                    let total = result.total;
                    self.live = Some(result);
                    if self.active_tab != Tab::Search {
                        self.info(format!("Live: {total} reads matched"));
                    }
                }
                JobEvent::Counts {
                    results,
                    totals,
                    elapsed,
                } => {
                    let files = results.len();
                    self.info(format!(
                        "Counted {files} sample{} in {:.2}s",
                        if files == 1 { "" } else { "s" },
                        elapsed.as_secs_f64()
                    ));
                    self.totals = Some(totals);
                    self.counts = results;
                    self.results_view = ResultsView::Counts;
                    self.job = JobState::Finished { elapsed, files };
                    self.active_tab = Tab::Results;
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
                        if files == 1 { "" } else { "s" },
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
                    self.info(summary);
                    self.live = Some(result);
                    for path in &written {
                        self.info(format!("Wrote {}", path.display()));
                    }
                    self.job = JobState::Finished { elapsed, files };
                    self.results_view = ResultsView::Live;
                    self.active_tab = Tab::Results;
                }
                JobEvent::SequenceRows {
                    source,
                    rows,
                    truncated,
                } => {
                    self.info(format!(
                        "Loaded {} sequences from {}",
                        rows.len(),
                        source
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or("?")
                    ));
                    self.sequence_source = Some(source);
                    self.sequence_rows = rows;
                    self.sequence_truncated = truncated;
                    self.results_view = ResultsView::Sequences;
                    self.active_tab = Tab::Results;
                }
                JobEvent::Error(message) => self.error(message),
            }
        }
    }

    /// Validates the count form and builds the run, or reports what is wrong.
    pub fn prepare_count_run(&mut self) -> Option<PendingRun> {
        if let Err(message) = parse_length_filter(&self.length_filter) {
            self.filter_error = Some(message.clone());
            self.error(message);
            return None;
        }
        self.filter_error = None;
        let files = self.selected_files();
        if files.is_empty() {
            self.warn("No sample files selected.");
            return None;
        }
        Some(PendingRun::Count {
            filter: self.parsed_length_filter(),
            files,
            results_dir: self.results_dir(),
        })
    }

    /// Validates the search form and builds the run, or reports what is wrong.
    pub fn prepare_search_run(&mut self) -> Option<PendingRun> {
        let sequence = match parse_target_sequence(&self.query) {
            Ok(sequence) => sequence,
            Err(message) => {
                self.query_error = Some(message.clone());
                self.error(message);
                return None;
            }
        };
        self.query_error = None;
        let files = self.selected_files();
        if files.is_empty() {
            self.warn("No sample files selected.");
            return None;
        }
        Some(PendingRun::Search {
            files,
            sequence,
            options: self.search_options,
            results_dir: self.results_dir(),
        })
    }

    /// Outputs a run would overwrite. The TUI has always reused fixed names, so
    /// the guard exists because that behaviour was never safe, not because the
    /// GUI changed it.
    pub fn outputs_to_overwrite(&self, run: &PendingRun) -> Vec<PathBuf> {
        let mut existing = Vec::new();
        match run {
            PendingRun::Count { files, results_dir, .. } => {
                for file in files {
                    let out = output_path(results_dir, &sample_name_from_path(file));
                    if out.exists() {
                        existing.push(out);
                    }
                }
            }
            PendingRun::Search { results_dir, .. } => {
                // `aggregate_hits` always points the combined result at this
                // fixed name, which is why the guard is needed at all.
                let out = results_dir.join("sequencecount.csv");
                if out.exists() {
                    existing.push(out);
                }
            }
        }
        existing
    }

    pub fn info(&mut self, text: impl Into<String>) {
        self.push(MessageKind::Info, text);
    }

    pub fn warn(&mut self, text: impl Into<String>) {
        self.push(MessageKind::Warning, text);
    }

    pub fn error(&mut self, text: impl Into<String>) {
        self.push(MessageKind::Error, text);
    }

    fn push(&mut self, kind: MessageKind, text: impl Into<String>) {
        let text = text.into();
        if text.is_empty() {
            return;
        }
        self.messages.retain(|message| message.text != text);
        self.messages.push(Message { kind, text });
        while self.messages.len() > 3 {
            self.messages.remove(0);
        }
    }

    /// The live counts in the order the Results tab shows them.
    pub fn sorted_preview(&self) -> Vec<usize> {
        let hits: &[SampleHit] = match &self.live {
            Some(result) => &result.hits,
            None => return Vec::new(),
        };
        let mut order: Vec<usize> = (0..hits.len()).collect();
        let descending = self.preview_sort.descending;
        order.sort_by(|a, b| {
            let left = &hits[*a];
            let right = &hits[*b];
            let ordering = match self.preview_sort.column {
                PreviewSort::Sample => left.sample_name.cmp(&right.sample_name),
                PreviewSort::Count => shown_count(left).cmp(&shown_count(right)),
                PreviewSort::Closest => left
                    .closest
                    .as_ref()
                    .map(|m| m.distance)
                    .unwrap_or(usize::MAX)
                    .cmp(&right.closest.as_ref().map(|m| m.distance).unwrap_or(usize::MAX)),
            };
            let ordering = if descending {
                ordering.reverse()
            } else {
                ordering
            };
            ordering.then_with(|| left.sample_name.cmp(&right.sample_name))
        });
        order
    }

    /// The count results in the order the table shows them.
    pub fn sorted_counts(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.counts.len()).collect();
        let descending = self.count_sort.descending;
        order.sort_by(|a, b| {
            let left = &self.counts[*a];
            let right = &self.counts[*b];
            let ordering = match self.count_sort.column {
                CountSort::Sample => left.sample_name.cmp(&right.sample_name),
                CountSort::Read => left.lines_read.cmp(&right.lines_read),
                CountSort::Used => left.sequences_used.cmp(&right.sequences_used),
                CountSort::Skipped => left.lines_skipped.cmp(&right.lines_skipped),
                CountSort::Unique => left.unique_sequences.cmp(&right.unique_sequences),
            };
            let ordering = if descending {
                ordering.reverse()
            } else {
                ordering
            };
            ordering.then_with(|| left.sample_name.cmp(&right.sample_name))
        });
        order
    }

    /// The sequence browser rows in the order the table shows them.
    pub fn sorted_sequences(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.sequence_rows.len()).collect();
        let descending = self.sequence_sort.descending;
        order.sort_by(|a, b| {
            let left = &self.sequence_rows[*a];
            let right = &self.sequence_rows[*b];
            let ordering = match self.sequence_sort.column {
                SequenceSort::Sequence => left.sequence.cmp(&right.sequence),
                SequenceSort::Counts => left.counts.cmp(&right.counts),
                SequenceSort::Length => left.length.cmp(&right.length),
            };
            let ordering = if descending {
                ordering.reverse()
            } else {
                ordering
            };
            ordering.then_with(|| left.sequence.cmp(&right.sequence))
        });
        order
    }
}

/// The count the tables and charts show for a sample: the combined figure once
/// reverse-complement searching is on, otherwise the exact count.
pub fn shown_count(hit: &SampleHit) -> u64 {
    if hit.combined > 0 {
        hit.combined
    } else {
        hit.count
    }
}
