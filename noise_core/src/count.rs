use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use crate::scan::{output_path, results_dir, sample_name_from_path};

const PREVIEW_LINE_LIMIT: usize = 20;
pub const TOP_N: usize = 10;

#[derive(Clone)]
pub struct CountingSession {
    pub input_folder: PathBuf,
    pub read_files: Vec<PathBuf>,
    pub results_dir: PathBuf,
    pub length_filter: Option<usize>,
    pub current_index: usize,
    pub completed: Vec<CountResult>,
}

impl CountingSession {
    pub fn new(
        input_folder: PathBuf,
        read_files: Vec<PathBuf>,
        length_filter: Option<usize>,
    ) -> Self {
        Self {
            results_dir: results_dir(&input_folder),
            input_folder,
            read_files,
            length_filter,
            current_index: 0,
            completed: Vec::new(),
        }
    }

    pub fn record_result(&mut self, result: CountResult) {
        self.completed.push(result);
    }

    pub fn current_file(&self) -> Option<&Path> {
        self.read_files.get(self.current_index).map(|p| p.as_path())
    }

    pub fn has_next(&self) -> bool {
        self.current_index + 1 < self.read_files.len()
    }

    pub fn advance(&mut self) {
        self.current_index += 1;
    }

    pub fn progress_label(&self) -> String {
        format!(
            "File {} of {}",
            self.current_index + 1,
            self.read_files.len()
        )
    }

    pub fn filter_label(&self) -> String {
        match self.length_filter {
            Some(n) => format!("first {n} bases"),
            None => "full length".to_string(),
        }
    }
}

pub fn start_counting_session(
    input_folder: &Path,
    read_files: Vec<PathBuf>,
    length_filter: Option<usize>,
) -> Result<CountingSession, String> {
    Ok(CountingSession::new(
        input_folder.to_path_buf(),
        read_files,
        length_filter,
    ))
}

/// Parses the length-filter prompt text. A blank value means "no filter"
/// (count full-length sequences); otherwise it must be a positive integer.
pub fn parse_length_filter(input: &str) -> Result<Option<usize>, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let n: usize = trimmed.parse().map_err(|_| {
        format!(
            "Invalid length filter '{trimmed}' — enter a positive whole number, or leave blank"
        )
    })?;
    if n == 0 {
        return Err("Length filter must be at least 1 (leave blank for full length)".to_string());
    }
    Ok(Some(n))
}

#[derive(Clone)]
pub struct CountResult {
    pub read_path: PathBuf,
    pub output_path: PathBuf,
    pub sample_name: String,
    pub lines_read: u64,
    pub sequences_used: u64,
    pub lines_skipped: u64,
    pub unique_sequences: u64,
    pub preview_lines: Vec<String>,
}

/// One entry in the "examine first file" top-10 table.
#[derive(Clone)]
pub struct ExamineEntry {
    pub sequence: String,
    pub count: u64,
    pub length: usize,
}

#[derive(Clone)]
pub struct ExamineResult {
    pub sample_name: String,
    pub lines_read: u64,
    pub sequences_used: u64,
    pub lines_skipped: u64,
    pub unique_sequences: u64,
    pub top: Vec<ExamineEntry>,
}

#[derive(Clone, Debug)]
pub struct RunTotals {
    pub file_count: usize,
    pub total_lines: u64,
    pub total_used: u64,
    pub total_skipped: u64,
    pub total_unique: u64,
}

pub fn compute_run_totals(completed: &[CountResult]) -> RunTotals {
    RunTotals {
        file_count: completed.len(),
        total_lines: completed.iter().map(|r| r.lines_read).sum(),
        total_used: completed.iter().map(|r| r.sequences_used).sum(),
        total_skipped: completed.iter().map(|r| r.lines_skipped).sum(),
        total_unique: completed.iter().map(|r| r.unique_sequences).sum(),
    }
}

/// Cleans one raw line into a countable sequence, or `None` if it should be
/// skipped. Leading/trailing whitespace and trailing `N` bases are removed,
/// the result is uppercased, and it must contain only A/C/G/T afterwards.
pub fn clean_sequence(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let without_n = trimmed.trim_end_matches(|c| c == 'N' || c == 'n');
    let upper: String = without_n
        .chars()
        .map(|c| c.to_ascii_uppercase())
        .collect();

    if upper.is_empty() {
        return None;
    }
    if !upper.chars().all(|c| matches!(c, 'A' | 'C' | 'G' | 'T')) {
        return None;
    }
    Some(upper)
}

struct Tally {
    counts: HashMap<String, u64>,
    lines_read: u64,
    sequences_used: u64,
    lines_skipped: u64,
}

