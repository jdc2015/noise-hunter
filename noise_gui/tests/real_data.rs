//! The same regression guard the core crate has, run through the GUI's parallel
//! job path: real sample data in, `baseline.sha256` compared, byte for byte.
//!
//! ```text
//! cargo test --release -p noise_gui -- --ignored
//! ```
//!
//! Ignored because it reads the sample set in the repository root.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Instant;

use sha2::{Digest, Sha256};

use noise_gui::jobs::{JobEvent, JobRunner};
use noise_gui::state::PendingRun;
use noise_core::SearchOptions;

const QUERY: &str = "CACCAGACACGTCTGAACTCCAGTCAC";

fn read_manifest(path: &Path) -> BTreeMap<String, String> {
    let text = fs::read_to_string(path).expect("read baseline.sha256");
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split("  ");
            match (parts.next(), parts.next()) {
                (Some(hash), Some(name)) => Some((name.trim().to_string(), hash.trim().to_string())),
                _ => None,
            }
        })
        .collect()
}

fn sha256_of(path: &Path) -> String {
    let mut file = File::open(path).expect("open output");
    let mut hasher = Sha256::new();
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer).expect("read output");
    hasher.update(&buffer);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

/// Drains the channel until the job reports its final event.
fn wait_for(runner: &mut JobRunner, is_final: impl Fn(&JobEvent) -> bool) -> Vec<JobEvent> {
    let deadline = Instant::now() + std::time::Duration::from_secs(300);
    let mut events = Vec::new();
    loop {
        events.extend(runner.drain());
        if events.iter().any(&is_final) {
            return events;
        }
        assert!(Instant::now() < deadline, "the real-data job never finished");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
#[ignore = "reads the real sample data; run explicitly with --ignored"]
fn gui_parallel_runs_match_the_v0_4_0_baseline() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.parent().expect("package dir has a parent");
    let input = repo_root.join("counted_results");
    let expected = read_manifest(&repo_root.join("baseline.sha256"));
    assert!(!expected.is_empty(), "baseline.sha256 is empty");

    let files = noise_core::list_directory_for_browse(&input)
        .expect("list counted_results")
        .read_files;
    assert_eq!(files.len(), 96);

    let out_root = std::env::temp_dir().join(format!("noise_gui_baseline_{}", std::process::id()));
    let _ = fs::remove_dir_all(&out_root);

    // 1. Count every sample with no filter, through the GUI's rayon path.
    let runner = JobRunner::new();
    runner.start(PendingRun::Count {
        files: files.clone(),
        results_dir: out_root.join("full"),
        filter: None,
    });
    let mut runner_ref = runner;
    wait_for(&mut runner_ref, |event| matches!(event, JobEvent::Counts { .. }));

    // 2. The same with the 30-base filter the README's worked example uses.
    let runner = JobRunner::new();
    runner.start(PendingRun::Count {
        files: files.clone(),
        results_dir: out_root.join("len30"),
        filter: Some(30),
    });
    let mut runner_ref = runner;
    wait_for(&mut runner_ref, |event| matches!(event, JobEvent::Counts { .. }));

    // 3. The worked-example search, this time with both options switched on, so
    //    the extra columns appear and the exact column stays put.
    let runner = JobRunner::new();
    runner.start(PendingRun::Search {
        files: files.clone(),
        sequence: QUERY.to_string(),
        options: SearchOptions {
            reverse_complement: true,
            max_mismatches: 0,
        },
        results_dir: out_root.join("seq_rc"),
    });
    let mut runner_ref = runner;
    let events = wait_for(&mut runner_ref, |event| {
        matches!(event, JobEvent::SearchFinished { .. })
    });
    let exact_total = events
        .iter()
        .find_map(|event| match event {
            JobEvent::SearchFinished { result, .. } => Some(result.total),
            _ => None,
        });
    // Unwrap is about the test, not the program.
    assert_eq!(exact_total.expect("search result"), 74_985);

    // The baseline's exact-search file is compared against the exact columns of
    // this run, since the baseline was captured before the options existed.
    let baseline_seq = expected
        .get("seq/sequencecount.csv")
        .expect("baseline contains the search output");
    let baseline_lines = read_baseline_sequence(&out_root.join("seq_rc/sequencecount.csv"));
    assert!(
        baseline_lines.iter().any(|line| line == baseline_seq),
        "no hash matches — the search output changed shape"
    );

    let mut compared = 0usize;
    for (name, hash) in &expected {
        if name.starts_with("seq/") {
            continue;
        }
        let path = out_root.join(name);
        assert!(path.exists(), "the GUI run did not produce {name}");
        assert_eq!(&sha256_of(&path), hash, "{name} differs from v0.4.0");
        compared += 1;
    }
    assert_eq!(compared, 192, "full/ plus len30/ outputs are both checked");
    let _ = fs::remove_dir_all(&out_root);
}

/// Reads a combined search CSV and returns the sha256 of every variant of it:
/// the whole file, and the file with the option columns removed. The second
/// variant is what a v0.4.0 run wrote, so an options-on run must still match it
/// in its first two columns.
fn read_baseline_sequence(path: &Path) -> Vec<String> {
    let text = fs::read_to_string(path).expect("read sequencecount.csv");
    let plain: String = text
        .lines()
        .map(|line| {
            line.split(',')
                .take(2)
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let hash = |bytes: &str| {
        let mut hasher = Sha256::new();
        hasher.update(bytes.as_bytes());
        hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    vec![hash(&text), hash(&plain)]
}
