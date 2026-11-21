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
| 2 | Per-sample noise profile + CSV | 2–3 h | |
| 3 | Index-aware counts (own/other/position) | 2–3 h | |
| 4 | Barcode check against the probe table | 1–2 h | |
| 5 | Poly-G: effective length, optional trimming | 1–2 h | |
| 6 | Cross-sample pattern search | 4–6 h | |
| 7 | Reporting + batch mode | 2–3 h | |
| 8 | GUI surfaces; TUI if cheap | 3–4 h | |
| 9 | Docs, version 0.6.0, verification sweep | 1–2 h | |

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
3. Wire it into the GUI's existing overwrite guard (`outputs_to_overwrite`), since
   the name is fixed like the others.

**Acceptance:** for the 96 samples the CSV's sample1 row equals the hand-measured
numbers; the file is deterministic (browse order, fixed column order); a second run
overwrites only after the GUI asks.

## Phase 3 — Index-aware counts

**Goal:** per read, which indices it holds — its own, another sample's, both.

1. Extend `ReadAnatomy` with `other_index1_hits: Vec<(String, usize)>` computed
   against all sheet indices (bounded: only report the first three).
2. Profile columns: `other_index1` (count of reads containing a *different*
   sample's index1), `own_and_other` (reads with both), and a
   `swapped_pair` count — only set when both indices are present in the read.
3. Because index2 is absent from these 151-base reads, `swapped_pair` will read
   `0` or `n/a`; report `n/a` rather than a false zero, and say why in the column
   doc comment.

**Acceptance:** sample1's own-index count equals 1,129; a fixture read containing
another sample's index1 increments `other_index1`; `swapped_pair` is `n/a` when no
read carries index2.

## Phase 4 — Barcode check

**Goal:** classify each read against the probe table.

1. `ProbeTable` gives each variant's two 27-mers. New `BarcodeClass`:
   `ReadA | ReadB | Both | Neither`.
2. Profile columns: `barcode_a`, `barcode_b`, `barcode_both`, `barcode_none`.
   With the single barcode known today, `barcode_b` and `barcode_both` are zero
   until the real probe table lands — the columns exist so the shape does not
   change later.

**Acceptance:** with `docs/probe_table.csv` absent, the profile still counts the
single known 27-mer and reports the `readB` columns as `n/a` rather than zero; with
a two-variant fixture, all four classes are produced correctly.

## Phase 5 — Poly-G handling

**Goal:** make short reads visible and, optionally, countable as what they are.

1. `effective_len` histogram per sample in the profile (phase 2 already carries
   median; add the bucketed histogram to the CSV as extra rows or a second file —
   decide at implementation, favouring one file with a `metric` column).
2. `SearchOptions`/`CountOptions` gain `trim_poly_g: bool`, default `false`. When
   on, the read is truncated at `poly_g_start` before counting.
3. Never default-on: the golden guard proves default behaviour is untouched.

**Acceptance:** with trimming off, the golden hashes still match; with trimming on
for sample1, the unique-sequence count drops in the direction the effective-length
histogram predicts, and a fixture pins the exact number.

## Phase 6 — Cross-sample pattern search

**Goal:** the motivating example — a sequence pattern present across the selected
samples, whether that is all 96, a subset, or one.

1. `noise_core/src/patterns.rs`: `shared_kmers(files, k, opts) -> Vec<SharedPattern>`
   where `SharedPattern { kmer, samples_present: usize, total: u64, per_sample: Vec<u64>, expected: f64 }`.
   `files` is the selection, so "all samples" means all *selected* ones; with a
   single file selected the result is that file's k-mer list, degenerate but well
   defined.
2. Compute per file in parallel (the job runner already does this), intersect by
   k-mer hash, then rank.
3. `expected` from D5: the k-mer's expected count under each sample's mononucleotide
   frequencies; report `observed/expected`.
4. Output **`noise_results/shared_kmers/shared_kmers.csv`** — a folder of its own,
   because this is a cross-sample analysis rather than a per-run count. Header
   `kmer,samples_present,samples_selected,total,observed_over_expected`, with
   per-sample counts in long form (one row per k-mer per sample) unless `--wide`
   asks for one column per sample.
5. Write `noise_results/shared_kmers/run.txt` beside it: `k`, the threshold, the
   selected samples in order, and the command line. A ranking is only meaningful
   against the settings that produced it.
6. CLI/GUI surface is phase 8; phase 6 ships the core function and its writer,
   exercised by tests.

**Acceptance:** over `counted_results` with `k = 20` and "present in all 96",
the command completes in under 10 seconds in release and returns a ranked list; a
fixture with a k-mer planted in every sample ranks it first; a k-mer planted in one
sample is excluded when the threshold is "all", and included when the selection is
that one sample; `run.txt` records what was run; the expected-count column is
present and non-trivial.

## Phase 7 — Reporting and batch mode

**Goal:** the whole thing runs without a window, over a folder, reproducibly.

1. `noise_gui` keeps its GUI; add a small `noise_profile` binary or an
   `--profile` flag on the GUI? Prefer a separate binary in `noise_gui`
   (`noise_profile`) so the GUI binary keeps its single responsibility, and the
   TUI is untouched.
2. Flags: `--folder <path>`, `--sample-sheet <path>`, `--probe-table <path>`
   (optional), `--samples <name,…>` (default: all files in the folder),
   `--k <n>` (default 20), `--min-samples <n>` (default: all selected),
   `--trim-poly-g`, `--force`.
3. Writes `noise_profile.csv` and `shared_kmers/shared_kmers.csv` under the sibling
   `noise_results/`; refuses to overwrite either without `--force`, matching the
   GUI's guard.
4. Performance: stream per file, do not hold all reads; the 96-sample set should
   stay under a few seconds.

**Acceptance:** `cargo run --release -p noise_gui --bin noise_profile -- --folder
counted_results --sample-sheet docs/sample_sheet.csv` writes both outputs; running
it twice without `--force` refuses the second time; `--samples sample1,sample2`
analyses exactly those two and `run.txt` names them; `--min-samples 96` reproduces
the phase 6 answer.

## Phase 8 — UI surfaces

**Goal:** the profile and the shared patterns are visible, not just on disk.

1. New GUI tab **Noise profile**: the per-sample table (sortable, filterable,
   reusing `TableBuilder`), with the class counts as columns.
2. New GUI tab or section **Shared patterns**: the ranked k-mer list, with a bar
   chart of `samples_present` and a drill-down showing per-sample counts.
3. Charts: effective-length histogram per sample; class composition.
4. TUI: only if cheap — a single screen showing the profile for the highlighted
   sample, produced from the same core call. Not a requirement of this phase.
5. Keep the tab 3 layout lesson: charts inside a bounded, scrollable region with a
   reserve for the table (the layout test already enforces this pattern).

**Acceptance:** the profile table matches the CSV row for row; the shared-pattern
view updates when `k` or the threshold changes without re-reading files that have
not changed; the layout test is extended to the new tabs.

## Phase 9 — Docs and verification

1. README: new jobs, new files, new flags, and the standing rule that default
   output is unchanged.
2. `glossary.md`: fold in any term this work introduces (`k-mer`, `shared pattern`,
   `effective length`, `observed/expected`).
3. Version `0.6.0` in the workspace, `APP_VERSION` in step.
4. Full sweep: `cargo test --release --workspace`, both `#[ignore]` golden guards,
   TUI smoke through a pty, GUI launch, and the new CLI on `counted_results`.

## Output files after this plan

| File | Written by | Notes |
|---|---|---|
| `noisecounts_<sample>.csv` | count job | unchanged, golden-guarded |
| `sequencecount.csv` | search job | unchanged, golden-guarded |
| `noise_profile.csv` | profile | new, fixed name, guarded |
| `shared_kmers/shared_kmers.csv` | patterns | new, own folder, guarded |
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
