# Refinement plan 1

Date: 2026-11-21
Scope: turn `noise-hunter` from a per-file counter into a per-sample *and*
cross-sample noise profiler, without changing anything it already writes.
Companion documents: [glossary.md](glossary.md), [refinements.md](refinements.md).

## What exists today

- `noise_core` — counting, search, file discovery; std-only; 30 tests.
- `noise_counter` (TUI) and `noise_gui` (GUI) — two front ends over that core.
- Two jobs: duplicate counting (`noisecounts_<sample>.csv`) and sequence search
  (`sequencecount.csv`), both byte-identical to v0.4.0 by default.
- A golden guard: 193 checksums in `baseline.sha256`, re-run by two `#[ignore]`d
  tests.
- 96 samples in `counted_results/`, 1.2k–2.1k reads each, 151 bases typical.

## Measured facts this plan builds on

| Fact | Value |
|---|---|
| Read layout | `[27-base barcode][index1][insert]`, poly-G padding when short |
| Barcode | `CACCAGACACGTCTGAACTCCAGTCAC`, same in every sample checked |
| index1 slot | base 28 in 966/1,244 sample1 reads, 1,692/2,119 sample2, 337/914 sample8 |
| index1 anywhere | 1,129/1,244 sample1 reads |
| index2 | 0/96 files contain their own; it sits beyond these reads |
| Poly-G | 1,134/1,244 sample1, 1,998/2,119 sample2, 112/115 sample96 |
| Barcode present | 888/1,244 sample1, 286/1,436 sample40 |
| Sheet | 96 rows, CRLF, every pair listed again reversed for sample `97 − i` |

## Design decisions

| # | Decision |
|---|---|
| D1 | All per-read classification lives in `noise_core` as pure functions over a cleaned sequence. Front ends only render; nothing about noise classes is computed twice. |
| D2 | **Additive only.** Existing files, names, headers and bytes do not change; the golden guard must keep passing at every phase. New outputs get new names. |
| D3 | A "pattern" starts as an **exact k-mer** presence/absence across the selected samples. `k` is the user's choice (default 20) and deliberately **arbitrary**: it is not tied to the 27-base barcode, and a k-mer may contain part of the barcode, part of an index, or nothing recognisable — what the noise looks like is the open question. Family/within-N-mismatch matching is a later plan, not this one. |
| D4 | Thresholds are **relative to the selection**, never to the full set: the user may analyse all 96 samples, a subset, or one. Default "present in every selected sample with ≥ 1 read"; `--min-samples` narrows that within the selection. |
| D5 | Chance is reported, not gated: each shared k-mer gets an expected count under a mononucleotide model per sample, so a G-rich artefact is visible as such. |
| D6 | Sample sheet and probe table are **runtime data files**, not compiled-in tables: `--sample-sheet` and `--probe-table` flags plus GUI pickers. The sheet is required for index work; the probe table is **optional** — committing one is the user's choice, and phases 3–4 degrade to `n/a` without it. |
| D7 | Poly-G trimming is **opt-in**. Default counts stay exactly as they are today. |

## Phase summary

| Phase | Work | Est. | Status |
|---|---|---|---|
| 0 | Tables in the repo, parsers, fixtures | 1–2 h | done |
| 1 | Read anatomy: one function, one struct | 2–3 h | done |
| 2 | Per-sample noise profile + CSV | 2–3 h | done |
| 3 | Index-aware counts (own/other/position) | 2–3 h | done |
| 4 | Barcode check against the probe table | 1–2 h | blocked on probe_table.csv |
| 5 | Poly-G: effective length, optional trimming | 1–2 h | 5a done, 5b pending |
| 6 | Cross-sample pattern search | 4–6 h | done |
| 7 | Reporting + batch mode | 2–3 h | done |
| 8 | GUI surfaces; TUI if cheap | 3–4 h | done |
| 9 | Docs, version 0.6.0, verification sweep | 1–2 h | done |

Dependencies: 0 → 1 → {2, 3, 4, 5} → 6 → 7 → 8 → 9. Phases 3–5 are independent of
each other and can be reordered.

## Phase 0 — Inputs and parsers

**Goal:** the two upstream tables are in the repository and readable by tests.

1. Add `docs/sample_sheet.csv` (the 96-row dual-index sheet). `docs/probe_table.csv`
   is optional and supplied only if the user chooses to commit one; keep either as
   supplied, and note the CRLF line endings.
