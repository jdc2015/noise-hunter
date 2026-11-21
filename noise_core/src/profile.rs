//! One row per sample: what its reads are made of.
//!
//! The profile is the noise table in miniature — how many reads carry the
//! barcode, where the indices turn up, how much of the file is padding — so that
//! samples can be compared without opening their CSVs. It is written to a fixed
//! name beside the other outputs, and rows come out in whatever order the caller
//! passes its files.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use crate::anatomy::anatomise;
use crate::scan::sample_name_from_path;
use crate::tables::{SampleIndices, SampleSheet};

/// The file [`write_profile`] writes, beside the count and search outputs.
pub const PROFILE_FILE: &str = "noise_profile.csv";

/// What one sample's reads are made of. `reads` counts usable reads only: lines
/// the counter would skip are not reads, so they are not profiled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SampleProfile {
    pub sample_name: String,
    pub reads: u64,
    pub barcode: u64,
    pub index1: u64,
    pub index1_slot: u64,
    pub index1_elsewhere: u64,
    pub index2: u64,
    pub both_indices: u64,
    pub neither_index: u64,
    pub poly_g: u64,
    /// Effective lengths, counted exactly. A map rather than a vector so a file
    /// with millions of reads costs only the number of distinct lengths.
    lengths: BTreeMap<usize, u64>,
}

impl SampleProfile {
    fn empty(sample_name: &str) -> Self {
        Self {
            sample_name: sample_name.to_string(),
            reads: 0,
            barcode: 0,
            index1: 0,
            index1_slot: 0,
            index1_elsewhere: 0,
            index2: 0,
            both_indices: 0,
            neither_index: 0,
            poly_g: 0,
            lengths: BTreeMap::new(),
        }
    }

    /// The lower middle effective length — the usual median for an even count.
    pub fn median_effective_len(&self) -> usize {
        if self.reads == 0 {
            return 0;
        }
        let target = self.reads.div_ceil(2);
        let mut seen = 0;
        for (length, count) in &self.lengths {
            seen += count;
            if seen >= target {
                return *length;
            }
        }
        0
    }
}

/// Profiles one sample's reads. `indices` is the sample's row from the sheet and
/// `barcode` the allele-locus 27-mer to look for.
pub fn profile_text(
    text: &str,
    sample_name: &str,
    indices: &SampleIndices,
    barcode: &str,
) -> SampleProfile {
    let mut profile = SampleProfile::empty(sample_name);

    for line in text.lines() {
        let Some(anatomy) = anatomise(line, indices, barcode) else {
            continue;
        };
        profile.reads += 1;
        if anatomy.has_barcode() {
            profile.barcode += 1;
        }
        match (anatomy.index1, anatomy.index2) {
            (Some(_), Some(_)) => profile.both_indices += 1,
            (None, None) => profile.neither_index += 1,
            _ => {}
        }
        if anatomy.index1.is_some() {
            profile.index1 += 1;
            if anatomy.index1_expected {
                profile.index1_slot += 1;
            } else {
                profile.index1_elsewhere += 1;
            }
        }
        if anatomy.index2.is_some() {
            profile.index2 += 1;
        }
        if anatomy.poly_g_start.is_some() {
            profile.poly_g += 1;
        }
        *profile.lengths.entry(anatomy.effective_len).or_insert(0) += 1;
    }

    profile
}

/// Profiles one file, naming the row after the file.
pub fn profile_file(
    path: &Path,
    indices: &SampleIndices,
    barcode: &str,
) -> Result<SampleProfile, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("Could not read '{}': {error}", path.display()))?;
    Ok(profile_text(&text, &sample_name_from_path(path), indices, barcode))
}

/// Profiles files in the order given, taking each sample's indices from the
/// sheet. A file whose sample the sheet does not list is an error, not a guess:
/// without indices there is nothing to look for.
pub fn profile_files(
    files: &[PathBuf],
    sheet: &SampleSheet,
    barcode: &str,
) -> Result<Vec<SampleProfile>, String> {
    let mut profiles = Vec::with_capacity(files.len());
    for path in files {
        let name = sample_name_from_path(path);
        let indices = sheet.get(&name).ok_or_else(|| {
            format!("sample '{name}' is not in the sample sheet, so its indices are unknown")
        })?;
        profiles.push(profile_file(path, indices, barcode)?);
    }
    Ok(profiles)
}

