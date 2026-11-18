//! "Export CSV…" writes a table the user is looking at.
//!
//! The sequence tables reuse the core writer with a different `output_path`, so
//! an exported file is byte-identical to the one a run produces. Nothing here
//! ever writes into `noise_results/` automatically.

use std::fs::File;
use std::io::Write;
use std::path::Path;

use noise_core::{CountResult, RunTotals, SequenceQueryResult};

use crate::state::SequenceRow;

/// Writes the count table: one row per sample, plus the totals row.
pub fn write_counts_table(
    path: &Path,
    order: &[usize],
    results: &[CountResult],
    totals: Option<&RunTotals>,
) -> Result<(), String> {
    let mut file = create(path)?;
    writeln!(
        file,
        "sample_name,read_count,used_binned_reads,skipped_reads,unique_binned_sequences"
    )
    .map_err(|e| format!("Could not write '{}': {e}", path.display()))?;
    for &index in order {
        let result = &results[index];
        writeln!(
            file,
            "{},{},{},{},{}",
            result.sample_name,
            result.lines_read,
            result.sequences_used,
            result.lines_skipped,
            result.unique_sequences
        )
        .map_err(|e| format!("Could not write '{}': {e}", path.display()))?;
    }
    if let Some(totals) = totals {
        writeln!(
            file,
            "total,{},{},{},{}",
            totals.total_lines,
            totals.total_used,
            totals.total_skipped,
            totals.total_unique
        )
        .map_err(|e| format!("Could not write '{}': {e}", path.display()))?;
    }
    Ok(())
}

/// Writes a sequence table through the core writer, so the columns match a real
/// run exactly, extended columns and all.
pub fn write_sequence_table(path: &Path, result: &SequenceQueryResult) -> Result<(), String> {
    let mut exported = result.clone();
    exported.output_path = path.to_path_buf();
    noise_core::write_sequence_result(&exported)
}

/// Writes the sequence browser's rows, keeping the `sequence,counts,length`
/// header a sample's own CSV uses.
pub fn write_sequences_table(
    path: &Path,
    order: &[usize],
    rows: &[SequenceRow],
) -> Result<(), String> {
    let mut file = create(path)?;
    writeln!(file, "sequence,counts,length")
        .map_err(|e| format!("Could not write '{}': {e}", path.display()))?;
    for &index in order {
        let row = &rows[index];
        writeln!(file, "{},{},{}", row.sequence, row.counts, row.length)
            .map_err(|e| format!("Could not write '{}': {e}", path.display()))?;
    }
    Ok(())
}

fn create(path: &Path) -> Result<File, String> {
    File::create(path).map_err(|e| format!("Could not create '{}': {e}", path.display()))
}
