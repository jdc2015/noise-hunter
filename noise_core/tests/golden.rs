//! Regression guard for the whole GUI effort: it re-runs both jobs over the
//! real sample data and checks every generated CSV against `baseline.sha256`,
//! captured from v0.4.0 before `noise_core` was extracted.
//!
//! ```text
//! cargo test --release -p noise_core -- --ignored
//! ```
//!
//! Ignored by default because it reads the 18 MB sample set in the repository
//! root; the unit tests need no sample data at all.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use noise_core::count::{count_file, count_sequence_across};
use noise_core::scan::list_directory_for_browse;

// Kept in step with examples/baseline.rs, which produced baseline.sha256.
const QUERY: &str = "CACCAGACACGTCTGAACTCCAGTCAC";
const LENGTH_FILTER: usize = 30;

#[test]
#[ignore = "reads the real sample data; run explicitly with --ignored"]
fn real_data_output_matches_v0_4_0_baseline() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.parent().expect("package dir has a parent");
    let input = repo_root.join("counted_results");
    let manifest = repo_root.join("baseline.sha256");

    let expected = read_manifest(&manifest).expect("read baseline.sha256");
    assert!(!expected.is_empty(), "baseline.sha256 is empty");

    // Same layout the baseline driver used: full/ len30/ seq/ under one root.
    let out_root = std::env::temp_dir().join(format!(
        "noise_core_golden_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&out_root);
    let files = list_directory_for_browse(&input)
        .expect("list counted_results")
        .read_files;
    assert_eq!(
        files.len(),
        96,
        "expected the 96 sample files counted_results holds"
    );

    for path in &files {
        count_file(path, &out_root.join("full"), None).expect("count full length");
    }
    for path in &files {
        count_file(path, &out_root.join(format!("len{LENGTH_FILTER}")), Some(LENGTH_FILTER))
            .expect("count with length filter");
    }
    let query = count_sequence_across(&files, QUERY, &out_root.join("seq")).expect("query");
    assert_eq!(query.total, 74_985, "README's worked example total");

    let mut produced = Vec::new();
    collect_csvs(&out_root, &out_root, &mut produced);
    produced.sort();

    // Same set of files, no extras: a rename or an extra column would show up here.
    let produced_names: Vec<String> = produced
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    let expected_names: Vec<String> = expected.keys().cloned().collect();
    assert_eq!(produced_names, expected_names, "output file set changed");

    for relative in &produced {
        let name = relative.to_string_lossy().to_string();
        let actual = sha256_of(&out_root.join(relative));
        assert_eq!(
            Some(&actual),
            expected.get(&name),
            "output changed for {name}"
        );
    }

    let _ = fs::remove_dir_all(&out_root);
}

fn read_manifest(path: &Path) -> Result<BTreeMap<String, String>, String> {
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut map = BTreeMap::new();
    for line in text.lines() {
        let (hash, name) = line
            .split_once("  ")
            .ok_or_else(|| format!("unparsable manifest line '{line}'"))?;
        map.insert(name.to_string(), hash.to_string());
    }
    Ok(map)
}

fn collect_csvs(root: &Path, dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read output dir") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            collect_csvs(root, &path, found);
        } else if path.extension().map(|e| e == "csv").unwrap_or(false) {
            found.push(path.strip_prefix(root).expect("relative path").to_path_buf());
        }
    }
}

fn sha256_of(path: &Path) -> String {
    let mut file = File::open(path).expect("open output csv");
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).expect("read output csv");
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}
