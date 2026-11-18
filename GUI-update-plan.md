# Noise Hunter — separate GUI build plan

Date: 2026-11-18
Scope: add a mouse-driven desktop GUI alongside the existing terminal UI, sharing one copy of the counting logic.

## Decisions taken

| Question | Decision |
|---|---|
| Toolkit | **egui / eframe** — pure Rust, single binary, no Node/webview, good Wayland support |
| Relationship to TUI | **Cargo workspace + shared `noise_core` crate**; TUI keeps working |
| GUI extras | Sortable/filterable tables, charts, parallel scanning + progress, reverse-complement search, mismatch-tolerant search |
| Output | **Identical CSVs, written to the sibling `noise_results/`** exactly as today |
| Packaging | `cargo build/run` only — no launcher, icon, or bundle |
| New search features | Live in `noise_core`; **extra CSV columns only when the mode is enabled** |
| Window layout | Sidebar (folder + sample list) + top tabs (Count / Sequence search / Results) |
| Length filter UX | **Live recompute** as you type (no Apply button) |
| Core extraction | **Move verbatim, keep existing names** — mechanical, low-risk |

## 1. Where things stand

`noise_counter/` is a Rust TUI (ratatui 0.30.2 + crossterm 0.29.0), 2,624 lines, edition 2021:

| File | Lines | Role |
|---|---|---|
| `src/main.rs` | 10 | entry → `tui::run()` |
| `src/count.rs` | 713 | all counting logic: `clean_sequence`, `examine_file`, `count_file`, `count_sequence_across`, `ClosestMatch`, CSV writing, 14 tests |
| `src/scan.rs` | 224 | file discovery, natural sort, `results_dir`/`output_path`, 6 tests |
| `src/tui.rs` | 1,677 | 7 screens (`Welcome`, `FolderBrowse`, `Examine`, `FileResult`, `AllComplete`, `SequenceQuery`, `SequenceResult`) |

Key structural facts that shape this plan:

- **It is a binary crate, not a library.** `count.rs`/`scan.rs` are `mod`-level and reachable only as `crate::count::…`. The GUI cannot reuse them without an extraction step — that is Phase 1.
- **The core is already almost UI-free.** `count_file` and `scan_sequence_in_file` take a `&Path` and return plain structs. Nothing in `count.rs`/`scan.rs` imports crossterm or ratatui. The extraction is genuinely mechanical.
- **No headless mode today** (no args, no stdin), which makes "did my refactor change the numbers?" hard to answer. Phase 0 fixes that with a throwaway driver binary, which then becomes the regression harness.
- **No git repo.** `git init` first — the core refactor touches every file.

Environment, verified on this machine:

- rustc/cargo **1.99.0**; Node 26 and GTK4 4.24 / Qt6 6.12 also present (not needed by egui).
- Desktop: **Hyprland / Wayland**. GPU: Intel Alder Lake-N UHD, **Vulkan ANV ICD installed** (`intel_icd.json`), so eframe's default `wgpu` renderer should work.
- `xdg-desktop-portal-hyprland` is installed, so `rfd`'s default `xdg-portal` file dialog works.
- Data: `counted_results/` is **18 MB, 96 samples**, ~1.2k lines per file (largest 17 KB). Correctness of output matters more than throughput; parallelism is for a responsive UI, not survival.
- `noise_counter/target/` is 139 MB of build artifacts and `noise_results/PP00005_noise_CACCAGACACGT_sequencecount.csv` is a hand-named artifact worth keeping — both are reasons to set up `.gitignore` before moving anything.

## 2. Target layout

