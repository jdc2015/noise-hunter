# noise-hunter

**v0.6.0** — count identical DNA sequences in plain-text read files, and count how
many reads contain a sequence you type. Two front ends over one core:

| Crate | What it is |
|---|---|
| `noise_core` | All the counting logic, std-only, no UI dependencies |
| `noise_counter` | Terminal UI (ratatui + crossterm) |
| `noise_gui` | Desktop GUI (egui / eframe) |

Both front ends do the same two jobs and write the **same files to the same
place**. With default options their output is byte-identical — that is asserted
against a checksum manifest, not merely intended.

1. **Duplicate counts** — tally identical sequences per sample, one CSV per
   sample.
2. **Sequence search** — count reads *containing* a sequence, one combined CSV.

## Quick start

```bash
cargo run --release -p noise_counter                  # terminal UI
cargo run --release -p noise_gui -- --folder counted_results   # GUI

# headless: profile every sample and find the k-mers they share
cargo run --release -p noise_cli --bin noise_profile -- \
    --folder counted_results --sample-sheet docs/sample_sheet.csv
```

## The output contract

Everything below is identical in both tools, and is what makes them
interchangeable.

- Output always goes to a `noise_results` folder that is a **sibling** of the
  input folder — never inside it, and never chosen separately.
- File names are fixed: `noisecounts_<sample>.csv` per sample, and
  `sequencecount.csv` for a search. No timestamps, no suffixes for filters or
  options.
- Because the names are fixed, a run **replaces** earlier output. The TUI
  overwrites silently (copy a CSV aside first); the GUI lists what would be
  replaced and asks before starting.
- A read counts at most once per sample, even if the sequence occurs in it
  several times, and — with reverse complement on — even if it matches on both
  strands.

## Duplicate counting (both tools)

- Every line of a `*_unbinned_reads` file is one sequence. Lines are trimmed,
  trailing `N` bases are stripped, and the rest is uppercased. A line that ends
  up empty, or still contains anything but `A/C/G/T`, is *skipped*.
- An optional **length filter** truncates sequences longer than `N` bases to
  their first `N` before counting, collapsing near-duplicates. Shorter reads keep
  their own length, so a filtered CSV can mix lengths. `0` is rejected; blank
  means full length.
- CSV header `sequence,counts,length`, ordered by count descending, ties
  alphabetical.

For `sample1` with a length filter of 30, `noise_results/noisecounts_sample1.csv`
starts:

```
sequence,counts,length
CACCAGACACGTCTGAACTCCAGTCACATA,884,30
CTGTCTCTTATACACGTCTGAACTCCAGTC,29,30
...
```

That run reads 1,246 lines, skips 2, counts 1,244, and finds 181 unique
sequences — down from 516 at full length.

## Sequence search (both tools)

One combined `noise_results/sequencecount.csv`: the queried sequence is the
column header, one row per sample in browse order, then a `total` row.

```
sample,CACCAGACACGTCTGAACTCCAGTCAC
sample1,888
sample2,1552
...
sample96,86
total,74985
```

That is the real result of the 27-base query over all 96 samples in
`counted_results`.

### Options and the columns they add

Default options reproduce the file above exactly. Extra columns appear **only**
for a mode that is switched on:

| Mode | Header |
|---|---|
| default (exact, as typed) | `sample,<SEQ>` |
| reverse complement | `sample,<SEQ>,<SEQ>_rc,combined` |
| mismatch tolerance `d` | `sample,<SEQ>,within_<d>,best_mismatches` |
| both | the union of the two |

`combined` counts a read once when it matches either strand. `within_<d>` counts
reads with a sliding window within `d` mismatches, and `best_mismatches` is the
closest distance seen in that sample (empty in the `total` row, which has no
single best). Exact counts are unchanged by either mode — turning an option on
never moves the numbers in the first two columns.

Mismatch searching costs O(reads × query length) per file, which is why the GUI
only runs it on an explicit Run.

## Noise profile

`noise_results/noise_profile.csv` has one row per sample: usable reads, reads
carrying the 27-base allele-locus barcode, reads containing index1 (and how many
sit in its usual base-28 slot), reads containing **another** sample's index1, reads
ending in G padding, and the median effective read length. `effective_lengths.csv`
holds the exact length histogram per sample.

