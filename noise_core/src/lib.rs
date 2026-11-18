//! Shared logic behind both noise-hunter front ends.
//!
//! Two jobs, no interface of their own:
//!
//! 1. **Duplicate counts** — tally identical sequences in plain-text
//!    `*_unbinned_reads` files and write one CSV per sample ([`count`]).
//! 2. **Sequence search** — count reads that contain a given sequence, per
//!    sample, into one combined CSV.
//!
//! Nothing here imports a terminal or widget toolkit, and the crate has no
//! dependencies. Both `noise_counter` (ratatui) and `noise_gui` (eframe) build
//! on it, so output is identical whichever front end you use.

pub mod count;
pub mod scan;

pub use count::{
    aggregate_hits, clean_sequence, compute_run_totals, count_file, count_sequence_across,
    count_sequence_across_with, examine_file, examine_file_with, parse_length_filter,
    parse_target_sequence, reverse_complement, sample_hit_from_scan, scan_sequence_in_file,
    scan_sequence_in_file_with, sequence_csv_header, start_counting_session, top_sequences,
    write_sequence_result, ClosestMatch, CountResult, CountingSession, ExamineEntry, ExamineResult,
    FileScan, RunTotals, SampleHit, SearchOptions, SequenceQueryResult, TOP_N,
};
pub use scan::{
    is_read_file, list_directory_for_browse, output_path, results_dir, sample_name_from_path,
    DirectoryListing,
};
