//! Patterns that appear across the selected samples.
//!
//! A k-mer present in one unbinned list is a curiosity; the same k-mer in every
//! list is a finding. This module intersects the k-mers of the selected files and
//! reports, for each survivor, how many samples hold it, how many times in each,
//! and what chance alone would have produced.
//!
//! `k` is the caller's choice and deliberately arbitrary: it is not tied to the
//! 27-base barcode, and a shared k-mer may contain part of the barcode, part of an
//! index, or nothing recognisable.
//!
//! The chance figure is a mononucleotide model: a k-mer's expected count in a
//! sample is that sample's window count times the product of its base
//! frequencies. It is crude — it ignores dinucleotide bias — but it is enough to
//! show when a G-rich k-mer is simply what G-rich reads produce.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use crate::count::clean_sequence;

/// The folder [`write_shared_kmers`] writes into, under `noise_results/`.
pub const PATTERNS_DIR: &str = "shared_kmers";

/// One k-mer found in every selected sample.
#[derive(Clone, Debug, PartialEq)]
pub struct SharedKmer {
    pub kmer: String,
    /// Count in each sample, aligned with the files the search was given.
    pub per_sample: Vec<u64>,
    /// Count chance alone would produce, summed over the samples.
    pub expected: f64,
}

impl SharedKmer {
    pub fn total(&self) -> u64 {
        self.per_sample.iter().sum()
    }

    pub fn samples_present(&self) -> usize {
        self.per_sample.iter().filter(|count| **count > 0).count()
    }

    /// Observed divided by expected. `None` when the expectation is zero, which
    /// can only happen for an empty sample set.
    pub fn observed_over_expected(&self) -> Option<f64> {
        (self.expected > 0.0).then(|| self.total() as f64 / self.expected)
    }
}

/// The result of a pattern search.
#[derive(Clone, Debug, PartialEq)]
pub struct PatternSearch {
    pub patterns: Vec<SharedKmer>,
    /// How many of the given files had at least one k-mer to compare. A file with
    /// no reads (or none at least `k` long) cannot hold a pattern, so it is not
    /// required to: requiring it would let one empty sample erase every finding.
    pub samples_compared: usize,
}

/// Every k-mer that appears at least once in **every comparable file**, ranked by
/// total count (ties alphabetical). With one file this is that file's k-mers.
pub fn shared_kmers(files: &[PathBuf], k: usize) -> Result<PatternSearch, String> {
    if files.is_empty() {
        return Err("no files to compare".to_string());
    }
    if k == 0 {
        return Err("k must be at least 1".to_string());
    }

    // Pass 1: intersect, file by file. Only the running candidate set is held in
    // memory, so a large set costs the size of one file, not all of them.
    let mut candidates: Option<HashSet<String>> = None;
    let mut samples_compared = 0;
    for path in files {
        let here = kmers_in_file(path, k)?;
        if here.is_empty() {
            continue;
        }
        samples_compared += 1;
        candidates = Some(match candidates {
            None => here,
            Some(current) => current
                .into_iter()
                .filter(|kmer| here.contains(kmer))
                .collect(),
        });
        if candidates.as_ref().is_some_and(HashSet::is_empty) {
            break;
        }
    }

    let Some(candidates) = candidates else {
        return Ok(PatternSearch {
            patterns: Vec::new(),
            samples_compared: 0,
        });
    };
    if candidates.is_empty() {
        return Ok(PatternSearch {
            patterns: Vec::new(),
            samples_compared,
        });
    }

    // Pass 2: count the survivors in every file, and measure each file's windows
    // and base composition for the chance figure.
    let index: HashMap<&str, usize> = candidates
        .iter()
        .enumerate()
        .map(|(position, kmer)| (kmer.as_str(), position))
        .collect();
    let mut per_sample = vec![vec![0u64; files.len()]; candidates.len()];
    let mut windows = vec![0u64; files.len()];
    let mut bases = vec![[0u64; 4]; files.len()];

    for (file_index, path) in files.iter().enumerate() {
        let text = fs::read_to_string(path)
            .map_err(|error| format!("Could not read '{}': {error}", path.display()))?;
        for line in text.lines() {
            let Some(cleaned) = clean_sequence(line) else {
                continue;
            };
            for base in cleaned.bytes() {
                bases[file_index][base_slot(base)] += 1;
            }
            if cleaned.len() < k {
                continue;
            }
            windows[file_index] += (cleaned.len() - k + 1) as u64;
            for start in 0..=cleaned.len() - k {
                // Cleaned sequences are A/C/G/T only, so every byte is a boundary.
                if let Some(&position) = index.get(&cleaned[start..start + k]) {
                    per_sample[position][file_index] += 1;
                }
            }
        }
    }

    let mut patterns: Vec<SharedKmer> = candidates
        .into_iter()
        .enumerate()
        .map(|(position, kmer)| {
            let expected = (0..files.len())
                .map(|file_index| {
                    windows[file_index] as f64 * kmer_probability(&kmer, &bases[file_index])
                })
                .sum();
            SharedKmer {
                kmer,
                per_sample: per_sample[position].clone(),
                expected,
            }
        })
        .collect();

    patterns.sort_by(|left, right| {
        right
            .total()
            .cmp(&left.total())
            .then_with(|| left.kmer.cmp(&right.kmer))
    });
    Ok(PatternSearch {
        patterns,
        samples_compared,
    })
}

