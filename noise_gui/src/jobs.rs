//! Runs work off the UI thread.
//!
//! Each job runs on a worker thread; per-file work inside a job uses rayon, so
//! four files are genuinely read at once (verified by timing in
//! `noise_core/examples/baseline.rs`). The worker reports back over a channel,
//! which is what keeps the window responsive mid-run.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::{Duration, Instant};

use noise_core::{
    CountResult, RunTotals, SampleHit, SearchOptions, SequenceQueryResult, aggregate_hits,
    compute_run_totals, count_file, sample_hit_from_scan, scan_sequence_in_file_with,
    write_sequence_result,
};

use crate::state::{PendingRun, SequenceRow, SEQUENCE_ROW_LIMIT};

/// The live per-sample counts for the current query, already aggregated.
pub struct LiveCounts {
    pub result: SequenceQueryResult,
}

pub enum JobEvent {
    /// Per-file progress, for the status bar.
    Progress { done: usize, total: usize },
    LiveCounts(LiveCounts),
    Counts {
        results: Vec<CountResult>,
        totals: RunTotals,
        elapsed: Duration,
    },
    SearchFinished {
        result: SequenceQueryResult,
        written: Vec<PathBuf>,
        elapsed: Duration,
    },
    SequenceRows {
        source: PathBuf,
        rows: Vec<SequenceRow>,
        truncated: bool,
    },
    Error(String),
}

pub struct JobRunner {
    sender: Sender<JobEvent>,
    receiver: Receiver<JobEvent>,
}

impl Default for JobRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl JobRunner {
    pub fn new() -> Self {
        let (sender, receiver) = channel();
        Self { sender, receiver }
    }

    /// Non-blocking harvest of everything the worker has reported.
    pub fn drain(&mut self) -> Vec<JobEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.receiver.try_recv() {
            events.push(event);
        }
        events
    }

    /// Only one job at a time: two running jobs would interleave progress and
    /// fight over the same output files.
    pub fn start(&self, run: PendingRun) {
        let sender = self.sender.clone();
        match run {
            PendingRun::Count {
                files,
                results_dir,
                filter,
            } => {
                thread::spawn(move || run_count(sender, files, results_dir, filter));
            }
            PendingRun::Search {
                files,
                sequence,
                options,
                results_dir,
            } => {
                thread::spawn(move || {
                    let started = Instant::now();
                    let result = scan_all(&sender, &files, &sequence, &options, &results_dir);
                    if let Some(result) = &result {
                        match write_sequence_result(result) {
                            Ok(()) => {
                                let _ = sender.send(JobEvent::SearchFinished {
                                    written: vec![result.output_path.clone()],
                                    elapsed: started.elapsed(),
                                    result: result.clone(),
                                });
                            }
                            Err(message) => {
                                let _ = sender.send(JobEvent::Error(message));
                            }
                        }
                    }
                });
            }
        }
    }

    /// Recomputes the live per-sample counts for the current query, writing
    /// nothing: the Results tab's charts read these from state instead.
    pub fn start_live_counts(
        &self,
        files: Vec<PathBuf>,
        sequence: String,
        options: SearchOptions,
        results_dir: PathBuf,
    ) {
        let sender = self.sender.clone();
        thread::spawn(move || {
            if let Some(result) = scan_all(&sender, &files, &sequence, &options, &results_dir) {
                let _ = sender.send(JobEvent::LiveCounts(LiveCounts { result }));
            }
        });
    }

    /// Loads one sample's `*_counts.csv` into the sequence browser.
    pub fn start_sequence_rows(&self, counts_file: PathBuf) {
        let sender = self.sender.clone();
        thread::spawn(move || match read_counts_csv(&counts_file, SEQUENCE_ROW_LIMIT) {
            Ok((rows, truncated)) => {
                let _ = sender.send(JobEvent::SequenceRows {
                    source: counts_file,
                    rows,
                    truncated,
                });
            }
            Err(message) => {
                let _ = sender.send(JobEvent::Error(message));
            }
        });
    }
}

/// Scans `files` in parallel and aggregates them in browse order. This is the
/// same code path [`noise_core::count_sequence_across_with`] takes, split so
/// the scanning can happen on several threads.
fn scan_all(
    sender: &Sender<JobEvent>,
    files: &[PathBuf],
    sequence: &str,
    options: &SearchOptions,
    results_dir: &Path,
) -> Option<SequenceQueryResult> {
    let hits = scan_files(sender, files, sequence, options)?;
    Some(aggregate_hits(results_dir, sequence, hits, *options))
}