/// Reads a file, cleans each line, optionally truncates to the first
/// `filter` bases, and tallies duplicate sequences.
fn tally_file(path: &Path, filter: Option<usize>) -> Result<Tally, String> {
    let file = File::open(path).map_err(|e| format!("Could not open '{}': {e}", path.display()))?;
    let reader = BufReader::with_capacity(1024 * 1024, file);

    let mut counts: HashMap<String, u64> = HashMap::new();
    let mut lines_read = 0u64;
    let mut sequences_used = 0u64;
    let mut lines_skipped = 0u64;

    for line in reader.lines() {
        let line = line.map_err(|e| format!("Could not read '{}': {e}", path.display()))?;
        lines_read += 1;

        if let Some(mut seq) = clean_sequence(&line) {
            if let Some(n) = filter {
                if seq.len() > n {
                    seq.truncate(n);
                }
            }
            *counts.entry(seq).or_insert(0) += 1;
            sequences_used += 1;
        } else {
            lines_skipped += 1;
        }
    }

    Ok(Tally {
        counts,
        lines_read,
        sequences_used,
        lines_skipped,
    })
}

/// Sorted (count descending, then alphabetical) top-`k` sequences.
pub fn top_sequences(counts: &HashMap<String, u64>, k: usize) -> Vec<ExamineEntry> {
    let mut rows: Vec<(&String, &u64)> = counts.iter().collect();
    rows.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));

    rows.into_iter()
        .take(k)
        .map(|(sequence, count)| ExamineEntry {
            length: sequence.len(),
            sequence: sequence.clone(),
            count: *count,
        })
        .collect()
}

/// Counts the first (examine) file at full length and returns the top-10
/// sequences plus summary stats, for the "choose a filter" screen.
pub fn examine_file(path: &Path) -> Result<ExamineResult, String> {
    examine_file_with(path, None)
}

/// As [`examine_file`], but tallied through an optional length filter, so a
/// front end can show the top-10 for the filter the user is typing.
pub fn examine_file_with(path: &Path, filter: Option<usize>) -> Result<ExamineResult, String> {
    let tally = tally_file(path, filter)?;
    Ok(ExamineResult {
        top: top_sequences(&tally.counts, TOP_N),
        sample_name: sample_name_from_path(path),
        lines_read: tally.lines_read,
        sequences_used: tally.sequences_used,
        lines_skipped: tally.lines_skipped,
        unique_sequences: tally.counts.len() as u64,
    })
}

/// Counts one file (with optional length filter) and writes its CSV.
pub fn count_file(
    path: &Path,
    results_dir: &Path,
    filter: Option<usize>,
) -> Result<CountResult, String> {
    fs::create_dir_all(results_dir).map_err(|e| {
        format!(
            "Could not create results directory '{}': {e}",
            results_dir.display()
        )
    })?;

    let sample_name = sample_name_from_path(path);
    let output_path = output_path(results_dir, &sample_name);

    let tally = tally_file(path, filter)?;
    write_counts(&output_path, &tally.counts)?;
    let preview_lines = read_preview_lines(&output_path, PREVIEW_LINE_LIMIT)?;

    Ok(CountResult {
        read_path: path.to_path_buf(),
        output_path,
        sample_name,
        lines_read: tally.lines_read,
        sequences_used: tally.sequences_used,
        lines_skipped: tally.lines_skipped,
        unique_sequences: tally.counts.len() as u64,
        preview_lines,
    })
}

/// Writes `sequence,counts,length` rows ordered by count descending, then
/// alphabetically for ties.
fn write_counts(path: &Path, counts: &HashMap<String, u64>) -> Result<(), String> {
    let mut rows: Vec<(&String, &u64)> = counts.iter().collect();
    rows.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));

    let mut file =
        File::create(path).map_err(|e| format!("Could not create output '{}': {e}", path.display()))?;

    writeln!(file, "sequence,counts,length")
        .map_err(|e| format!("Could not write output '{}': {e}", path.display()))?;

    for (sequence, count) in rows {
        writeln!(file, "{sequence},{count},{}", sequence.len())
            .map_err(|e| format!("Could not write output '{}': {e}", path.display()))?;
    }

    Ok(())
}

fn read_preview_lines(path: &Path, limit: usize) -> Result<Vec<String>, String> {
    let file = File::open(path).map_err(|e| format!("Could not read output '{}': {e}", path.display()))?;
    let reader = BufReader::new(file);
    reader.lines().take(limit).collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Could not read output '{}': {e}", path.display()))
}

/// The nearest read window found for a sample with zero hits. Used purely as a
/// diagnostic: it never changes the reported count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClosestMatch {
    /// Number of differing bases between the query and `window`.
    pub distance: usize,
    /// The equal-length read window that is nearest to the query.
    pub window: String,
    /// The full cleaned read that contains `window`.
    pub read: String,
}