/// The distinct k-mers of one file. Distinct is all pass 1 needs: presence.
fn kmers_in_file(path: &Path, k: usize) -> Result<HashSet<String>, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("Could not read '{}': {error}", path.display()))?;
    let mut found = HashSet::new();
    for line in text.lines() {
        let Some(cleaned) = clean_sequence(line) else {
            continue;
        };
        if cleaned.len() < k {
            continue;
        }
        for start in 0..=cleaned.len() - k {
            found.insert(cleaned[start..start + k].to_string());
        }
    }
    Ok(found)
}

fn base_slot(base: u8) -> usize {
    match base {
        b'A' => 0,
        b'C' => 1,
        b'G' => 2,
        _ => 3,
    }
}

/// The mononucleotide probability of one k-mer given a sample's base counts.
fn kmer_probability(kmer: &str, bases: &[u64; 4]) -> f64 {
    let total: u64 = bases.iter().sum();
    if total == 0 {
        return 0.0;
    }
    kmer.bytes().fold(1.0, |probability, base| {
        probability * (bases[base_slot(base)] as f64 / total as f64)
    })
}

/// Writes the pattern search into `noise_results/shared_kmers/`: a ranked summary,
/// a per-sample long table, and the settings it was run with. Returns the folder.
pub fn write_shared_kmers(
    search: &PatternSearch,
    samples: &[String],
    k: usize,
    results_dir: &Path,
    command: &str,
) -> Result<PathBuf, String> {
    let folder = results_dir.join(PATTERNS_DIR);
    fs::create_dir_all(&folder).map_err(|error| {
        format!("Could not create '{}': {error}", folder.display())
    })?;

    let mut summary = String::from(
        "kmer,samples_present,samples_selected,samples_compared,total,expected,\
         observed_over_expected\n",
    );
    let mut long = String::from("kmer,sample,count\n");
    for pattern in &search.patterns {
        let ratio = pattern
            .observed_over_expected()
            .map(|value| format!("{value:.3}"))
            .unwrap_or_else(|| "n/a".to_string());
        writeln!(
            summary,
            "{},{},{},{},{},{:.3},{}",
            pattern.kmer,
            pattern.samples_present(),
            samples.len(),
            search.samples_compared,
            pattern.total(),
            pattern.expected,
            ratio,
        )
        .map_err(|error| error.to_string())?;
        for (sample, count) in samples.iter().zip(&pattern.per_sample) {
            writeln!(long, "{},{sample},{count}", pattern.kmer)
                .map_err(|error| error.to_string())?;
        }
    }

    fs::write(folder.join("shared_kmers.csv"), summary)
        .map_err(|error| format!("Could not write the summary: {error}"))?;
    fs::write(folder.join("shared_kmers_by_sample.csv"), long)
        .map_err(|error| format!("Could not write the per-sample table: {error}"))?;

    let mut notes = format!(
        "k={k}\nsamples_selected={}\nsamples_compared={}\nthreshold=all compared\nsamples={}\ncommand={command}\n",
        samples.len(),
        search.samples_compared,
        samples.join(",")
    );
    if notes.ends_with("command=\n") {
        notes.truncate(notes.len() - "command=\n".len());
    }
    fs::write(folder.join("run.txt"), notes)
        .map_err(|error| format!("Could not write run.txt: {error}"))?;

    Ok(folder)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "noise_patterns_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        fs::create_dir_all(&path).expect("create temp dir");
        path
    }

    const SHARED: &str = "ACGTTGCAAGGC"; // 12 bases, no internal repeat
    const ONCE: &str = "TTTTTTTTTTTT"; // 12 bases, in one file only

    fn write_samples(dir: &Path) -> Vec<PathBuf> {
        let bodies = [
            format!("{SHARED}GGGGATATC\nACGTGCA{SHARED}TTAC\n"),
            format!("{SHARED}CACAC{SHARED}\n{ONCE}\n"),
            format!("CCCC{SHARED}CCCC\n"),
        ];
        bodies
            .iter()
            .enumerate()
            .map(|(index, body)| {
                let path = dir.join(format!("sample{}_unbinned_reads", index + 1));
                fs::write(&path, body).expect("write sample");
                path
            })
            .collect()
    }

    #[test]
    fn a_kmer_in_every_file_survives_and_one_in_a_single_file_does_not() {
        let dir = temp_dir("shared");
        let files = write_samples(&dir);

        let search = shared_kmers(&files, SHARED.len()).expect("search");
        let found: Vec<&SharedKmer> = search
            .patterns
            .iter()
            .filter(|pattern| pattern.kmer == SHARED)
            .collect();

        assert_eq!(found.len(), 1, "the planted k-mer is shared");
        assert_eq!(found[0].samples_present(), 3);
        assert_eq!(found[0].per_sample, vec![2, 2, 1], "counted per sample");
        assert_eq!(found[0].total(), 5);
        assert_eq!(search.samples_compared, 3);
        assert!(
            !search.patterns.iter().any(|pattern| pattern.kmer == ONCE),
            "a k-mer in one file is not shared"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_kmer_longer_than_every_read_finds_nothing() {
        let dir = temp_dir("long_k");
        let files = write_samples(&dir);
        let search = shared_kmers(&files, 200).expect("search");
        assert!(search.patterns.is_empty());
        assert_eq!(search.samples_compared, 0, "nothing was long enough to compare");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_empty_sample_is_not_required_to_hold_a_pattern() {
        let dir = temp_dir("empty");
        let mut files = write_samples(&dir);
        let empty = dir.join("sample4_unbinned_reads");
        fs::write(&empty, "").expect("write empty file");
        files.push(empty);

        let search = shared_kmers(&files, SHARED.len()).expect("search");

        assert_eq!(search.samples_compared, 3, "the empty file is skipped");
        assert!(
            search.patterns.iter().any(|pattern| pattern.kmer == SHARED),
            "one empty sample must not erase the finding"
        );
        assert!(
            search
                .patterns
                .iter()
                .all(|pattern| pattern.per_sample.len() == 4),
            "counts stay aligned with the files given"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn one_file_returns_its_own_kmers() {
        let dir = temp_dir("one_file");
        let files = write_samples(&dir);
        let search = shared_kmers(&files[..1], SHARED.len()).expect("search");
        assert!(search.patterns.iter().any(|pattern| pattern.kmer == SHARED));
        assert!(
            search
                .patterns
                .iter()
                .all(|pattern| pattern.samples_present() == 1)
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_expected_count_is_positive_and_the_ratio_is_finite() {
        let dir = temp_dir("expected");
        let files = write_samples(&dir);
        let search = shared_kmers(&files, SHARED.len()).expect("search");
        let shared = search
            .patterns
            .iter()
            .find(|pattern| pattern.kmer == SHARED)
            .expect("shared k-mer");
        assert!(shared.expected > 0.0, "chance is not zero");
        assert!(shared.observed_over_expected().is_some());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_written_folder_holds_the_summary_the_long_table_and_the_settings() {
        let dir = temp_dir("write");
        let files = write_samples(&dir);
        let search = shared_kmers(&files, SHARED.len()).expect("search");
        let samples = vec!["sample1".to_string(), "sample2".to_string(), "sample3".to_string()];

        let folder =
            write_shared_kmers(&search, &samples, SHARED.len(), &dir, "noise_profile --k 12")
                .expect("write");
        assert_eq!(folder.file_name().and_then(|name| name.to_str()), Some(PATTERNS_DIR));

        let summary = fs::read_to_string(folder.join("shared_kmers.csv")).expect("summary");
        assert!(summary.starts_with(
            "kmer,samples_present,samples_selected,samples_compared,total,expected,\
             observed_over_expected\n"
        ));
        assert!(summary.contains(&format!("{SHARED},3,3,3,")), "{summary}");

        let long = fs::read_to_string(folder.join("shared_kmers_by_sample.csv")).expect("long");
        assert!(long.starts_with("kmer,sample,count\n"));
        assert!(long.contains(&format!("{SHARED},sample3,1")), "{long}");

        let notes = fs::read_to_string(folder.join("run.txt")).expect("run notes");
        assert!(notes.contains("k=12"), "{notes}");
        assert!(notes.contains("samples_selected=3"), "{notes}");
        assert!(notes.contains("samples_compared=3"), "{notes}");
        assert!(notes.contains("noise_profile --k 12"), "{notes}");

        let _ = fs::remove_dir_all(&dir);
    }
}