It needs the sample sheet — the indices belong to the run, not to the program —
and a sample with no reads contributes a row of zeros rather than being skipped
(`sample57` and `sample71` are empty).

## Shared patterns

A **k-mer** present in every compared sample. The search writes
`noise_results/shared_kmers/`: `shared_kmers.csv` (ranked), 
`shared_kmers_by_sample.csv` (per-sample counts) and `run.txt` (k, exclusions,
selection, command line).

- `k` is arbitrary and chosen by you. It is not tied to the barcode: a shared
  k-mer may contain part of the barcode, part of an index, or nothing
  recognisable.
- **Observed/expected** comes from a mononucleotide model, so a designed sequence
  scores in the millions and a G-rich artefact in the tens.
- A sample with no reads cannot hold a pattern and is not required to;
  `samples_compared` says how many were compared.
- Exclusions remove what is shared because it is the construct rather than the
  noise: `--exclude-barcode` and `--exclude-g-rich <n>`.

First result on this sample set: at `k = 20`, 20 k-mers are shared by all 94
comparable samples — eight fragments of the barcode and twelve G-rich padding
boundaries. Excluding both leaves **nothing** shared at that length.

## Desktop GUI (`noise_gui`)

Three tabs, driven with a mouse — every job also starts from the keyboard.

**1 Browse & count.** `Open folder…` (or `Ctrl+O`) lists every `*_unbinned_reads`
file in natural order (`sample2` before `sample10`) and **selects them all**.
Narrow with per-file checkboxes, `Select all` / `Select none` / `Select filtered`
(the filter box accepts a sample name or part of a file name; typing in it never
unselects anything), and set the length filter, which validates as you type. Then
`Count selected` (or `Ctrl+R`).

**2 Search sequence.** Type a sequence; the per-sample counts update as you type,
without writing anything. `Also search reverse complement` and `Allow mismatches`
(1–10) switch on the extra columns shown above. A tolerant scan costs every
selected sample, so **live counts pause while mismatches are on** — the tab says
so, and `Run search` is what performs it, reporting elapsed time. Live counts
write no CSV; only an explicit run does.

**3 Results.** Three views over the same runs:

- **Counts** — one row per sample (reads, used, skipped, unique), sortable by any
  column, with `open` to browse that sample's sequences.
- **Counts for query** — one row per sample with the count, the closest match,
  and either the nearest window or the within-tolerance count. The columns
  describe the numbers on screen, not the form.
- **Sequences** — one sample's `noisecounts_*.csv` (first 5,000 rows), sortable
  and filterable.

Above the tables: a bar chart of counts per sample (the largest 24, with a note
when samples are omitted) and a distribution histogram, or — for a count run —
reads used versus skipped per sample. `Export CSV…` writes the table you are
looking at, through the same writer a run uses, so an export is byte-identical to
a run's file.

Before any run that would replace existing output, the GUI lists the files it
would overwrite and waits for confirmation — the fixed names make this necessary,
and it is the one place the GUI deliberately differs from the TUI.

**4 Noise profile.** Choose the sample sheet, then `Profile selected`: the
per-sample table described above, sortable by any column. A sample with no reads
shows `no reads` instead of a blank row.

**5 Shared patterns.** Set `k`, optionally exclude the barcode or G-rich k-mers,
then `Find shared patterns`. The table ranks what every compared sample holds;
clicking a k-mer lists its count in each sample underneath.

Shortcuts: `Ctrl+O` folder · `Ctrl+R` run (the job the current tab is about) ·
`Ctrl+F` filter · `1`/`2`/`3` tabs · `R` refresh live counts · `Esc` cancel a
dialog, else clear the selection, else clear the messages.

Flags: `--folder <path>` opens a folder on start-up; `--renderer wgpu|glow`
picks the renderer. `glow` is the escape hatch if the default `wgpu`/Vulkan path
misbehaves on a given GPU.

## Terminal UI (`noise_counter`)

Run it with no arguments; it opens a file browser in the working directory.