/// One sample's result for a specific-sequence query.
///
/// `count` is always the exact-containment count, whatever the options were,
/// so numbers never move between a default run and an extended one. The extra
/// fields stay empty for the default options.
#[derive(Clone)]
pub struct SampleHit {
    pub sample_name: String,
    pub count: u64,
    /// Populated only when `count == 0`: the closest non-matching read window.
    pub closest: Option<ClosestMatch>,
    /// Reads containing the reverse complement — only with
    /// `SearchOptions::reverse_complement`.
    pub reverse: u64,
    /// Reads containing the sequence or its reverse complement, counted once.
    pub combined: u64,
    /// Reads within `SearchOptions::max_mismatches` — only when that is > 0.
    pub within_tolerance: u64,
    /// Fewest mismatched bases seen in this sample — only when
    /// `SearchOptions::max_mismatches` is > 0. `None` when every read was
    /// shorter than the query.
    pub best_distance: Option<usize>,
}

/// Result of counting one specific sequence across one or more sample files.
#[derive(Clone)]
pub struct SequenceQueryResult {
    pub sequence: String,
    pub options: SearchOptions,
    pub hits: Vec<SampleHit>,
    pub total: u64,
    pub reverse_total: u64,
    pub combined_total: u64,
    pub within_tolerance_total: u64,
    pub output_path: PathBuf,
}

/// How a specific-sequence search behaves. `Default` is exact containment of
/// the sequence as typed — what the TUI has always done — so leaving options
/// alone reproduces v0.4.0 output byte for byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchOptions {
    /// Also search the reverse complement.
    pub reverse_complement: bool,
    /// Count reads whose nearest window differs by at most this many bases.
    /// `0` means exact containment only.
    pub max_mismatches: usize,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            reverse_complement: false,
            max_mismatches: 0,
        }
    }
}

impl SearchOptions {
    /// True when nothing beyond exact containment is asked for, which is the
    /// case that keeps the cheap fast path (no per-read window scan).
    pub fn is_exact(&self) -> bool {
        self.max_mismatches == 0 && !self.reverse_complement
    }
}

/// The complementary strand read 3'→5': A↔T, C↔G. Input is expected to be
/// A/C/G/T already (`clean_sequence` and `parse_target_sequence` enforce that).
pub fn reverse_complement(sequence: &str) -> String {
    sequence
        .chars()
        .rev()
        .map(|base| match base {
            'A' => 'T',
            'C' => 'G',
            'G' => 'C',
            'T' => 'A',
            other => other,
        })
        .collect()
}

/// What one file scan found. Each counter counts a *read* at most once, even
/// if the sequence occurs in it more than once.
#[derive(Clone, Debug, Default)]
pub struct FileScan {
    pub exact: u64,
    pub reverse: u64,
    pub combined: u64,
    pub within_tolerance: u64,
    pub best_distance: Option<usize>,
    pub closest: Option<ClosestMatch>,
}

/// Normalizes a user-entered target sequence: trims whitespace, uppercases,
/// and rejects anything that is not A/C/G/T.
pub fn parse_target_sequence(input: &str) -> Result<String, String> {
    let sequence: String = input
        .trim()
        .chars()
        .map(|c| c.to_ascii_uppercase())
        .collect();

    if sequence.is_empty() {
        return Err("Enter a sequence to count (A/C/G/T)".to_string());
    }
    if !sequence.chars().all(|c| matches!(c, 'A' | 'C' | 'G' | 'T')) {
        return Err("Sequence may only contain A, C, G, T".to_string());
    }
    Ok(sequence)
}

/// v0.4.0-compatible entry point: the exact-containment count plus the
/// zero-hit diagnostic. Equivalent to `scan_sequence_in_file_with` with
/// default options.
pub fn scan_sequence_in_file(
    path: &Path,
    sequence: &str,
) -> Result<(u64, Option<ClosestMatch>), String> {
    let scan = scan_sequence_in_file_with(path, sequence, &SearchOptions::default())?;
    Ok((scan.exact, scan.closest))
}

