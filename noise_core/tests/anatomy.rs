//! Pins the anatomy of the real sample set against numbers measured by hand from
//! the files. If the definition of "index found" or "poly-G padding" ever drifts,
//! this is what notices.
//!
//! ```text
//! cargo test --release -p noise_core --test anatomy -- --ignored
//! ```

use std::path::PathBuf;

use noise_core::{anatomise, SampleSheet};

const BARCODE: &str = "CACCAGACACGTCTGAACTCCAGTCAC";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("package dir has a parent")
        .to_path_buf()
}

#[test]
#[ignore = "reads the real sample data; run explicitly with --ignored"]
fn sample1_anatomy_matches_the_hand_measurements() {
    let root = repo_root();
    let sheet_text = std::fs::read_to_string(root.join("docs/sample_sheet.csv"))
        .expect("read the committed sample sheet");
    let sheet = SampleSheet::parse(&sheet_text).expect("parse the sheet");
    let indices = sheet.get("sample1").expect("sample1");

    let text = std::fs::read_to_string(root.join("counted_results/sample1_unbinned_reads"))
        .expect("read sample1");

    let mut reads = 0;
    let mut barcode_at_base_1 = 0;
    let mut barcode_anywhere = 0;
    let mut index1_anywhere = 0;
    let mut index1_at_28 = 0;
    let mut index2_anywhere = 0;
    let mut poly_g = 0;

    for line in text.lines() {
        let Some(anatomy) = anatomise(line, indices, BARCODE) else {
            continue;
        };
        reads += 1;
        if anatomy.barcode == Some(1) {
            barcode_at_base_1 += 1;
        }
        if anatomy.has_barcode() {
            barcode_anywhere += 1;
        }
        if anatomy.index1.is_some() {
            index1_anywhere += 1;
        }
        if anatomy.index1_expected {
            index1_at_28 += 1;
        }
        if anatomy.index2.is_some() {
            index2_anywhere += 1;
        }
        if anatomy.poly_g_start.is_some() {
            poly_g += 1;
        }
    }

    println!(
        "reads={reads} barcode1={barcode_at_base_1} barcode_any={barcode_anywhere} \
         index1={index1_anywhere} at28={index1_at_28} index2={index2_anywhere} polyG={poly_g}"
    );

    // 1,246 lines, but two are entirely N and `clean_sequence` rejects them, so
    // the profile will report 1,244 reads and 2 skipped.
    assert_eq!(reads, 1_244, "usable reads");
    assert_eq!(barcode_at_base_1, 888, "the worked-example count");
    assert!(barcode_anywhere >= barcode_at_base_1);
    assert_eq!(index1_anywhere, 1_129);
    assert_eq!(index1_at_28, 966);
    assert_eq!(index2_anywhere, 0, "index2 sits beyond these reads");
    assert_eq!(poly_g, 1_134, "reads ending in at least eight Gs");
}