2. New `noise_core/src/tables.rs`: `SampleSheet::parse(&str)`, tolerant of CRLF
   and a header, rejecting duplicate sample names and indices that are not eight
   `A/C/G/T` bases. Structs: `SampleSheet { samples: Vec<SampleIndices> }`,
   `SampleIndices { name, index1, index2 }`. The probe-table parser lands with the
   file itself (phase 4) rather than guessing at its columns.
3. CLI/GUI plumbing for the paths comes in phase 7; this phase only parses.

**Acceptance:** 96 rows parsed from the committed sheet; a test asserts that for
every `i`, sample `97 − i` carries the same two indices reversed; CRLF and LF
both parse; a malformed row is a typed error, not a panic.

## Phase 1 — Read anatomy

**Goal:** one pure function that says what a read is made of.

```rust
pub struct ReadAnatomy {
    pub barcode: Option<usize>,      // offset of the 27-mer, 1-based
    pub index1: Option<usize>,       // offset of the sample's index1
    pub index1_expected: bool,       // == Some(28)
    pub index2: Option<usize>,       // offset of the sample's index2
    pub poly_g_start: Option<usize>, // first base of the terminal G-run
    pub effective_len: usize,        // poly_g_start - 1, else full length
}

pub fn anatomise(read: &str, indices: &SampleIndices, barcode: &str) -> ReadAnatomy;
```

1. Reuse `clean_sequence` so anatomy and counting agree on what a read is.
2. `poly_g_start` = start of the trailing run of `G`/`g` when the run is ≥ 8 bases;
   below that it is not padding.
3. Offsets are 1-based, matching how the measurements above are quoted.

**Acceptance:** the function reproduces the measured table for sample1 — 966
index1-at-28, 1,129 index1-anywhere, 1,134 poly-G, 888 barcode — and for samples
2, 40 and 96. These numbers are already hand-measured; the test pins them.

## Phase 2 — Per-sample noise profile

**Goal:** the profile table described in `refinements.md` §2, as data and a CSV.

1. `noise_core/src/profile.rs`: `SampleProfile` with counts for reads, reads with
   barcode, index1 present / in slot / elsewhere, index2 present, both indices,
   neither, poly-G, effective-length histogram (bucketed).
2. Writer: `noise_results/noise_profile.csv`, one row per sample, header
   `sample,reads,barcode,index1,index1_slot,index1_elsewhere,index2,both_indices,
   neither_index,poly_g,median_effective_len`.
3. `PROFILE_FILE` is exported so the overwrite guard can name it; wiring it into
   `outputs_to_overwrite` happens with the job itself in phase 7, since a guard
   with nothing to guard is dead code.

**Acceptance:** for the 96 samples the CSV's sample1 row equals the hand-measured
numbers; the file is deterministic (browse order, fixed column order); a second run
overwrites only after the GUI asks.

## Phase 3 — Index-aware counts

**Goal:** per read, which indices it holds — its own, another sample's, both.

1. The foreign scan lives in the profile, not in `ReadAnatomy`: anatomy is about
   one read's own structure, while "another sample's index" needs the whole sheet.
   A `HashMap` of the other samples' index1 8-mers makes each read one hash lookup
   per window instead of one search per sample.