/// Counts reads under `options`, and when no read matches exactly also finds
/// the closest read window (minimum Hamming distance).
///
/// With default options the window scan is skipped as soon as the first hit is
/// seen, so samples with hits pay no extra cost — the cheap path the TUI takes.
/// Reverse-complement or mismatch tolerance has to examine every read, so that
/// path scans all of them; `within_tolerance` and `best_distance` are only
/// meaningful there.
pub fn scan_sequence_in_file_with(
    path: &Path,
    sequence: &str,
    options: &SearchOptions,
) -> Result<FileScan, String> {
    let file = File::open(path).map_err(|e| format!("Could not open '{}': {e}", path.display()))?;
    let reader = BufReader::with_capacity(1024 * 1024, file);

    let query = sequence.as_bytes();
    let query_len = query.len();
    let reverse_query = reverse_complement(sequence);
    let reverse_query = options
        .reverse_complement
        .then_some(reverse_query.as_bytes());

    let mut scan = FileScan::default();

    for line in reader.lines() {
        let line = line.map_err(|e| format!("Could not read '{}': {e}", path.display()))?;
        let Some(cleaned) = clean_sequence(&line) else {
            continue;
        };

        if options.is_exact() {
            // The fast path: substring containment, diagnostic only while the
            // sample has no hits. Identical work to v0.4.0.
            if cleaned.contains(sequence) {
                scan.exact += 1;
                // A hit means this sample needs no diagnostic; stop tracking it.
                scan.closest = None;
            } else if scan.exact == 0 {
                let forward = closest_window(&cleaned, query);
                record_closest(&mut scan.closest, forward, &cleaned, query_len);
            }
            continue;
        }

        let forward = closest_window(&cleaned, query);
        let reverse = reverse_query.and_then(|rc| closest_window(&cleaned, rc));
        let forward_distance = forward.map(|(distance, _)| distance);
        let reverse_distance = reverse.map(|(distance, _)| distance);

        let forward_hit = forward_distance == Some(0);
        let reverse_hit = reverse_distance == Some(0);
        if forward_hit {
            scan.exact += 1;
        }
        if reverse_hit {
            scan.reverse += 1;
        }
        if forward_hit || reverse_hit {
            scan.combined += 1;
        }

        // The nearest window in either searched orientation, for this read.
        let best = forward_distance.into_iter().chain(reverse_distance).min();
        if let Some(distance) = best {
            if scan.best_distance.map_or(true, |seen| distance < seen) {
                scan.best_distance = Some(distance);
            }
            if distance <= options.max_mismatches {
                scan.within_tolerance += 1;
            }
        }

        if scan.exact == 0 {
            record_closest(&mut scan.closest, forward, &cleaned, query_len);
        }
    }

    if options.is_exact() {
        // With nothing extra asked for, the derived columns are the exact ones.
        scan.combined = scan.exact;
        scan.within_tolerance = scan.exact;
    }

    Ok(scan)
}

/// Keeps the nearest forward window seen so far in `slot`. `forward` is the
/// already-computed `(distance, start)` for `cleaned`, so no re-scan is needed.
fn record_closest(
    slot: &mut Option<ClosestMatch>,
    forward: Option<(usize, usize)>,
    cleaned: &str,
    query_len: usize,
) {
    let Some((distance, start)) = forward else {
        return;
    };
    let better = slot
        .as_ref()
        .map_or(true, |current| distance < current.distance);
    if better {
        *slot = Some(ClosestMatch {
            distance,
            window: cleaned[start..start + query_len].to_string(),
            read: cleaned.to_string(),
        });
    }
}

/// Finds the window in `cleaned` with the fewest mismatches against `query`.
/// Returns `(distance, start_index)`, or `None` when the read is shorter than
/// the query (no equal-length window exists).
fn closest_window(cleaned: &str, query: &[u8]) -> Option<(usize, usize)> {
    let bytes = cleaned.as_bytes();
    let query_len = query.len();
    if bytes.len() < query_len {
        return None;
    }

    let mut best_distance = usize::MAX;
    let mut best_start = 0usize;

    for start in 0..=bytes.len() - query_len {
        let mut distance = 0usize;
        for (a, b) in bytes[start..start + query_len].iter().zip(query) {
            if a != b {
                distance += 1;
            }
        }
        if distance < best_distance {
            best_distance = distance;
            best_start = start;
            if distance == 0 {
                break;
            }
        }
    }

    if best_distance == usize::MAX {
        None
    } else {
        Some((best_distance, best_start))
    }
}

/// The v0.4.0 entry point: exact containment of `sequence` across `files`,
/// written to `sequencecount.csv` with header `sample,<SEQUENCE>`, one row per
/// sample in browse order, then a total row.
pub fn count_sequence_across(
    files: &[PathBuf],
    sequence: &str,
    results_dir: &Path,
) -> Result<SequenceQueryResult, String> {
    count_sequence_across_with(files, sequence, results_dir, &SearchOptions::default())
}

/// As [`count_sequence_across`], but with reverse-complement and
/// mismatch-tolerance options. With default options the file written is
/// byte-identical to a v0.4.0 run; enabling an option only adds columns.
pub fn count_sequence_across_with(
    files: &[PathBuf],
    sequence: &str,
    results_dir: &Path,
    options: &SearchOptions,
) -> Result<SequenceQueryResult, String> {
    fs::create_dir_all(results_dir).map_err(|e| {
        format!(
            "Could not create results directory '{}': {e}",
            results_dir.display()
        )
    })?;

    let mut hits = Vec::with_capacity(files.len());
    for path in files {
        let scan = scan_sequence_in_file_with(path, sequence, options)?;
        hits.push(sample_hit_from_scan(path, scan));
    }

    let result = aggregate_hits(results_dir, sequence, hits, *options);
    write_sequence_result(&result)?;
    Ok(result)
}