fn scan_files(
    sender: &Sender<JobEvent>,
    files: &[PathBuf],
    sequence: &str,
    options: &SearchOptions,
) -> Option<Vec<SampleHit>> {
    let total = files.len();
    let done = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    // Indexed slots keep the results in browse order however rayon runs them.
    let slots: Vec<Mutex<Option<SampleHit>>> =
        (0..total).map(|_| Mutex::new(None)).collect();
    let slots = std::sync::Arc::new(slots);
    let errors: std::sync::Arc<Mutex<Vec<String>>> = std::sync::Arc::new(Mutex::new(Vec::new()));

    rayon::scope_fifo(|scope| {
        for (index, path) in files.iter().enumerate() {
            let path = path.clone();
            let sequence = sequence.to_string();
            let options = *options;
            let done = done.clone();
            let sender = sender.clone();
            let slots = slots.clone();
            let errors = errors.clone();
            scope.spawn_fifo(move |_| {
                match scan_sequence_in_file_with(&path, &sequence, &options) {
                    Ok(scan) => {
                        *slots[index].lock().unwrap() = Some(sample_hit_from_scan(&path, scan));
                    }
                    Err(message) => {
                        let name = path
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("<file>");
                        errors.lock().unwrap().push(format!("{name}: {message}"));
                    }
                }
                let seen = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                let _ = sender.send(JobEvent::Progress {
                    done: seen,
                    total,
                });
            });
        }
    });

    for message in errors.lock().unwrap().iter() {
        let _ = sender.send(JobEvent::Error(message.clone()));
    }
    let hits: Vec<SampleHit> = slots
        .iter()
        .filter_map(|slot| slot.lock().unwrap().take())
        .collect();
    if hits.is_empty() {
        let _ = sender.send(JobEvent::Error("No sample files could be scanned.".into()));
        return None;
    }
    Some(hits)
}

fn run_count(
    sender: Sender<JobEvent>,
    files: Vec<PathBuf>,
    results_dir: PathBuf,
    filter: Option<usize>,
) {
    let started = Instant::now();
    let total = files.len();
    let done = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let slots: Vec<Mutex<Option<CountResult>>> =
        (0..total).map(|_| Mutex::new(None)).collect();
    let slots = std::sync::Arc::new(slots);
    let errors: std::sync::Arc<Mutex<Vec<String>>> = std::sync::Arc::new(Mutex::new(Vec::new()));

    rayon::scope_fifo(|scope| {
        for (index, path) in files.iter().enumerate() {
            let path = path.clone();
            let results_dir = results_dir.clone();
            let done = done.clone();
            let sender = sender.clone();
            let slots = slots.clone();
            let errors = errors.clone();
            scope.spawn_fifo(move |_| {
                match count_file(&path, &results_dir, filter) {
                    Ok(result) => {
                        *slots[index].lock().unwrap() = Some(result);
                    }
                    Err(message) => {
                        let name = path
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("<file>");
                        errors.lock().unwrap().push(format!("{name}: {message}"));
                    }
                }
                let seen = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                let _ = sender.send(JobEvent::Progress {
                    done: seen,
                    total,
                });
            });
        }
    });

    for message in errors.lock().unwrap().iter() {
        let _ = sender.send(JobEvent::Error(message.clone()));
    }
    let results: Vec<CountResult> = slots
        .iter()
        .filter_map(|slot| slot.lock().unwrap().take())
        .collect();
    let totals = compute_run_totals(&results);
    let _ = sender.send(JobEvent::Counts {
        totals,
        results,
        elapsed: started.elapsed(),
    });
}

/// Reads a sample's `*_counts.csv` (header `sequence,counts,length`), stopping
/// at `limit` rows so a sample with 200k distinct sequences can't blow up the UI.
fn read_counts_csv(path: &Path, limit: usize) -> Result<(Vec<SequenceRow>, bool), String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("Could not read '{}': {e}", path.display()))?;
    let mut rows = Vec::new();
    let mut truncated = false;
    for (index, line) in text.lines().enumerate() {
        if index == 0 {
            continue;
        }
        if rows.len() >= limit {
            truncated = true;
            break;
        }
        let mut fields = line.split(',');
        let Some(sequence) = fields.next() else {
            continue;
        };
        let counts: u64 = fields.next().and_then(|v| v.parse().ok()).unwrap_or(0);
        let length: usize = fields
            .next()
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| sequence.len());
        rows.push(SequenceRow {
            sequence: sequence.to_string(),
            counts,
            length,
        });
    }
    Ok((rows, truncated))
}
