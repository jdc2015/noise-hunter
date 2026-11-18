//! Drives the GUI's job runner the way the window does, without a window. The
//! point is the comparison: a count or a search that goes through the GUI's
//! parallel path must produce the same bytes as the same job through the core
//! functions the TUI calls.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use noise_gui::jobs::{JobEvent, JobRunner};
use noise_gui::state::{AppState, PendingRun, Tab};

fn temp_dir(tag: &str) -> PathBuf {
    let unique = format!(
        "noise_gui_{tag}_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    );
    let path = std::env::temp_dir().join(unique);
    fs::create_dir_all(&path).expect("create temp dir");
    path
}

/// `samples/` plus its sibling `noise_results/`, matching how the real folders
/// are laid out.
fn fixture(tag: &str) -> (PathBuf, PathBuf) {
    let root = temp_dir(tag).join("samples");
    fs::create_dir_all(&root).expect("create samples");
    // Names chosen so natural order and lexical order disagree.
    let contents: [(&str, &str); 3] = [
        (
            "sample1_unbinned_reads",
            "ACGTACGTACGT\nACGTACGTACGT\nGGGGGGGGGGGG\n",
        ),
        (
            "sample10_unbinned_reads",
            "ACGTACGTACGT\nTTTTTTTTTTTT\nTTTTTTTTTTTT\nTTTTTTTTTTTT\n",
        ),
        (
            "sample2_unbinned_reads",
            "ACGTACGTACGT\nACGTACGTACGT\nACGTACGTACGT\nNs\n",
        ),
    ];
    for (name, body) in contents {
        fs::write(root.join(name), body).expect("write sample");
    }
    let results = root.parent().expect("parent").join("noise_results");
    (root, results)
}

fn sample_files(root: &Path) -> Vec<PathBuf> {
    noise_core::list_directory_for_browse(root)
        .expect("listing")
        .read_files
}

/// Waits for the job to finish, collecting every event it posts.
fn collect(runner: &mut JobRunner, until: impl Fn(&[JobEvent]) -> bool) -> Vec<JobEvent> {
    let deadline = Instant::now() + std::time::Duration::from_secs(30);
    let mut events = Vec::new();
    loop {
        events.extend(runner.drain());
        if until(&events) {
            return events;
        }
        assert!(Instant::now() < deadline, "job never finished");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn counted_files_arrive_in_folder_order() {
    let (root, results) = fixture("order");
    let files = sample_files(&root);
    assert_eq!(
        files
            .iter()
            .map(|p| noise_core::sample_name_from_path(p))
            .collect::<Vec<_>>(),
        vec!["sample1", "sample2", "sample10"],
        "the listing must be in natural order, not lexical"
    );

    let runner = JobRunner::new();
    runner.start(PendingRun::Count {
        files: files.clone(),
        results_dir: results.clone(),
        filter: None,
    });
    let mut runner_ref = runner;
    let events = collect(&mut runner_ref, |events| {
        events
            .iter()
            .any(|event| matches!(event, JobEvent::Counts { .. }))
    });

    let counts = events
        .iter()
        .find_map(|event| match event {
            JobEvent::Counts { results, .. } => Some(results.clone()),
            _ => None,
        })
        .expect("counts event");
    // rayon finishes in arbitrary order; the GUI must put the rows back.
    assert_eq!(
        counts
            .iter()
            .map(|result| result.sample_name.as_str())
            .collect::<Vec<_>>(),
        vec!["sample1", "sample2", "sample10"]
    );

    let totals = events.iter().find_map(|event| match event {
        JobEvent::Counts { totals, .. } => Some(totals.clone()),
        _ => None,
    });
    // Unwrap is about the test, not the program: the event exists by now.
    let totals = totals.expect("totals");
    assert_eq!(totals.file_count, 3);
    assert_eq!(totals.total_lines, 11);
    assert_eq!(totals.total_used, 10);
    assert_eq!(totals.total_skipped, 1, "the bare 'Ns' line is the only skip");
    fs::remove_dir_all(root.parent().expect("parent")).ok();
}

#[test]
fn gui_count_matches_core_count_byte_for_byte() {
    let (gui_root, gui_results) = fixture("gui_vs_core_gui");
    let (core_root, core_results) = fixture("gui_vs_core_core");
    fs::create_dir_all(&core_results).expect("create core results");

    let gui_files = sample_files(&gui_root);
    let core_files = sample_files(&core_root);

    let runner = JobRunner::new();
    runner.start(PendingRun::Count {
        files: gui_files.clone(),
        results_dir: gui_results.clone(),
        filter: None,
    });
    let mut runner_ref = runner;
    collect(&mut runner_ref, |events| {
        events
            .iter()
            .any(|event| matches!(event, JobEvent::Counts { .. }))
    });

    for path in &core_files {
        noise_core::count_file(path, &core_results, None).expect("core count");
    }

    for name in [
        "noisecounts_sample1.csv",
        "noisecounts_sample2.csv",
        "noisecounts_sample10.csv",
    ] {
        let gui = fs::read(gui_results.join(name)).expect("gui output");
        let core = fs::read(core_results.join(name)).expect("core output");
        assert_eq!(gui, core, "{name} differs between the GUI path and the TUI path");
    }
    for base in [gui_root, core_root] {
        fs::remove_dir_all(base.parent().expect("parent")).ok();
    }
}

#[test]
fn live_counts_write_nothing_and_agree_with_a_tolerant_core_search() {
    let (root, results) = fixture("live");
    let files = sample_files(&root);
    let runner = JobRunner::new();

    // A query that matches nothing exactly but sits two mismatches from `GGGG…`.
    let query = "GCGTACGTACGT";
    let options = noise_core::SearchOptions {
        reverse_complement: false,
        max_mismatches: 2,
    };
    runner.start_live_counts(files.clone(), query.to_string(), options, results.clone());
    let mut runner_ref = runner;
    let events = collect(&mut runner_ref, |events| {
        events
            .iter()
            .any(|event| matches!(event, JobEvent::LiveCounts { .. }))
    });
    let live = events.iter().find_map(|event| match event {
        JobEvent::LiveCounts(update) => Some(update.result.clone()),
        _ => None,
    });
    // Unwrap is about the test: the event matched above.
    let live = live.expect("live counts");
    assert_eq!(live.total, 0, "nothing matches exactly");
    assert_eq!(live.hits.len(), 3);
    assert!(
        !results.join("sequencecount.csv").exists(),
        "live counts must never write a CSV the user did not ask for"
    );

    let core = noise_core::count_sequence_across_with(&files, query, &results, &options)
        .expect("core search");
    assert_eq!(
        live.within_tolerance_total, core.within_tolerance_total,
        "the tolerant counts must agree with core"
    );
    assert_eq!(
        live.hits
            .iter()
            .map(|hit| (hit.sample_name.clone(), hit.best_distance))
            .collect::<Vec<_>>(),
        core.hits
            .iter()
            .map(|hit| (hit.sample_name.clone(), hit.best_distance))
            .collect::<Vec<_>>()
    );
    fs::remove_dir_all(root.parent().expect("parent")).ok();
}

#[test]
fn a_run_that_would_replace_output_is_reported_first() {
    let (root, results) = fixture("overwrite");
    let files = sample_files(&root);
    let mut state = AppState::new();
    state.input_folder = root.clone();
    state.open_folder(&root, false);
    assert_eq!(state.active_tab, Tab::BrowseCount);
    assert_eq!(state.selected.len(), 3, "opening a folder selects everything");

    let run = PendingRun::Count {
        files: files.clone(),
        results_dir: results.clone(),
        filter: None,
    };
    assert!(
        state.outputs_to_overwrite(&run).is_empty(),
        "a fresh results folder needs no confirmation"
    );

    noise_core::count_file(&files[0], &results, None).expect("pre-existing output");
    let guard = state.outputs_to_overwrite(&run);
    assert_eq!(guard.len(), 1, "only the sample that exists is listed");
    assert_eq!(
        guard[0].file_name().and_then(|name| name.to_str()),
        Some("noisecounts_sample1.csv")
    );

    let search = PendingRun::Search {
        files: files.clone(),
        sequence: "ACGTACGTACGT".to_string(),
        options: noise_core::SearchOptions::default(),
        results_dir: results.clone(),
    };
    assert!(
        state.outputs_to_overwrite(&search).is_empty(),
        "no search has run yet, so sequencecount.csv is safe"
    );
    noise_core::count_sequence_across(&files, "ACGTACGTACGT", &results).expect("core search");
    assert_eq!(state.outputs_to_overwrite(&search).len(), 1);

    // "Select all" honours the filter box, so a narrow filter narrows the run,
    // while an ordinary selection is never dropped by typing in the filter.
    state.filter_text = "sample2".to_string();
    assert_eq!(state.selected_files().len(), 3, "the filter never unselects");
    state.select_none();
    state.select_all_visible();
    assert_eq!(
        state.selected_files().len(),
        1,
        "with a filter set, Select all takes the filtered rows"
    );
    state.filter_text.clear();
    state.select_all_visible();
    let selected: HashSet<String> = state
        .selected_files()
        .iter()
        .map(|p| noise_core::sample_name_from_path(p))
        .collect();
    assert_eq!(selected.len(), 3);
    fs::remove_dir_all(root.parent().expect("parent")).ok();
}

#[test]
fn exports_reuse_the_core_sequence_table_format() {
    let (root, results) = fixture("export");
    let files = sample_files(&root);
    let options = noise_core::SearchOptions {
        reverse_complement: true,
        max_mismatches: 0,
    };
    let result = noise_core::count_sequence_across_with(&files, "ACGTACGTACGT", &results, &options)
        .expect("core search");

    let path = results.join("exported.csv");
    noise_gui::export::write_sequence_table(&path, &result).expect("export");
    let exported = fs::read_to_string(&path).expect("read export");
    let run = fs::read_to_string(&result.output_path).expect("read run output");
    assert_eq!(exported, run, "an export must look exactly like a run's CSV");
    assert!(
        exported.starts_with("sample,ACGTACGTACGT,ACGTACGTACGT_rc,combined\n"),
        "the reverse-complement columns must be there: {exported}"
    );
    fs::remove_dir_all(root.parent().expect("parent")).ok();
}