/// Turns one file's scan into its sample row. Split out so a caller can scan
/// files in parallel — the GUI does — and still aggregate and write through
/// the same code path as the TUI.
pub fn sample_hit_from_scan(path: &Path, scan: FileScan) -> SampleHit {
    SampleHit {
        sample_name: sample_name_from_path(path),
        count: scan.exact,
        closest: scan.closest,
        reverse: scan.reverse,
        combined: scan.combined,
        within_tolerance: scan.within_tolerance,
        best_distance: scan.best_distance,
    }
}

/// Sums `hits` (already in browse order) and points the result at the combined
/// CSV, without writing anything.
pub fn aggregate_hits(
    results_dir: &Path,
    sequence: &str,
    hits: Vec<SampleHit>,
    options: SearchOptions,
) -> SequenceQueryResult {
    SequenceQueryResult {
        sequence: sequence.to_string(),
        options,
        total: hits.iter().map(|hit| hit.count).sum(),
        reverse_total: hits.iter().map(|hit| hit.reverse).sum(),
        combined_total: hits.iter().map(|hit| hit.combined).sum(),
        within_tolerance_total: hits.iter().map(|hit| hit.within_tolerance).sum(),
        hits,
        output_path: results_dir.join("sequencecount.csv"),
    }
}

/// The combined CSV's header for a given set of options. A default search
/// keeps the v0.4.0 header exactly; each option only ever adds columns.
pub fn sequence_csv_header(sequence: &str, options: &SearchOptions) -> String {
    let mut header = format!("sample,{sequence}");
    if options.reverse_complement {
        header.push_str(&format!(",{sequence}_rc,combined"));
    }
    if options.max_mismatches > 0 {
        header.push_str(&format!(",within_{},best_mismatches", options.max_mismatches));
    }
    header
}