/// Writes the profile table, in the order given, and returns the path.
pub fn write_profile(
    profiles: &[SampleProfile],
    results_dir: &Path,
) -> Result<PathBuf, String> {
    fs::create_dir_all(results_dir).map_err(|error| {
        format!(
            "Could not create results directory '{}': {error}",
            results_dir.display()
        )
    })?;
    let path = results_dir.join(PROFILE_FILE);

    let mut text = String::from(
        "sample,reads,barcode,index1,index1_slot,index1_elsewhere,index2,\
         both_indices,neither_index,poly_g,median_effective_len\n",
    );
    for profile in profiles {
        writeln!(
            text,
            "{},{},{},{},{},{},{},{},{},{},{}",
            profile.sample_name,
            profile.reads,
            profile.barcode,
            profile.index1,
            profile.index1_slot,
            profile.index1_elsewhere,
            profile.index2,
            profile.both_indices,
            profile.neither_index,
            profile.poly_g,
            profile.median_effective_len(),
        )
        .map_err(|error| error.to_string())?;
    }

    fs::write(&path, text)
        .map_err(|error| format!("Could not write '{}': {error}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BARCODE: &str = "CACCAGACACGTCTGAACTCCAGTCAC";
    const POLY_G: &str = "GGGGGGGGGG";

    fn indices() -> SampleIndices {
        SampleIndices {
            name: "sample1".to_string(),
            index1: "ATATGCGC".to_string(),
            index2: "CTGATCGT".to_string(),
        }
    }

    /// One read with the barcode, index1 in its slot and a padded end.
    fn padded_read() -> String {
        format!("{BARCODE}ATATGCGCATCT{POLY_G}")
    }

    #[test]
    fn counts_each_class_once_per_read() {
        let text = [
            padded_read(),                                        // all three
            format!("{BARCODE}GGATATGCGCATCT"),                   // index1 elsewhere
            format!("{BARCODE}ATATGCGCATCT"),                     // no padding
            "ACGTACGTACGT".to_string(),                           // neither
            "NNNN".to_string(),                                   // not a read
        ]
        .join("\n");

        let profile = profile_text(&text, "sample1", &indices(), BARCODE);

        assert_eq!(profile.reads, 4, "the all-N line is not a read");
        assert_eq!(profile.barcode, 3);
        assert_eq!(profile.index1, 3);
        assert_eq!(profile.index1_slot, 2);
        assert_eq!(profile.index1_elsewhere, 1);
        assert_eq!(profile.index2, 0);
        assert_eq!(profile.both_indices, 0);
        assert_eq!(profile.neither_index, 1);
        assert_eq!(profile.poly_g, 1);
    }

    #[test]
    fn the_median_is_the_lower_middle_effective_length() {
        let profile = profile_text(
            &[
                format!("{BARCODE}ATATGCGCATCT{POLY_G}"), // 49 - 10 = 39
                format!("{BARCODE}ATATGCGCATCT"),         // 39
                "ACGTACGTACGT".to_string(),               // 12
                format!("{BARCODE}GGATATGCGCATCT"),       // 41
            ]
            .join("\n"),
            "sample1",
            &indices(),
            BARCODE,
        );
        // Sorted: 12, 39, 39, 41 — the lower middle is the second value.
        assert_eq!(profile.median_effective_len(), 39);
    }

    #[test]
    fn an_empty_file_has_a_zero_median_and_no_reads() {
        let profile = profile_text("", "sample1", &indices(), BARCODE);
        assert_eq!(profile.reads, 0);
        assert_eq!(profile.median_effective_len(), 0);
    }

    #[test]
    fn the_written_table_has_the_agreed_header_and_rows_in_order() {
        let a = profile_text(&padded_read(), "sample2", &indices(), BARCODE);
        let b = profile_text("ACGTACGTACGT", "sample1", &indices(), BARCODE);
        let dir = std::env::temp_dir().join(format!(
            "noise_profile_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));

        let path = write_profile(&[a, b], &dir).expect("write");
        let text = std::fs::read_to_string(&path).expect("read back");
        let lines: Vec<&str> = text.lines().collect();

        assert_eq!(
            lines[0],
            "sample,reads,barcode,index1,index1_slot,index1_elsewhere,index2,\
             both_indices,neither_index,poly_g,median_effective_len"
        );
        assert!(lines[1].starts_with("sample2,1,1,1,1,0,0,0,0,1,"), "{}", lines[1]);
        assert!(lines[2].starts_with("sample1,1,0,0,0,0,0,0,1,0,"), "{}", lines[2]);
        assert_eq!(lines.len(), 3, "header plus one row per profile");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
