//! The per-sample profile against the real sample set.
//!
//! ```text
//! cargo test --release -p noise_core --test profile -- --ignored
//! ```

use std::path::PathBuf;

use noise_core::{profile_files, SampleSheet};

const BARCODE: &str = "CACCAGACACGTCTGAACTCCAGTCAC";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("package dir has a parent")
        .to_path_buf()
}

#[test]
#[ignore = "reads the real sample data; run explicitly with --ignored"]
fn every_sample_profiles_and_sample1_matches_the_hand_measurements() {
    let root = repo_root();
    let sheet = SampleSheet::parse(
        &std::fs::read_to_string(root.join("docs/sample_sheet.csv")).expect("read the sheet"),
    )
    .expect("parse the sheet");
    let files = noise_core::list_directory_for_browse(&root.join("counted_results"))
        .expect("list counted_results")
        .read_files;

    let profiles = profile_files(&files, &sheet, BARCODE).expect("profile every sample");
    assert_eq!(profiles.len(), 96, "one row per sample, in browse order");

    let mut empty: Vec<&str> = Vec::new();
    for profile in &profiles {
        if profile.reads == 0 {
            empty.push(profile.sample_name.as_str());
            assert_eq!(profile.median_effective_len(), 0, "no reads, no median");
            continue;
        }
        assert_eq!(
            profile.reads,
            profile.index1 + profile.index2 - profile.both_indices + profile.neither_index,
            "{}: index classes must partition the reads",
            profile.sample_name
        );
    }
    assert_eq!(
        empty,
        vec!["sample57", "sample71"],
        "these two samples had nothing unbinned, so their files are empty"
    );

    let sample1 = profiles
        .iter()
        .find(|profile| profile.sample_name == "sample1")
        .expect("sample1");

    println!(
        "sample1 reads={} barcode={} index1={} slot={} elsewhere={} index2={} \
         both={} neither={} polyG={} median={}",
        sample1.reads,
        sample1.barcode,
        sample1.index1,
        sample1.index1_slot,
        sample1.index1_elsewhere,
        sample1.index2,
        sample1.both_indices,
        sample1.neither_index,
        sample1.poly_g,
        sample1.median_effective_len(),
    );

    assert_eq!(sample1.reads, 1_244);
    assert_eq!(sample1.barcode, 888);
    assert_eq!(sample1.index1, 1_129);
    assert_eq!(sample1.index1_slot, 966);
    assert_eq!(sample1.index1_elsewhere, 163);
    assert_eq!(sample1.index2, 0);
    assert_eq!(sample1.both_indices, 0);
    assert_eq!(sample1.neither_index, 115);
    assert_eq!(sample1.poly_g, 1_134);
    // Half of sample1's reads carry no more than 65 bases of real sequence before
    // the G padding starts — the padding is not a fringe case, it is the norm.
    assert_eq!(sample1.median_effective_len(), 65);
}
