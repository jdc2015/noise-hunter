//! Headless driver for the two jobs the TUI performs interactively, so their
//! output can be hashed and compared across refactors. Not part of the app.
//!
//! ```text
//! cargo run --release --example baseline
//! ```
//!
//! Writes CSVs to `target/baseline/` — it never touches the real
//! `noise_results/` — and prints one `<sha256>  <relative/path>` line per file
//! to stdout, which is what `baseline.sha256` holds.
//!
//! ```text
//! cargo run --release -p noise_core --example baseline
//! ```

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use noise_core::count::{count_file, count_sequence_across};
use noise_core::scan::list_directory_for_browse;

/// The README's worked example: a 29-base query over all 96 samples, total 74985.
const QUERY: &str = "CACCAGACACGTCTGAACTCCAGTCAC";
/// The README's other worked example: `sample1` filtered to its first 30 bases.
const LENGTH_FILTER: usize = 30;
const SPOTLIGHT: &str = "sample1";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .parent()
        .ok_or("package dir has no parent")?
        .to_path_buf();

    let mut args = std::env::args().skip(1);
    let input = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root.join("counted_results"));
    let out_root = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root.join("target/baseline"));

    if out_root.exists() {
        fs::remove_dir_all(&out_root)?;
    }

    let listing = list_directory_for_browse(&input)?;
    let files = &listing.read_files;
    println!("# {} read files under {}", files.len(), input.display());

    // Job 1: duplicate counts, at full length and with a length filter.
    let full_dir = out_root.join("full");
    let filtered_dir = out_root.join(format!("len{LENGTH_FILTER}"));
    for path in files {
        count_file(path, &full_dir, None)?;
    }
    for path in files {
        let result = count_file(path, &filtered_dir, Some(LENGTH_FILTER))?;
        if result.sample_name == SPOTLIGHT {
            println!(
                "# {SPOTLIGHT} @ {LENGTH_FILTER} bases: read {} used {} skipped {} unique {}",
                result.lines_read,
                result.sequences_used,
                result.lines_skipped,
                result.unique_sequences
            );
        }
    }

    // Job 2: one specific sequence across every sample.
    let query = count_sequence_across(files, QUERY, &out_root.join("seq"))?;
    println!("# query {QUERY}: total {} hits across {}", query.total, query.hits.len());

    // Manifest: stable ordering, paths relative to the output root.
    let mut csvs = Vec::new();
    collect_csvs(&out_root, &out_root, &mut csvs)?;
    csvs.sort();
    for relative in &csvs {
        println!(
            "{}  {}",
            sha256_of(&out_root.join(relative))?,
            relative.to_string_lossy()
        );
    }
    Ok(())
}

fn collect_csvs(root: &Path, dir: &Path, found: &mut Vec<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_csvs(root, &path, found)?;
        } else if path.extension().map(|e| e == "csv").unwrap_or(false) {
            found.push(path.strip_prefix(root)?.to_path_buf());
        }
    }
    Ok(())
}

fn sha256_of(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>())
}