2. Profile columns: `other_index1` (reads containing a different sample's index1),
   `own_and_other` (reads with both), and `swapped_pair` — `Some(count)` only when
   both indices appear, otherwise an empty field.
3. Because index2 is absent from these 151-base reads, `swapped_pair` is `n/a`
   rather than a false zero, and the column says so.
4. The count is reported raw. An 8-mer occurs by chance about once per 65,536
   windows, so the number is only interpretable beside an expectation — which the
   pattern search reports, and which phase 6 could reuse here.

**Acceptance:** sample1's own-index count equals 1,129; a fixture read containing
another sample's index1 increments `other_index1`; `swapped_pair` is `n/a` when no
read carries index2. Measured: 22 of sample1's reads carry a foreign index1, six
of them alongside their own.

## Phase 4 — Barcode check (blocked)

**Goal:** classify each read against the probe table.

1. `ProbeTable` gives each variant's two 27-mers. New `BarcodeClass`:
   `ReadA | ReadB | Both | Neither`.
2. Profile columns: `barcode_a`, `barcode_b`, `barcode_both`, `barcode_none`.
   The profile today counts a single known 27-mer in `barcode`; the four-way split
   waits for the table rather than adding three columns that can only be zero.

**Status:** blocked on `docs/probe_table.csv`, which is supplied at the user's
discretion. The parser will be written against the real file's columns, not a
guess.

**Acceptance when unblocked:** with the table absent, the profile still counts the
single known 27-mer and the split columns are `n/a`; with a two-variant fixture,
all four classes are produced correctly.

## Phase 5 — Poly-G handling

**Goal:** make short reads visible and, optionally, countable as what they are.

### 5a — effective length (done)

1. The profile keeps an exact histogram of effective lengths, and
   `write_lengths` emits `noise_results/effective_lengths.csv` —
   `sample,effective_len,reads`, shortest first, no rows for an empty sample.
   One tidy file rather than 96, and no mixed row types inside the profile table.
2. sample1's median effective length is **65 bases**: half its reads carry no more
   than that before the padding starts.

### 5b — optional trimming (pending)

1. `SearchOptions`/`CountOptions` gain `trim_poly_g: bool`, default `false`. When
   on, the read is truncated at `poly_g_start` before counting.
2. This touches both front ends and the output contract, so it lands as its own
   change with the golden guard proving default behaviour is untouched.

**Acceptance for 5b:** with trimming off, the golden hashes still match; with
trimming on for sample1, the unique-sequence count drops in the direction the
effective-length histogram predicts, and a fixture pins the exact number.

## Phase 6 — Cross-sample pattern search

**Goal:** the motivating example — a sequence pattern present across the selected
samples, whether that is all 96, a subset, or one.

1. `noise_core/src/patterns.rs`: `shared_kmers(files, k) -> PatternSearch` where
   `PatternSearch { patterns: Vec<SharedKmer>, samples_compared }` and
   `SharedKmer { kmer, per_sample: Vec<u64>, expected: f64 }`. `files` is the
   selection, so "all samples" means all *selected* ones; with a single file
   selected the result is that file's k-mer list, degenerate but well defined.
2. Intersect file by file, holding only the running candidate set — the memory cost
   is one file's k-mers, not all of them. Rank by total count, ties alphabetical.
3. `expected` from D5: the k-mer's expected count under each sample's mononucleotide
   frequencies; report `observed/expected`.
4. A sample with no reads cannot hold a pattern, so it is not required to:
   `samples_compared` records how many files were comparable, and an empty sample
   cannot erase a finding. This was found by the first real run, not designed in.
5. Output **`noise_results/shared_kmers/`**: `shared_kmers.csv` (header
   `kmer,samples_present,samples_selected,samples_compared,total,expected,
   observed_over_expected`), `shared_kmers_by_sample.csv`
   (`kmer,sample,count`), and `run.txt` with `k`, the selection, the compared
   count, the sample list and the command line.
6. CLI/GUI surface is phase 8; phase 6 ships the core function and its writer,
   exercised by tests.

**Status:** done (core). First real result: at `k = 20`, 20 k-mers are shared by
all 94 comparable samples — eight fragments of the constant barcode and twelve
G-rich padding boundaries, and nothing else. At `k = 12`, 35 shared, all explained
the same way. The test pins that invariant, so a new unexplained universal pattern
would fail it.

**Exclusions (done):** `shared_kmers_with` drops k-mers that are substrings of a
barcode or that carry at least N `G`s, and reports how many it removed. With
`--exclude-barcode --exclude-g-rich 8`, all 20 are removed and **nothing** is
shared at k=20 — so the answer to "is there a universal noise 20-mer" is no, once
the construct and the padding are taken out.

**Acceptance:** over `counted_results` with `k = 20` and "present in all 96",
the command completes in under 10 seconds in release and returns a ranked list; a
fixture with a k-mer planted in every sample ranks it first; a k-mer planted in one
sample is excluded when the threshold is "all", and included when the selection is
that one sample; `run.txt` records what was run; the expected-count column is
present and non-trivial.

## Phase 7 — Reporting and batch mode

**Goal:** the whole thing runs without a window, over a folder, reproducibly.

1. A new crate, `noise_cli`, holding the `noise_profile` binary. The plan said a
   bin in `noise_gui`; a command-line analysis should not link eframe, so it gets a
   crate with one dependency (`noise_core`) and builds in seconds.
2. Flags: `--folder`, `--sample-sheet`, `--samples a,b`, `--barcode <seq>`,
   `--no-barcode`, `--k <n>` (default 20), `--exclude-barcode`,
   `--exclude-g-rich <n>`, `--force`. The probe table and trimming are not wired
   yet: phase 4 is blocked and 5b is its own change.
3. Writes `noise_profile.csv`, `effective_lengths.csv` and
   `shared_kmers/` under the sibling `noise_results/`; refuses to replace any of
   them without `--force`, naming what it would replace.
4. Performance: the 96-sample set profiles and searches in a couple of seconds.

**Status:** done. The real run reports `profiled 96 samples (94 compared, 2 empty)`
and writes all three outputs; the guard refuses a second run without `--force`.

**Acceptance:** met — a second run is refused, `--samples` narrows the run, and
`run.txt` records the command, `k`, exclusions and compared count.

## Phase 8 — UI surfaces

**Goal:** the profile and the shared patterns are visible, not just on disk.

1. New GUI tab **4 Noise profile**: a sample-sheet picker, `Profile selected`, and
   the per-sample table (sortable by every column, reusing `TableBuilder`). A
   sample with no reads says so instead of showing a blank row.
2. New GUI tab **5 Shared patterns**: `k`, the two exclusions, `Find shared
   patterns`, the ranked k-mer table, and a drill-down listing a selected k-mer's
   count in each sample.
3. No bar chart of `samples_present`: every returned pattern has the same
   `samples_present` by definition, so the chart would be a row of equal bars. The
   table sorted by total or by observed/expected is the useful view.
4. `k` and the exclusions do **not** re-run the search as you edit them: a search
   takes about a second over 96 samples, so it is a button, like the mismatch
   slider in the search tab.
5. The drill-down sits under the table, so the table gives it a reserve — the same
   lesson as the charts, one tab over, enforced by the layout test.
6. TUI: not touched. The profile and patterns are GUI and CLI features; adding a
   screen to the TUI would duplicate them for no gain.

**Status:** done. The layout test now covers tabs 3, 4 and 5 at both the default
and minimum window sizes, with the drill-down open.

**Acceptance:** met, with one deliberate change — the shared-pattern view does not
live-update on `k`, it runs on request. The profile table is built from the same
`SampleProfile` values that were written to the CSV, so the two cannot disagree.

## Phase 9 — Docs and verification

1. README: new jobs, new files, new flags, and the standing rule that default
   output is unchanged.
2. `glossary.md`: fold in any term this work introduces (`k-mer`, `shared pattern`,
   `effective length`, `observed/expected`).
3. Version `0.6.0` in the workspace, `APP_VERSION` in step.
4. Full sweep: `cargo test --release --workspace`, both `#[ignore]` golden guards,
   TUI smoke through a pty, GUI launch, and the new CLI on `counted_results`.

**Status:** done. README documents the profile, the pattern search and the CLI;
the glossary gained the profile terms; the workspace is 0.6.0 with `APP_VERSION`
in step; the sweep is recorded in the commit that closed the plan.

## Output files after this plan

| File | Written by | Notes |
|---|---|---|
| `noisecounts_<sample>.csv` | count job | unchanged, golden-guarded |
| `sequencecount.csv` | search job | unchanged, golden-guarded |
| `noise_profile.csv` | profile | new, fixed name, guarded |
| `effective_lengths.csv` | profile | new, one row per sample and length |
| `shared_kmers/shared_kmers.csv` | patterns | new, own folder, guarded |
| `shared_kmers/shared_kmers_by_sample.csv` | patterns | per-sample counts, long form |
| `shared_kmers/run.txt` | patterns | the settings a ranking was produced under |

## Risks

| Risk | Mitigation |
|---|---|
| `index2` never appears in these reads, so swapped-pair detection cannot be measured | Report `n/a`, not zero; revisit if longer reads or index reads become available |
| Thresholds and the chance model get tuned to the 96 samples | Defaults documented, values configurable, and the chance column shown rather than hidden |
| Poly-G trimming silently changes counts | Opt-in only; golden guard runs on every phase |
| New fixed-name outputs clobber earlier runs | Extend the existing overwrite guard; `--force` in batch mode |
| Scope creep into re-binning reads | The binner stays out of scope: this program explains rejections, it does not redo assignments |
| 96 samples are small; real runs are 25M–2000M reads | Profile and patterns stream per file; memory limits stated in the README rather than discovered later |

## Settled decisions

| Question | Answer |
|---|---|
| Pattern length | `k` is the user's choice, default 20, and deliberately arbitrary — not tied to the 27-base barcode. A k-mer may contain part of the barcode, part of an index, or nothing recognisable. |
| Shared threshold | Relative to the selection: all 96 samples, a subset, or one. Default "every selected sample"; `--min-samples` narrows it. |
| Probe table | Optional. Committing one is the user's decision; phases 3–4 report `n/a` without it. |
| Pattern output | `noise_results/shared_kmers/`, holding `shared_kmers.csv` and `run.txt`. |

## Left to implementation judgement

- Whether the effective-length histogram is extra rows in `noise_profile.csv`
  (a `metric` column) or a second file. One file is preferred.
- Whether `--wide` earns its place; if 96 columns are never useful, drop it.