```
noise-hunter/
├── Cargo.toml                 # NEW: [workspace] members = noise_core, noise_counter, noise_gui
├── .gitignore                 # NEW: /target, noise_results/
├── baseline.sha256            # NEW (Phase 0): golden checksums of real-data output
├── noise_core/                # NEW: std-only library — the single source of truth
│   ├── Cargo.toml             # no dependencies
│   ├── src/lib.rs             # pub mod count; pub mod scan; re-exports
│   ├── src/count.rs           # moved verbatim from noise_counter/src/count.rs
│   ├── src/scan.rs            # moved verbatim from noise_counter/src/scan.rs
│   └── tests/golden.rs        # #[ignore] real-data regression test vs baseline.sha256
├── noise_counter/             # TUI: directory and package name UNCHANGED
│   ├── Cargo.toml             # + noise_core = { path = "../noise_core" }
│   ├── src/main.rs            # drop `mod count; mod scan;`
│   ├── src/tui.rs             # use-path edits only: crate::count:: → noise_core::count::
│   └── examples/baseline.rs   # headless driver (Phase 0), later the golden generator
└── noise_gui/                 # NEW
    ├── Cargo.toml             # eframe, egui, egui_extras, egui_plot, rfd, rayon
    └── src/
        ├── main.rs            # eframe::run_native, window options, renderer fallback
        ├── app.rs             # impl eframe::App — tab bar, sidebar, dispatch
        ├── state.rs           # AppState: forms, selections, sort/filter state, results
        ├── jobs.rs            # worker thread + channel, JobState, progress
        ├── search_form.rs     # scope + RC + mismatch options
        └── ui/
            ├── browse.rs      # folder picker, sample list w/ checkboxes, select-all
            ├── count_tab.rs   # live length filter, top-10 table, preview, progress
            ├── search_tab.rs  # query input, options, results table, closest-match cell
            ├── results_tab.rs # cross-sample table + totals
            └── charts.rs      # egui_plot bar charts and histograms
```

`noise_counter/target/` disappears once the workspace root owns `target/`; run `cargo clean` after the move to reclaim the 139 MB.

## 3. The output contract (non-negotiable)

Everything below is a rule the implementation must satisfy, not a suggestion. It is what makes the TUI and GUI interchangeable.

1. Duplicate counts: one CSV per sample at `<input>/../noise_results/noisecounts_<sample>.csv`, header `sequence,counts,length`, count descending then alphabetical. **Unchanged, in both tools.**
2. Sequence search: one combined `noise_results/sequencecount.csv`, and **for default options it must be byte-identical to what the TUI writes today** — header `sample,<SEQUENCE>`, one row per sample in browse order, `total` row.
3. Extra columns appear **only** when a new mode is switched on:
   - reverse complement on → `sample,<SEQ>,<SEQ>_rc,combined`
   - mismatch tolerance `d > 0` → `sample,<SEQ>,within_<d>,best_mismatches`
   - both on → union of those columns.
4. Output is always the sibling `noise_results/` — no folder picker for output. Filenames stay as they are (no timestamps, no filter suffixes), so **any run that would overwrite an existing CSV shows a confirm dialog first**. This matters more in the GUI than the TUI because an RC run would otherwise silently destroy an exact-only result with a different header.
5. A read counts at most once per sample even with multiple matches, and RC counts are combined into one read-level total — same semantics as today's containment rule.

## 4. Phases

### Phase 0 — Safety net (≈15 min)

1. `git init`, add `.gitignore` (`/target`, `noise_results/`), commit the current tree as-is so the working TUI is recoverable.
2. Back up `noise_results/PP00005_noise_CACCAGACACGT_sequencecount.csv` somewhere safe (it is the only surviving copy of an old query).
3. Write `noise_counter/examples/baseline.rs`: for every `*_unbinned_reads` in `counted_results/`, call `examine_file` / `count_file` (with and without a length filter of 30) and `count_sequence_across` (with the 29-base query from the README), writing CSVs to a temp output dir. This is the only headless path to the numbers.
4. `cargo run --release --example baseline`, then `sha256sum` every generated CSV into `baseline.sha256`.

**Acceptance:** `cargo test --release` → 20 tests pass; `baseline.sha256` committed; `cargo run` still opens the TUI.

### Phase 1 — Extract `noise_core` (≈1–2 h, no logic changes)

1. Add the workspace `Cargo.toml` at the repo root with `members = ["noise_core", "noise_counter", "noise_gui"]` and `resolver = "2"`.
2. `git mv noise_counter/src/{count,scan}.rs noise_core/src/`; add `noise_core/src/lib.rs` exposing `pub mod count; pub mod scan;` plus flat re-exports. `count.rs`'s internal `use crate::scan::{…}` needs **no change** — it stays crate-relative inside the new crate.
3. `noise_core/Cargo.toml`: package `noise_core`, edition 2021, **zero dependencies** (keep it std-only so the GUI can own its own deps).
4. In `noise_counter/Cargo.toml`, add `noise_core = { path = "../noise_core" }`; delete the two `mod` lines from `main.rs`; in `tui.rs` replace `crate::count::` → `noise_core::count::` and `crate::scan::` → `noise_core::scan::` (a handful of lines around tui.rs:14-19).
5. Move the test modules with their files; they already build their own temp folders, so they need no edits.
6. If `tui.rs` fails to compile on an item that was `pub(crate)`-visible-by-crate, **widen it in `noise_core`** rather than duplicating logic.
7. Add `noise_core/tests/golden.rs` marked `#[ignore]`, running the Phase 0 driver logic and asserting the sha256s match `baseline.sha256`.