| Screen | Keys |
|--------|------|
| Welcome | `Enter` browse folders · `q`/`Esc` quit |
| Browse | `↑`/`↓` (or `j`/`k`) select · `Enter` open folder **or** use the highlighted single file · `Space` count every `*_unbinned_reads` file in this folder · `s` count a specific sequence · `Esc` back · `q` quit |
| Examine | shows top 10 reads + ruler · type digits (yellow block cursor) · `↑`/`↓` add/subtract 10 · `←`/`→` and `Home`/`End` move the caret · `Backspace`/`Delete` edit · blank = full length · `Enter` apply and start counting · `Esc` back · `q` quit |
| Sequence query | type A/C/G/T · `Backspace`/`Delete` edit · `←`/`→`, `Home`/`End` move the caret · `Tab` toggle highlighted file ↔ all files · `Enter` count · `Esc` back · `q` quit |
| Sequence results | `↑`/`↓` (or `j`/`k`) scroll · `PgUp`/`PgDn` jump 10 · `Home`/`End` first/last · `Esc` back · `q`/`Enter` quit |
| Results | `Enter`/`n` next file · `a` all remaining files · `q`/`Esc` quit (written CSVs stay) |
| Totals | `Enter`/`q`/`Esc` quit · `r` restart browsing |

`Ctrl+Q` quits from any screen. The TUI also shows the top-10 table with a base
ruler, and closest-match diagnostics with mismatched bases highlighted in red for
zero-hit samples. Those diagnostics are display-only and never written to the
CSV.

The TUI does **not** yet offer reverse complement or mismatch tolerance; the
core supports both, and the GUI exposes them.

## Repository layout

```
noise-hunter/
├── Cargo.toml                  # workspace: noise_core, noise_counter, noise_gui, noise_cli
├── baseline.sha256             # 193 checksums of v0.4.0 output, the regression guard
├── counted_results/            # sample data: 96 sample pairs
│   ├── sampleN_unbinned_reads      # one sequence per line  <- input
│   └── sampleN_binned_results.csv  # not read by this program
├── docs/
│   └── sample_sheet.csv        # the 96-sample dual-index sheet
├── noise_results/              # output folder (created next to the input folder)
├── noise_core/                 # counting, profile and pattern logic, std-only
├── noise_counter/              # terminal UI
├── noise_gui/                  # desktop GUI
└── noise_cli/                  # headless noise_profile binary
```

## Tests and verification

```bash
cargo test --release --workspace                    # 59 core + 9 GUI + layout tests
cargo test --release -p noise_core -- --ignored     # 193 CSVs vs baseline.sha256
cargo test --release -p noise_gui  -- --ignored     # the same, through the GUI's parallel path
cargo test --release -p noise_core --test anatomy --test profile --test patterns -- --ignored
```

The unit tests cover sequence cleaning, the length filter, CSV ordering, the
combined search CSV and its option-dependent headers, closest-match diagnostics,
file discovery and sorting, the sample sheet, read anatomy, the noise profile and
the pattern search; the GUI tests drive its worker thread and assert its output
matches the core's byte for byte. They create their own temporary folders, so
`counted_results/` is not needed.

The ignored guards are the real-data checks: the two golden tests re-run both
counting jobs over all 96 samples and compare every CSV against `baseline.sha256`;
the anatomy, profile and pattern tests pin the measured numbers (888 barcode,
1,129 index1, 1,134 poly-G in sample1; 20 shared 20-mers, all barcode or padding).

The two `#[ignore]`d tests are the real regression guard: they re-run both jobs
over all 96 samples and compare every generated CSV against `baseline.sha256`,
captured from v0.4.0 before the core was extracted. Both paths — the TUI's
sequential one and the GUI's parallel one — reproduce it exactly.

## Notes & limits

- No headless or batch mode; the GUI's `--folder` and `--renderer` are the only
  flags anywhere.
- Mismatch searching is deliberately not live in the GUI, and not exposed at all
  in the TUI.
- Everything is held in memory per file (`HashMap` of sequence → count), so very
  large read files are limited by RAM.
- Sample numbering assumes the `sampleN_unbinned_reads` naming; other names work
  but sort last and are used verbatim as the sample name.
- The GUI's sequence browser reads the first 5,000 rows of a sample's CSV.
- Charts show the largest 24 samples, with a note when more exist.
- The profile and the pattern search both need the sample sheet; the pattern
  search holds one file's k-mers at a time, so memory follows the largest file
  rather than the whole set.
- A sample with no unbinned reads contributes zeros to the profile and is not
  required to hold a shared pattern.

## License

MIT — see [LICENSE](LICENSE). Copyright (c) 2026 John D Curry.

