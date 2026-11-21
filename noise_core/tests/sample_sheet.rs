//! The sample sheet is committed, so this guard always runs: 96 samples, two
//! 8-base indices each, and every pair listed again in reverse for its partner.
//!
//! The mirroring is a property of the supplied sheet, not of the parser, which is
//! why it is asserted here rather than built into [`SampleSheet`].

use std::path::PathBuf;

use noise_core::SampleSheet;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("package dir has a parent")
        .to_path_buf()
}

#[test]
fn the_committed_sheet_has_96_samples_and_mirrored_pairs() {
    let path = repo_root().join("docs/sample_sheet.csv");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    let sheet = SampleSheet::parse(&text).expect("parse the committed sheet");

    assert_eq!(sheet.len(), 96, "the supplied sheet has one row per sample");

    let first = sheet.get("sample1").expect("sample1 is in the sheet");
    assert_eq!(first.index1, "ATATGCGC");
    assert_eq!(first.index2, "CTGATCGT");

    for number in 1..=96 {
        let name = format!("sample{number}");
        let partner = format!("sample{}", 97 - number);
        let left = sheet.get(&name).unwrap_or_else(|| panic!("{name} missing"));
        let right = sheet
            .get(&partner)
            .unwrap_or_else(|| panic!("{partner} missing"));

        // The same two indices, in the opposite order, name the partner sample.
        assert_eq!(left.index1, right.index2, "{name} and {partner} disagree");
        assert_eq!(left.index2, right.index1, "{name} and {partner} disagree");
        assert_eq!(left.index1.len(), 8, "{name} index1");
        assert_eq!(left.index2.len(), 8, "{name} index2");
    }
}