**Acceptance:** `cargo test --release --workspace` → 20 tests pass; `cargo run -p noise_counter` behaves identically; `cargo test --release -p noise_core -- --ignored` → golden hashes match. **Everything after this point is additive.**

### Phase 2 — Extend core: reverse complement, mismatch tolerance, aggregation (≈2–3 h, TUI untouched)

All additions, with `Default` reproducing today's behaviour, so no TUI call site changes.

1. `pub fn reverse_complement(seq: &str) -> String` — A↔T, C↔G. Input is already ACGT-clean by `clean_sequence`.
2. `pub struct SearchOptions { pub reverse_complement: bool, pub max_mismatches: usize }` with `Default` = `{ false, 0 }` (= exact containment).
3. Keep `count_sequence_across(...)` as the existing signature, delegating to a new `count_sequence_across_with(files, query, opts)`. Same for `scan_sequence_in_file`.
4. Mismatch matching: slide a `query.len()`-wide window along each cleaned read and take the minimum Hamming distance; a read counts once if any window is within `max_mismatches`. Generalise the existing `closest_window` helper (currently equal-length windows only, computed only for zero-hit samples) to sliding windows, and make "report best distance even when there are hits" option-driven.
5. Split write/aggregate so the GUI can parallelise per-file work itself: make `write_sequence_counts` public as `write_sequence_result(&SequenceQueryResult)`, and add `aggregate_hits(folder, hits, query, opts) -> SequenceQueryResult`. `SequenceQueryResult` gains option-derived fields (`rc_hits`, `best_mismatches_per_sample`) that stay empty by default.
6. Implement the header rules from §3 inside `write_sequence_result`, and add tests: default header string is **exactly** `sample,<SEQ>`; RC-on header has 4 columns; `d>0` adds `within_d`/`best_mismatches`; a read containing both query and RC is counted once in `combined`.
7. Cost guard: mismatch mode is O(lines × query_len) per file — document that it is ~10⁷ char comparisons over the whole 96-sample set (fine), and never recompute it on every keystroke in the GUI.

**Acceptance:** `cargo test --release --workspace` green with the new RC/mismatch tests; golden test still matches `baseline.sha256` (proves the default path is untouched); TUI runs unchanged.

### Phase 3 — GUI skeleton (≈2–3 h)

1. `noise_gui/Cargo.toml`: `eframe`, `egui`, `egui_extras`, `egui_plot`, `rfd`, `rayon` (versions in §5).
2. `main.rs`: `eframe::run_native` with a 1200×800 min-size window, title `Noise Hunter`, `visible_on_screen_correction`, and a documented `--renderer glow` escape hatch (see §7).
3. `AppState` (in `state.rs`) — plain data, no widgets in it:

```rust
pub struct AppState {
    input_folder: PathBuf,
    listing: Option<DirectoryListing>,        // from noise_core::scan
    selected: Vec<PathBuf>,                   // keeps natural browse order
    length_filter: String,                    // live-editable
    examine: Option<ExamineResult>,
    job: JobState,                            // Idle | Running { done, total } | Done | Failed(String)
    counts: Vec<CountResult>,
    search: SearchOptions + scope + query,    // search_form.rs
    search_result: Option<SequenceQueryResult>,
    sort: SortState,                          // column + direction, per table
    filter_text: String,                      // live table filter
    notices: Vec<Notice>,                     // errors/warnings shown in-UI, never eprintln
}
```

4. `jobs.rs` — the one rule that decides whether the GUI feels good: **never count on the UI thread**. Sketch:

```rust
// worker thread owns the CPU work; UI thread only reads
let (tx, rx) = std::sync::mpsc::channel::<JobEvent>();
std::thread::spawn(move || {
    let hits = files.par_iter().map(|f| scan_sequence_in_file(f, &q, &opts)).collect();
    let _ = tx.send(JobEvent::SearchDone(hits));
});
// in impl eframe::App::update: drain rx, fold into AppState, then ctx.request_repaint()
```

