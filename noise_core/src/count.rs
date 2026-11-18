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
    let tally = tally_file(path, None)?;
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

/// One sample's hit count for a specific-sequence query.
#[derive(Clone)]
pub struct SampleHit {
    pub sample_name: String,
    pub count: u64,
    /// Populated only when `count == 0`: the closest non-matching read window.
    pub closest: Option<ClosestMatch>,
}

/// Result of counting one specific sequence across one or more sample files.
#[derive(Clone)]
pub struct SequenceQueryResult {
    pub sequence: String,
    pub hits: Vec<SampleHit>,
    pub total: u64,
    pub output_path: PathBuf,
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

/// Counts reads containing `sequence` and, when no read matches at all, also
/// finds the closest equal-length window (minimum Hamming distance). The
/// diagnostic is skipped entirely as soon as the first hit is seen, so samples
/// with hits pay no extra cost.
pub fn scan_sequence_in_file(
    path: &Path,
    sequence: &str,
) -> Result<(u64, Option<ClosestMatch>), String> {
    let file = File::open(path).map_err(|e| format!("Could not open '{}': {e}", path.display()))?;
    let reader = BufReader::with_capacity(1024 * 1024, file);

    let query = sequence.as_bytes();
    let query_len = query.len();
    let mut hits = 0u64;
    let mut closest: Option<ClosestMatch> = None;

    for line in reader.lines() {
        let line = line.map_err(|e| format!("Could not read '{}': {e}", path.display()))?;
        let Some(cleaned) = clean_sequence(&line) else {
            continue;
        };

        if cleaned.contains(sequence) {
            hits += 1;
            // A hit means this sample needs no diagnostic; stop tracking it.
            closest = None;
        } else if hits == 0 {
            if let Some((distance, start)) = closest_window(&cleaned, query) {
                let better = closest
                    .as_ref()
                    .map_or(true, |current| distance < current.distance);
                if better {
                    closest = Some(ClosestMatch {
                        distance,
                        window: cleaned[start..start + query_len].to_string(),
                        read: cleaned,
                    });
                }
            }
        }
    }

    Ok((hits, closest))
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

/// Counts one sequence across `files` and writes a single combined CSV whose
/// header is `sample,<SEQUENCE>`, followed by one row per sample and a total.
pub fn count_sequence_across(
    files: &[PathBuf],
    sequence: &str,
    results_dir: &Path,
) -> Result<SequenceQueryResult, String> {
    fs::create_dir_all(results_dir).map_err(|e| {
        format!(
            "Could not create results directory '{}': {e}",
            results_dir.display()
        )
    })?;

    let mut hits = Vec::with_capacity(files.len());
    let mut total = 0u64;
    for path in files {
        let (count, closest) = scan_sequence_in_file(path, sequence)?;
        total += count;
        hits.push(SampleHit {
            sample_name: sample_name_from_path(path),
            count,
            closest,
        });
    }

    let output_path = results_dir.join("sequencecount.csv");
    write_sequence_counts(&output_path, sequence, &hits, total)?;

    Ok(SequenceQueryResult {
        sequence: sequence.to_string(),
        hits,
        total,
        output_path,
    })
}

fn write_sequence_counts(
    path: &Path,
    sequence: &str,
    hits: &[SampleHit],
    total: u64,
) -> Result<(), String> {
    let mut file =
        File::create(path).map_err(|e| format!("Could not create output '{}': {e}", path.display()))?;

    writeln!(file, "sample,{sequence}")
        .map_err(|e| format!("Could not write output '{}': {e}", path.display()))?;
    for hit in hits {
        writeln!(file, "{},{}", hit.sample_name, hit.count)
            .map_err(|e| format!("Could not write output '{}': {e}", path.display()))?;
    }
    writeln!(file, "total,{total}")
        .map_err(|e| format!("Could not write output '{}': {e}", path.display()))?;

    Ok(())
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
}