/// Writes the combined CSV described by [`sequence_csv_header`]: one row per
/// sample in browse order, then a `total` row. The `best_mismatches` column has
/// no total, so its field in the last row is empty.
pub fn write_sequence_result(result: &SequenceQueryResult) -> Result<(), String> {
    let path = &result.output_path;
    // Creating the folder here means a caller that scanned files itself (the
    // GUI) can call this directly without a separate setup step.
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            format!(
                "Could not create results directory '{}': {e}",
                parent.display()
            )
        })?;
    }
    let mut file =
        File::create(path).map_err(|e| format!("Could not create output '{}': {e}", path.display()))?;

    let write_line = |file: &mut File, line: String| -> Result<(), String> {
        writeln!(file, "{line}")
            .map_err(|e| format!("Could not write output '{}': {e}", path.display()))
    };

    write_line(&mut file, sequence_csv_header(&result.sequence, &result.options))?;

    for hit in &result.hits {
        let mut row = format!("{},{}", hit.sample_name, hit.count);
        if result.options.reverse_complement {
            row.push_str(&format!(",{},{}", hit.reverse, hit.combined));
        }
        if result.options.max_mismatches > 0 {
            let best = hit
                .best_distance
                .map(|distance| distance.to_string())
                .unwrap_or_default();
            row.push_str(&format!(",{},{}", hit.within_tolerance, best));
        }
        write_line(&mut file, row)?;
    }

    let mut total_row = format!("total,{}", result.total);
    if result.options.reverse_complement {
        total_row.push_str(&format!(",{},{}", result.reverse_total, result.combined_total));
    }
    if result.options.max_mismatches > 0 {
        total_row.push_str(&format!(",{},", result.within_tolerance_total));
    }
    write_line(&mut file, total_row)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "noise_counter_{tag}_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ))
    }

    #[test]
    fn trims_trailing_n() {
        assert_eq!(clean_sequence("ACGTNNNN"), Some("ACGT".to_string()));
        assert_eq!(clean_sequence(" ACGTNN  "), Some("ACGT".to_string()));
    }

    #[test]
    fn uppercases_input() {
        assert_eq!(clean_sequence("acgt"), Some("ACGT".to_string()));
        assert_eq!(clean_sequence("acgtnn"), Some("ACGT".to_string()));
    }

    #[test]
    fn skips_empty_and_all_n() {
        assert_eq!(clean_sequence(""), None);
        assert_eq!(clean_sequence("   "), None);
        assert_eq!(clean_sequence("NNNNNN"), None);
    }

    #[test]
    fn skips_internal_or_other_bases() {
        assert_eq!(clean_sequence("ACNT"), None);
        assert_eq!(clean_sequence("ACGX"), None);
    }

    #[test]
    fn parses_length_filter() {
        assert_eq!(parse_length_filter(""), Ok(None));
        assert_eq!(parse_length_filter("   "), Ok(None));
        assert_eq!(parse_length_filter("30"), Ok(Some(30)));
        assert_eq!(parse_length_filter("1"), Ok(Some(1)));
        assert!(parse_length_filter("0").is_err());
        assert!(parse_length_filter("abc").is_err());
    }

    #[test]
    fn counts_and_writes_sorted_csv() {
        let dir = temp_dir("count");
        let input = dir.join("sample1_unbinned_reads");
        let results = results_dir(&dir);

        // 4x ACGT, 2x AACC, 1x G (trailing Ns removed), 1 blank, 1 all-N.
        fs::create_dir_all(&dir).expect("create input dir");
        fs::write(
            &input,
            "ACGT\nAACC\nACGT\nGNNN\n\nNNN\nAACC\nACGT\nACGT\n",
        )
        .expect("write input");

        let result = count_file(&input, &results, None).expect("count");
        assert_eq!(result.lines_read, 9);
        assert_eq!(result.sequences_used, 7);
        assert_eq!(result.lines_skipped, 2);
        assert_eq!(result.unique_sequences, 3);
        assert_eq!(result.sample_name, "sample1");
        assert!(result.output_path.exists());

        let csv = fs::read_to_string(&result.output_path).expect("read csv");
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], "sequence,counts,length");
        assert_eq!(lines[1], "ACGT,4,4");
        assert_eq!(lines[2], "AACC,2,4");
        assert_eq!(lines[3], "G,1,1");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn length_filter_collapses_duplicates() {
        let dir = temp_dir("filter");
        let input = dir.join("sample2_unbinned_reads");
        let results = results_dir(&dir);

        fs::create_dir_all(&dir).expect("create input dir");
        fs::write(&input, "ACGTAC\nACGTCC\nACGTGG\nTTAG\n").expect("write input");

        // First 4 bases: ACGT x3, plus TTAG -> 2 unique sequences.
        let result = count_file(&input, &results, Some(4)).expect("count");
        assert_eq!(result.sequences_used, 4);
        assert_eq!(result.unique_sequences, 2);

        let csv = fs::read_to_string(&result.output_path).expect("read csv");
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], "sequence,counts,length");
        assert_eq!(lines[1], "ACGT,3,4");
        assert_eq!(lines[2], "TTAG,1,4");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn examine_returns_top_most_frequent() {
        let dir = temp_dir("examine");
        let input = dir.join("sample3_unbinned_reads");
        fs::create_dir_all(&dir).expect("create input dir");
        // AAAA x4, CCCC x2, GGGG x1, TTTT x1 (8 lines)
        fs::write(&input, "AAAA\nCCCC\nAAAA\nGGGG\nAAAA\nCCCC\nAAAA\nTTTT\n")
            .expect("write input");

        let ex = examine_file(&input).expect("examine");
        assert_eq!(ex.lines_read, 8);
        assert_eq!(ex.unique_sequences, 4);
        assert_eq!(ex.top.len(), 4); // fewer than 10 present
        assert_eq!(ex.top[0].sequence, "AAAA");
        assert_eq!(ex.top[0].count, 4);
        assert_eq!(ex.top[0].length, 4);
        assert_eq!(ex.top[1].sequence, "CCCC");
        assert_eq!(ex.top[1].count, 2);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn examine_honours_the_length_filter() {
        let dir = temp_dir("examine_filter");
        let input = dir.join("sample4_unbinned_reads");
        fs::create_dir_all(&dir).expect("create input dir");
        fs::write(&input, "ACGTAA\nACGTCC\nGGGGTTTT\nGGGGAAAA\n").expect("write input");

        let full = examine_file(&input).expect("full");
        let filtered = examine_file_with(&input, Some(4)).expect("filtered");
        assert_eq!(full.unique_sequences, 4);
        assert_eq!(filtered.unique_sequences, 2);
        // Tie on count, so alphabetical order picks the winner.
        assert_eq!(filtered.top[0].sequence, "ACGT");
        assert_eq!(filtered.top[0].length, 4);
        assert_eq!(filtered.top[0].count, 2);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_target_sequence() {
        assert_eq!(parse_target_sequence("acgt").unwrap(), "ACGT");
        assert_eq!(parse_target_sequence("  ACGT  ").unwrap(), "ACGT");
        assert!(parse_target_sequence("").is_err());
        assert!(parse_target_sequence("   ").is_err());
        assert!(parse_target_sequence("ACNT").is_err());
        assert!(parse_target_sequence("ACGT123").is_err());
    }

    #[test]
    fn counts_reads_containing_sequence() {
        let dir = temp_dir("seqcount");
        let input = dir.join("sample5_unbinned_reads");
        fs::create_dir_all(&dir).expect("create input dir");
        // ACGT appears inside reads on lines 1,2,3 (line 2 twice) and 4; line 5 doesn't.
        fs::write(
            &input,
            "TAC GT\nTTACGTAA\nACGT\nGGGACGTACGT\nCCCC\n",
        )
        .expect("write input");
        // Line 1 "TAC GT" contains a space -> skipped by clean_sequence.
        assert_eq!(scan_sequence_in_file(&input, "ACGT").unwrap().0, 3);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn finds_closest_match_when_no_hit() {
        let dir = temp_dir("closest");
        fs::create_dir_all(&dir).expect("create input dir");
        let input = dir.join("sample6_unbinned_reads");
        fs::write(&input, "GGGGAAAACCCC\n").expect("write input");

        let (hits, closest) = scan_sequence_in_file(&input, "AAAACCCG").expect("scan");
        assert_eq!(hits, 0);
        let closest = closest.expect("a closest window");
        assert_eq!(closest.distance, 1);
        assert_eq!(closest.window, "AAAACCCC");
        assert_eq!(closest.read, "GGGGAAAACCCC");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_closest_match_when_reads_are_shorter_than_query() {
        let dir = temp_dir("closest_short");
        fs::create_dir_all(&dir).expect("create input dir");
        let input = dir.join("sample7_unbinned_reads");
        fs::write(&input, "ACGT\nTTTT\n").expect("write input");

        let (hits, closest) = scan_sequence_in_file(&input, "ACGTACGT").expect("scan");
        assert_eq!(hits, 0);
        assert!(closest.is_none());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn hit_suppresses_closest_match() {
        let dir = temp_dir("closest_hit");
        fs::create_dir_all(&dir).expect("create input dir");
        let input = dir.join("sample8_unbinned_reads");
        fs::write(&input, "GGGGAAAACCCC\n").expect("write input");

        let (hits, closest) = scan_sequence_in_file(&input, "AAAA").expect("scan");
        assert_eq!(hits, 1);
        assert!(closest.is_none());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn writes_combined_sequence_count_csv() {
        let dir = temp_dir("seqcsv");
        let results = results_dir(&dir);
        fs::create_dir_all(&dir).expect("create input dir");
        let a = dir.join("sample1_unbinned_reads");
        let b = dir.join("sample2_unbinned_reads");
        fs::write(&a, "ACGTAA\nTTTT\nACGT\n").expect("write a");
        fs::write(&b, "CCCC\nACGTACGT\n").expect("write b");

        let result = count_sequence_across(&[a, b], "ACGT", &results).expect("count");
        assert_eq!(result.total, 3);
        assert_eq!(result.hits.len(), 2);
        assert_eq!(result.hits[0].sample_name, "sample1");
        assert_eq!(result.hits[0].count, 2);
        assert_eq!(result.hits[1].sample_name, "sample2");
        assert_eq!(result.hits[1].count, 1);

        let csv = fs::read_to_string(&result.output_path).expect("read csv");
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], "sample,ACGT");
        assert_eq!(lines[1], "sample1,2");
        assert_eq!(lines[2], "sample2,1");
        assert_eq!(lines[3], "total,3");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn reverse_complements_bases_in_reverse_order() {
        assert_eq!(reverse_complement("ACGT"), "ACGT");
        assert_eq!(reverse_complement("AAGC"), "GCTT");
        assert_eq!(reverse_complement("AATTGGCC"), "GGCCAATT");
        assert_eq!(reverse_complement("A"), "T");
    }

    #[test]
    fn header_for_default_options_is_the_v0_4_0_header() {
        assert_eq!(sequence_csv_header("ACGT", &SearchOptions::default()), "sample,ACGT");
    }

    #[test]
    fn header_gains_columns_only_for_options_that_are_on() {
        let rc = SearchOptions {
            reverse_complement: true,
            ..SearchOptions::default()
        };
        let mm = SearchOptions {
            max_mismatches: 2,
            ..SearchOptions::default()
        };
        assert_eq!(sequence_csv_header("ACGT", &rc), "sample,ACGT,ACGT_rc,combined");
        assert_eq!(
            sequence_csv_header("ACGT", &mm),
            "sample,ACGT,within_2,best_mismatches"
        );
        assert_eq!(
            sequence_csv_header("ACGT", &SearchOptions { max_mismatches: 2, ..rc }),
            "sample,ACGT,ACGT_rc,combined,within_2,best_mismatches"
        );
    }

    #[test]
    fn counts_forward_and_reverse_strands_separately_and_once_combined() {
        let dir = temp_dir("rc");
        fs::create_dir_all(&dir).expect("create input dir");
        let input = dir.join("sample9_unbinned_reads");
        // forward-only, reverse-only, both, neither.
        fs::write(&input, "TTACGGAA\nGGCCGTCC\nACGGCCGT\nTTTT\n").expect("write input");

        let options = SearchOptions {
            reverse_complement: true,
            ..SearchOptions::default()
        };
        let scan = scan_sequence_in_file_with(&input, "ACGG", &options).expect("scan");
        assert_eq!(scan.exact, 2);
        assert_eq!(scan.reverse, 2);
        // The read containing both strands is still counted once.
        assert_eq!(scan.combined, 3);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn exact_column_does_not_move_when_options_are_enabled() {
        let dir = temp_dir("rc_stable");
        fs::create_dir_all(&dir).expect("create input dir");
        let a = dir.join("sample1_unbinned_reads");
        let b = dir.join("sample2_unbinned_reads");
        fs::write(&a, "TTACGGAA\nACGGCCGT\n").expect("write a");
        fs::write(&b, "GGCCGTCC\nTTTT\n").expect("write b");
        let files = vec![a, b];

        let plain = count_sequence_across(&files, "ACGG", &results_dir(&dir)).expect("plain");
        let options = SearchOptions {
            reverse_complement: true,
            max_mismatches: 3,
        };
        let extended =
            count_sequence_across_with(&files, "ACGG", &dir.join("other"), &options).expect("extended");

        assert_eq!(plain.total, extended.total, "exact counts must not move");
        assert_eq!(
            plain.hits.iter().map(|h| h.count).collect::<Vec<_>>(),
            extended.hits.iter().map(|h| h.count).collect::<Vec<_>>()
        );
        assert_eq!(extended.reverse_total, 2);
        assert_eq!(extended.combined_total, 3);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn counts_reads_within_mismatch_tolerance() {
        let dir = temp_dir("mismatch");
        fs::create_dir_all(&dir).expect("create input dir");
        let input = dir.join("sample10_unbinned_reads");
        // exact / one mismatch / four mismatches / shorter than the query
        fs::write(&input, "AACGTT\nACAT\nTTTT\nAC\n").expect("write input");

        let options = SearchOptions {
            max_mismatches: 1,
            ..SearchOptions::default()
        };
        let scan = scan_sequence_in_file_with(&input, "ACGT", &options).expect("scan");
        assert_eq!(scan.exact, 1);
        assert_eq!(scan.within_tolerance, 2, "exact plus the one-mismatch read");
        assert_eq!(scan.best_distance, Some(0));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn zero_tolerance_covers_both_strands_when_reverse_complement_is_on() {
        let dir = temp_dir("mismatch_zero");
        fs::create_dir_all(&dir).expect("create input dir");
        let input = dir.join("sample11_unbinned_reads");
        fs::write(&input, "TTACGGAA\nGGCCGTCC\nACGGCCGT\nTTTT\n").expect("write input");

        // Both strands with no tolerance: "within 0 mismatches" must agree with
        // the combined containment count, and with the exact count when only
        // the forward strand is searched.
        let forward_only =
            scan_sequence_in_file_with(&input, "ACGG", &SearchOptions::default()).expect("forward");
        let both_strands = scan_sequence_in_file_with(
            &input,
            "ACGG",
            &SearchOptions {
                reverse_complement: true,
                ..SearchOptions::default()
            },
        )
        .expect("both strands");

        assert_eq!(forward_only.exact, 2);
        assert_eq!(forward_only.within_tolerance, forward_only.exact);
        assert_eq!(both_strands.within_tolerance, both_strands.combined);
        assert_eq!(both_strands.best_distance, Some(0));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn scans_shorter_than_query_have_no_best_distance() {
        let dir = temp_dir("mismatch_short");
        fs::create_dir_all(&dir).expect("create input dir");
        let input = dir.join("sample12_unbinned_reads");
        fs::write(&input, "AC\nGAT\n").expect("write input");

        let scan = scan_sequence_in_file_with(
            &input,
            "ACGTACGT",
            &SearchOptions {
                max_mismatches: 2,
                ..SearchOptions::default()
            },
        )
        .expect("scan");
        assert_eq!(scan.exact, 0);
        assert_eq!(scan.best_distance, None);
        assert_eq!(scan.within_tolerance, 0);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parallel_scan_aggregate_write_matches_the_sequential_helper() {
        let dir = temp_dir("aggregate");
        fs::create_dir_all(&dir).expect("create input dir");
        let a = dir.join("sample1_unbinned_reads");
        let b = dir.join("sample2_unbinned_reads");
        fs::write(&a, "ACGTAA\nTTTT\nACGT\n").expect("write a");
        fs::write(&b, "CCCC\nACGTACGT\n").expect("write b");
        let files = vec![a.clone(), b.clone()];

        let sequential = count_sequence_across(&files, "ACGT", &dir.join("seq")).expect("sequential");

        // What the GUI does: scan each file on its own, then aggregate and write.
        let hits: Vec<SampleHit> = files
            .iter()
            .map(|path| {
                let scan = scan_sequence_in_file_with(path, "ACGT", &SearchOptions::default())
                    .expect("scan");
                sample_hit_from_scan(path, scan)
            })
            .collect();
        let result = aggregate_hits(&dir.join("manual"), "ACGT", hits, SearchOptions::default());
        write_sequence_result(&result).expect("write");

        assert_eq!(result.total, sequential.total);
        assert_eq!(
            fs::read_to_string(&result.output_path).expect("manual csv"),
            fs::read_to_string(&sequential.output_path).expect("helper csv")
        );

        let _ = fs::remove_dir_all(&dir);
    }
}