Progress = an `AtomicUsize` bumped per completed file, plus `ctx.request_repaint()` so egui repaints without a timer.
5. Shell layout: left sidebar (folder button via `rfd`, native sample list with checkboxes + "select all N in folder", selection count, output path label, **Run** button); top tab strip `Count | Sequence search | Results`; bottom status strip with the progress bar and notices.
6. Theme: dark by default, one toggle to light; sequences always rendered in monospace (`egui`'s built-in monospace is fine — no font packaging needed).

**Acceptance:** `cargo run -p noise_gui` opens a window on Hyprland; you can pick `counted_results`, see all 96 samples listed in natural numeric order, tick them, and the Run button drives a progress bar that stays smooth (window is draggable while counting).

### Phase 4 — Count tab (≈2–3 h)

1. Length filter as a live text field: recompute `examine_file`/`count_file` on change (one file ≈1.2k lines ≈ microseconds, so no debounce machinery is needed — cap it by only recomputing on actual change). Reuse `parse_length_filter` so an empty field means full length and `0` still errors.
2. Top-10 table with `egui_extras::TableBuilder`, headers clickable to sort by count / length / sequence. Drop the TUI's ASCII ruler; the length column plus a chart covers it.
3. Run job: `rayon` over the selected files calling `count_file` (each writes its own CSV and returns `preview_lines`, so parallel calls are safe); aggregate with the existing `compute_run_totals`.
4. Per-file results table (read / used / skipped / unique / output path) with live text filter; expandable row showing the first 20 CSV preview lines.
5. Totals card from `RunTotals`.
6. Confirm-before-overwrite dialog per §3 rule 4.

**Acceptance:** selecting all 96 samples and running produces the same 96 CSVs as the TUI (verify with `sha256sum -c baseline.sha256`); the golden test still passes.

### Phase 5 — Sequence-search tab (≈3–4 h)

1. Query field with live validation via `parse_target_sequence`, showing the same class of message the TUI shows for non-ACGT input.
2. Scope control: highlighted file / every file in folder / current selection (replaces the TUI's `Tab` toggle).
3. Options row: **reverse complement** checkbox, **mismatch tolerance** slider (`0`–`3`, `0` = exact) — and switching either on surfaces an inline hint naming the extra CSV columns that will be written.
4. Results table: `sample`, `exact`, (`_rc`, `combined`), (`within_d`, `best_mismatches`), `nearest window` — sortable by any column, live-filterable, virtualised by `TableBuilder` so 96+ rows cost nothing.
5. Nearest-window cell reuses `ClosestMatch` and paints mismatched bases in red, as the TUI does; `total` row pinned at the bottom.
6. Mismatch runs require an explicit **Run** (explicit, not live) and show elapsed time, so a slider drag can't trigger nine full re-scans.

**Acceptance:** the README's 29-base query over all 96 samples yields `total = 74985` and a `sequencecount.csv` byte-identical to the TUI's; enabling RC adds the two columns without changing the `exact` numbers.

### Phase 6 — Charts (≈1–2 h)

1. `ui/charts.rs` using `egui_plot`: bar chart of top-N counts for the current sample; bar chart of hits per sample for the current query (this is the view that genuinely cannot work in a terminal, and it makes the 96-sample answer readable at a glance); histogram of the duplicate-count distribution.
2. Tooltips on bars showing exact sequence + count; clicking a bar scrolls the linked table to that row.
3. Keep chart data derived from `AppState`, never stored separately — no second source of truth.

**Acceptance:** charts update when the length filter, selection, or query changes; nothing in the chart code re-reads files.

### Phase 7 — Polish and docs (≈1–2 h)

1. Shortcuts: `Ctrl+O` choose folder, `Ctrl+R` run, `Ctrl+F` focus table filter, `Esc` clear selection.
2. Errors as in-UI notices (`Notice` list with severity), never `eprintln!` into a void — a GUI launched from a launcher has no console.
3. README: split **TUI** and **GUI** sections, document the new options and the extended `sequencecount.csv` columns, restate that default output is identical from either tool.
4. Versioning: the TUI keeps `APP_VERSION` in `src/tui.rs:22` in sync with `Cargo.toml`. For the workspace, put `version.workspace = true` in `[workspace.package]` and have all three crates inherit it — one bump, no drift. Target **0.5.0** (core gains features), and bump `tui.rs`'s constant to match.
5. Final verification sweep (below).

## 5. Dependency versions (checked against crates.io today)

| Crate | Version | Notes |
|---|---|---|
| `egui` | 0.37.0 | latest stable; **pin exact**, egui breaks APIs between minors |
| `eframe` | 0.37.0 | default features include `wayland`, `wgpu`, `accesskit` — all fine here |
| `egui_extras` | 0.37.0 | `TableBuilder` for sortable/scrollable tables |
| `egui_plot` | 0.38.0 | verified to require `egui ^0.37` — compatible with the trio above |
| `rfd` | 0.18.0 | default `xdg-portal` backend; `xdg-desktop-portal-hyprland` is installed |
| `rayon` | 1.12.0 | parallel per-file scanning |
| `noise_core` | path dep | std-only, no external deps |

Do **not** add `thiserror` in this pass — the core keeps returning `String` errors, per the "move verbatim" decision. Revisit if the error surface grows.

## 6. Verification

```bash
cargo build --release --workspace
cargo test --release --workspace            # 20 existing + new core tests
cargo test --release -p noise_core -- --ignored   # golden hashes vs baseline.sha256
cargo run --release -p noise_counter        # TUI still works, unchanged behaviour
cargo run --release -p noise_gui            # GUI
```

GUI manual checklist: pick folder → list shows 96 samples in `sample2 < sample10` order → select all → run all → 96 CSVs match `sha256sum -c baseline.sha256` → length filter 30 on `sample1` reproduces 1,246 read / 2 skipped / 1,244 counted / 181 unique → 29-base query gives `total = 74985` → RC mode leaves `exact` unchanged → overwriting a CSV prompts → dragging the window during a run stays smooth.

## 7. Risks

| Risk | Mitigation |
|---|---|
| egui minor-version API churn breaks the build mid-project | Pin 0.37.0 exactly; commit a compiling skeleton in Phase 3 before writing features |
| `wgpu` renderer fails on the Intel UHD/ANV stack | `eframe` supports `--renderer glow` (GL fallback) and `WINIT_UNIX_BACKEND=x11`; both are one-line escapes, decided in `main.rs` |
| Portal file dialog misbehaves under Hyprland | `rfd`'s xdg-portal is already the right backend; fall back to an editable path field driven by `list_directory_for_browse` (the TUI's own browser, reused as a widget) |
| Counting on the UI thread freezes the window | Hard rule in Phase 3: all counting in a worker thread + channel; review any code that calls `count_file` from `update()` |
| Live length-filter recompute misused for mismatch search | Explicit **Run** required whenever `max_mismatches > 0` |
| Silent CSV overwrite destroys a different-header result | Confirm dialog (§3 rule 4); this is the main behavioural risk of keeping fixed filenames |
| Refactor silently changes the numbers | `baseline.sha256` + `#[ignore]` golden test, established **before** the move |
| `noise_counter/target/` (139 MB) and the hand-named `PP00005_*.csv` lost in the shuffle | `.gitignore` + explicit backup step in Phase 0 |

## 8. Explicitly out of scope

- CLI flags / headless mode as a shipping feature (the Phase 0 driver is an internal test tool, in `examples/`)
- Anything beyond the two existing jobs: no alignment, no FASTQ quality handling, no UMI dedup, no plots of read length distributions
- Installing an app-menu entry, icon, `.desktop` file, or AppImage
- Changing where output goes (still the sibling `noise_results/`) or renaming output files
- Reworking core error types away from `String`
- Retiring or restyling the TUI beyond the Phase 1 use-path edits

## 9. Still open (deliberately deferred)

1. Should filtered/RC runs eventually get suffixed filenames (`noisecounts_sample1_len30.csv`)? Deferred because you chose to keep output identical; the confirm dialog is the interim guard.
2. Should the TUI adopt RC/mismatch options now that they live in `noise_core`? The plumbing exists after Phase 2; wiring `s`-screen checkboxes is ~30 lines.
3. Does the GUI need a session save/restore (last folder + selection + query) via a small JSON file? Cheap, but adds a state file to reason about.
4. Should `Results` be a third tab with both jobs side by side, or fold into the two job tabs? Decide in Phase 3 once the layout is visible.
