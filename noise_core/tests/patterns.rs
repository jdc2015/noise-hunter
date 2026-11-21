//! What the shared-pattern search says about the real sample set.
//!
//! ```text
//! cargo test --release -p noise_core --test patterns -- --ignored
//! ```

use std::path::PathBuf;
use std::time::Instant;

use noise_core::shared_kmers;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("package dir has a parent")
        .to_path_buf()
}

#[test]
#[ignore = "reads the real sample data; run explicitly with --ignored"]
fn a_long_shared_kmer_is_rare_and_a_short_one_is_not() {
    let files = noise_core::list_directory_for_browse(&repo_root().join("counted_results"))
        .expect("list counted_results")
        .read_files;
    assert_eq!(files.len(), 96);

    let started = Instant::now();
    let long = shared_kmers(&files, 20).expect("k = 20");
    let long_elapsed = started.elapsed();

    let started = Instant::now();
    let short = shared_kmers(&files, 12).expect("k = 12");
    let short_elapsed = started.elapsed();

    println!(
        "compared={} k=20: {} patterns in {long_elapsed:?}",
        long.samples_compared,
        long.patterns.len()
    );
    println!("k=12: {} patterns in {short_elapsed:?}", short.patterns.len());
    for pattern in short.patterns.iter().take(3) {
        println!(
            "  {} total={} present={} expected={:.3} o/e={:?}",
            pattern.kmer,
            pattern.total(),
            pattern.samples_present(),
            pattern.expected,
            pattern.observed_over_expected(),
        );
    }
    for pattern in long.patterns.iter().take(5) {
        println!(
            "  k20 {} total={} o/e={:.1}",
            pattern.kmer,
            pattern.total(),
            pattern.observed_over_expected().unwrap_or(f64::NAN)
        );
    }

    // Two samples (57 and 71) have empty unbinned files; they cannot hold a
    // pattern and are not required to.
    assert_eq!(long.samples_compared, 94, "two samples have no reads");

    // The result is the finding: at k=20 the only things every sample shares are
    // fragments of the constant allele-locus barcode and the boundary into the
    // poly-G padding. No other 20-mer is universal, which is the answer to
    // "is there a noise pattern in every sample" at this length.
    const BARCODE: &str = "CACCAGACACGTCTGAACTCCAGTCAC";
    // A padding k-mer may have a construct base or two inside the run, so "G-rich"
    // is the honest test rather than "eight Gs in a row".
    let explained = |kmer: &str| {
        BARCODE.contains(kmer) || kmer.bytes().filter(|base| *base == b'G').count() >= 8
    };
    assert!(
        long.patterns.iter().all(|pattern| explained(&pattern.kmer)),
        "an unexplained 20-mer is shared by every sample: {:?}",
        long.patterns
            .iter()
            .map(|pattern| &pattern.kmer)
            .filter(|kmer| !explained(kmer))
            .collect::<Vec<_>>()
    );
    assert!(
        long.patterns
            .iter()
            .any(|pattern| BARCODE.contains(&pattern.kmer)),
        "the barcode is the most obvious shared sequence and must be found"
    );

    // At k=12 more is shared, and the same explanation should hold: the barcode
    // and the padding, nothing else.
    assert!(!short.patterns.is_empty(), "12-mers shared across samples should exist");
    assert!(
        short
            .patterns
            .iter()
            .all(|pattern| explained(&pattern.kmer)),
        "an unexplained 12-mer is shared by every sample: {:?}",
        short
            .patterns
            .iter()
            .map(|pattern| &pattern.kmer)
            .filter(|kmer| !explained(kmer))
            .collect::<Vec<_>>()
    );
    assert!(
        short
            .patterns
            .iter()
            .all(|pattern| pattern.samples_present() == 94),
        "every returned pattern is in every sample that has reads"
    );
    assert!(
        short_elapsed.as_secs() < 60,
        "the whole set should take seconds, took {short_elapsed:?}"
    );
}
