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
    /// Query + options `live` was computed with, to detect staleness.
    pub live_source: Option<(String, SearchOptions, usize)>,

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
                self.clear_live();
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

    /// Clears the live counts, so the next frame recomputes them.
    pub fn clear_live(&mut self) {
        self.live = None;
        self.live_source = None;
    }

    /// True when the live counts no longer match the query, options, or
    /// selection, so they need recomputing.
    pub fn live_is_stale(&self) -> bool {
        let selection = self.selected.len();
        match (&self.live_source, self.parsed_query()) {
            (None, None) => false,
            (None, Some(_)) => true,
            (Some((query, options, seen)), Some(query_now)) => {
                *query != query_now || *options != self.search_options || *seen != selection
            }
            (Some(_), None) => true,
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
