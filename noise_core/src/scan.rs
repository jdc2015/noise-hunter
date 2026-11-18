use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};

/// Files the program cares about are plain-text "unbinned reads" files whose
/// name ends with `_unbinned_reads` (e.g. `sample1_unbinned_reads`).
pub fn is_read_file(path: &Path) -> bool {
    path.file_name()
        .map(|name| name.to_string_lossy().ends_with("_unbinned_reads"))
        .unwrap_or(false)
}

fn is_hidden_name(path: &Path) -> bool {
    path.file_name()
        .map(|name| name.to_string_lossy().starts_with('.'))
        .unwrap_or(false)
}

#[derive(Clone, Debug)]
pub struct DirectoryListing {
    pub current: PathBuf,
    pub parent: Option<PathBuf>,
    pub subfolders: Vec<PathBuf>,
    pub read_files: Vec<PathBuf>,
}

pub fn list_directory_for_browse(dir: &Path) -> Result<DirectoryListing, String> {
    if !dir.is_dir() {
        return Err(format!("'{}' is not a valid directory", dir.display()));
    }

    let current = fs::canonicalize(dir)
        .map_err(|e| format!("Could not resolve directory '{}': {e}", dir.display()))?;

    let parent = current.parent().map(|p| p.to_path_buf());
    let entries = fs::read_dir(&current).map_err(|e| format!("Could not read directory: {e}"))?;

    let mut subfolders = Vec::new();
    let mut read_files = Vec::new();

    for entry in entries {
        let entry = entry.map_err(|e| format!("Could not read directory entry: {e}"))?;
        let path = entry.path();
        if is_hidden_name(&path) {
            continue;
        }
        if path.is_dir() {
            subfolders.push(path);
        } else if path.is_file() && is_read_file(&path) {
            read_files.push(path);
        }
    }

    subfolders.sort();
    sort_read_files_naturally(&mut read_files);

    Ok(DirectoryListing {
        current,
        parent,
        subfolders,
        read_files,
    })
}

/// Parses the sample number out of `sampleN_unbinned_reads`, if the name
/// matches that shape.
fn sample_number(name: &str) -> Option<u64> {
    let stem = name.strip_suffix("_unbinned_reads")?;
    let num = stem.strip_prefix("sample")?;
    num.parse::<u64>().ok()
}

/// Sort `sampleN_unbinned_reads` in numeric order (sample1, sample2,
/// ... sample10, sample11 ...) so the "first file" is the natural first sample.
/// Any non-matching names sort afterwards, lexicographically.
fn sort_read_files_naturally(files: &mut [PathBuf]) {
    files.sort_by(|a, b| {
        let name_a = a.file_name().map(|n| n.to_string_lossy().to_string());
        let name_b = b.file_name().map(|n| n.to_string_lossy().to_string());
        let num_a = name_a.as_deref().and_then(sample_number);
        let num_b = name_b.as_deref().and_then(sample_number);

        match (num_a, num_b) {
            (Some(x), Some(y)) => x.cmp(&y).then_with(|| a.cmp(b)),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => a.cmp(b),
        }
    });
}

/// Derives the sample name by stripping the `_unbinned_reads` suffix.
/// `sample1_unbinned_reads` -> `sample1`.
pub fn sample_name_from_path(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    name.strip_suffix("_unbinned_reads")
        .unwrap_or(&name)
        .to_string()
}

/// The output folder is a new `noise_results` folder created as a sibling of
/// the folder holding the selected input(s).
pub fn results_dir(input_folder: &Path) -> PathBuf {
    input_folder
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("noise_results")
}

pub fn output_path(results_dir: &Path, sample_name: &str) -> PathBuf {
    results_dir.join(format!("noisecounts_{sample_name}.csv"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn recognizes_read_files() {
        assert!(is_read_file(Path::new("sample1_unbinned_reads")));
        assert!(!is_read_file(Path::new("sample1_binned_results.csv")));
        assert!(!is_read_file(Path::new("notes.txt")));
    }

    #[test]
    fn derives_sample_name() {
        assert_eq!(
            sample_name_from_path(Path::new("sample1_unbinned_reads")),
            "sample1"
        );
        assert_eq!(
            sample_name_from_path(Path::new("/data/sample42_unbinned_reads")),
            "sample42"
        );
    }

    #[test]
    fn results_are_siblings_of_input_folder() {
        let folder = Path::new("/data/counted_results");
        assert_eq!(results_dir(folder), PathBuf::from("/data/noise_results"));
    }

    #[test]
    fn output_filename_matches_spec() {
        assert_eq!(
            output_path(Path::new("/data/noise_results"), "sample1"),
            PathBuf::from("/data/noise_results/noisecounts_sample1.csv")
        );
    }

    #[test]
    fn lists_read_files_in_directory() {
        let dir = std::env::temp_dir().join(format!(
            "noise_counter_scan_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(dir.join("sub")).expect("subfolder");
        fs::write(dir.join("sample1_unbinned_reads"), b"ACGT\n").expect("read file");
        fs::write(dir.join("sample2_unbinned_reads"), b"ACGT\n").expect("read file");
        fs::write(dir.join("sample1_binned_results.csv"), b"x\n").expect("csv");

        let listing = list_directory_for_browse(&dir).expect("list");
        let names: Vec<_> = listing
            .read_files
            .iter()
            .filter_map(|p| p.file_name())
            .collect();

        assert_eq!(names.len(), 2);
        assert!(names.iter().any(|n| *n == "sample1_unbinned_reads"));
        assert!(names.iter().any(|n| *n == "sample2_unbinned_reads"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sorts_read_files_in_numeric_order() {
        let dir = std::env::temp_dir().join(format!(
            "noise_counter_sort_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("dir");
        for name in [
            "sample10_unbinned_reads",
            "sample2_unbinned_reads",
            "sample1_unbinned_reads",
            "sample20_unbinned_reads",
        ] {
            fs::write(dir.join(name), b"ACGT\n").expect("write");
        }

        let listing = list_directory_for_browse(&dir).expect("list");
        let names: Vec<String> = listing
            .read_files
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();

        assert_eq!(
            names,
            vec![
                "sample1_unbinned_reads",
                "sample2_unbinned_reads",
                "sample10_unbinned_reads",
                "sample20_unbinned_reads",
            ]
        );

        let _ = fs::remove_dir_all(&dir);
    }
}
