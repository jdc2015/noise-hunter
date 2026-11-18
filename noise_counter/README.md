# noise_counter

**v0.4.0** — a small Rust TUI (ratatui + crossterm) with two jobs:

1. **Duplicate counts** — tally identical DNA sequences in plain-text
   `*_unbinned_reads` files and write one CSV per sample.
2. **Sequence search** — count how many reads *contain* a sequence you type,
   per sample, into one combined CSV.

There are no command-line flags: run the binary and drive everything from the
keyboard. It opens the file browser in the current working directory.

## What it does

### Browsing
- Only three kinds of entries are listed: `../`, subfolders, and files whose
  name ends in `_unbinned_reads` (e.g. `sample1_unbinned_reads`). Everything
  else — including the `*_binned_results.csv` files that sit next to the reads —
  is ignored, as are dotfiles and dot-folders.
- Read files are sorted **numerically**, so `sample2` comes before `sample10`;
  names that don't look like `sampleN_unbinned_reads` sort after them.
- `Enter` opens a folder or uses the highlighted file alone; `Space` selects
  **every** `*_unbinned_reads` file in the current folder.

### Duplicate counting
- Reads files where every line is a single sequence.
- Each line is trimmed of surrounding whitespace and trailing `N` bases, then
  uppercased. Lines that end up empty or still contain anything other than
  `A/C/G/T` (internal `N`s, `X`, spaces …) are skipped and counted as *skipped*.
- On selection it examines the **first** file and shows the **top 10
  most-frequent reads** in a table, with a size ruler marking base positions 1,
  10, 20 … up to the width of the sequence column (max 100 bases; longer
  sequences are clipped with `…`).
- You are then prompted for an optional **length filter**: if given, sequences
  longer than `N` bases are truncated to their first `N` before counting,
  collapsing many near-duplicates so the output is much shorter and easier to
  examine. Reads shorter than `N` keep their own length, so a filtered CSV can
  contain mixed lengths.
- Writes a CSV per sample with header `sequence,counts,length`, ordered by count
  descending (ties sorted alphabetically).
- After each file, a results screen shows lines read / counted / skipped /
  unique, the output path, and a **preview of the first 20 lines** of the CSV it
  just wrote. When the last file is done, a totals screen sums every file.

### Counting a specific sequence
- Press `s` on the browse screen, type a sequence (A/C/G/T), and it reports how
  many reads **contain** it — in the highlighted file, or across every file in
  the folder (`Tab` toggles when a file is highlighted; with a folder selected
  it always searches all files). Results go to a single combined CSV whose
  header embeds the sequence.
- **Closest-match diagnostics**: when a sample has zero hits, the results screen
  shows how close it came — the minimum number of mismatches and the nearest
  equal-length read window, with mismatched bases highlighted in red. If every
  read is shorter than the query, it says so instead. This is display-only; the
  CSV still contains exact counts only, and samples with hits pay no extra cost
  for the search.
- The per-sample results table is scrollable, so every selected sample can be
  inspected even when there are dozens of rows.

## Build

Needs a current stable Rust toolchain (edition 2021). From `noise_counter/`:

```bash
cargo build --release
# binary at ./target/release/noise_counter
```

The release profile enables LTO and a single codegen unit. `src/tui.rs` carries
its own `APP_VERSION` constant shown in the header, so bump it together with
`version` in `Cargo.toml`.

## Repository layout

```
noise-hunter/
├── counted_results/            # sample data: 96 sample pairs
│   ├── sampleN_unbinned_reads      # one sequence per line  <- program input
│   └── sampleN_binned_results.csv  # not read by this program
├── noise_results/              # output folder (created next to the input folder)
└── noise_counter/              # this program
```

## Usage (TUI)

Run it and follow the on-screen keys:

| Screen | Keys |
|--------|------|
| Welcome | `Enter` browse folders · `q`/`Esc` quit |
| Browse | `↑`/`↓` (or `j`/`k`) select · `Enter` open folder **or** use the highlighted single file · `Space` count every `*_unbinned_reads` file in this folder · `s` count a specific sequence · `Esc` back to Welcome · `q` quit |
| Examine | shows top 10 reads + ruler · **type digits** into the field (a yellow block cursor marks the caret) · `↑`/`↓` add/subtract 10 for quick presets · `←`/`→` and `Home`/`End` move the caret · `Backspace`/`Delete` edit · blank = full length · `Enter` apply and start counting · `Esc` back to Browse · `q` quit |
| Sequence query | type A/C/G/T (a block cursor marks the caret) · `Backspace`/`Delete` edit · `←`/`→` and `Home`/`End` move the caret · `Tab` toggle highlighted file ↔ all files (only when a file was highlighted) · `Enter` count · `Esc` back to Browse · `q` quit |
| Sequence results | `↑`/`↓` (or `j`/`k`) scroll one row · `PgUp`/`PgDn` jump 10 · `Home`/`End` first/last sample · `Esc` back to Browse · `q`/`Enter` quit |
| Results | `Enter`/`n` next file · `a` all remaining files in one pass · `q`/`Esc` quit (no back — already-written CSVs stay) |
| Totals | `Enter`/`q`/`Esc` quit · `r` restart browsing from the input folder |

`Ctrl+Q` quits from any screen. A length filter of `0` is rejected ("at least 1,
leave blank for full length"); `↑` on a blank field starts at 10, and `↓` never
steps below blank.

From the repository root it will start the browser in the current directory, so
you can open `counted_results` and press `Space` to count all samples.

## Output

All output goes to a new `noise_results` folder created as a **sibling** of the
folder holding the selected input(s) — never inside the input folder itself.

### Duplicate counts (one CSV per sample)

For `sample1_unbinned_reads` the file is `noise_results/noisecounts_sample1.csv`
(the sample name is the input filename minus the `_unbinned_reads` suffix):

```
sequence,counts,length
CACCAGACACGTCTGAACTCCAGTCACATA,884,30
CTGTCTCTTATACACGTCTGAACTCCAGTC,29,30
...
```

Those two rows are the real top hits for `sample1` **with a length filter of
30**: 1,246 lines read, 2 skipped, 1,244 counted, and 181 unique sequences —
down from 516 unique when the same file is counted at full length (its longest
cleaned read is 151 bases). The filter is **not** part of the filename, so
re-running a sample overwrites its CSV — a full-length run replaces a filtered
one (copy the file elsewhere if you want to compare).

### Sequence search (one combined CSV)

A specific-sequence query writes a single file, `noise_results/sequencecount.csv`
(not one per sample), with the queried sequence as the column header, one row
per sample in browse order, and a `total` row. Each read is counted at most once,
even if the sequence occurs in it more than once:

```
sample,CACCAGACACGTCTGAACTCCAGTCAC
sample1,888
sample2,1552
sample3,1274
...
sample95,615
sample96,86
total,74985
```

This is the real result of that 29-base query over all 96 samples in
`counted_results`. Note that the current code always writes the fixed name
`sequencecount.csv`, **overwriting** any previous query, so keep a copy before
searching for another sequence; the file currently in `noise_results/` was saved
under an older, hand-named variant (`PP00005_noise_CACCAGACACGT_sequencecount.csv`).

The closest-match diagnostics shown on the results screen are intentionally not
written to `sequencecount.csv`; the file format and exact counts are unchanged.

## Tests

```bash
cargo test --release
```

20 unit tests (14 in `count.rs`, 6 in `scan.rs`) cover sequence cleaning, the
length filter, CSV ordering, the combined sequence-count CSV, the closest-match
diagnostics, and file discovery/sorting. They create their own temporary
folders and do not need the sample data, so `counted_results/` is not part of
the test run.

## Notes & limits

- TUI only: no arguments, no stdin, no headless/batch mode. The top-10 table's
  sequence column is sized from the terminal (clamped to 20–100 characters), so
  long reads are clipped with `…` unless the window is wide.
- Counting is exact-match only (after `N` trimming) — the Hamming-distance
  search is a display-only diagnostic for zero-hit samples.
- Reverse complements are **not** searched for; a sequence is matched as typed.
- Everything is held in memory per file (`HashMap` of sequence → count), so very
  large read files are limited by RAM, not by the streaming reader.
- Sample numbering assumes the `sampleN_unbinned_reads` naming; other names work
  but sort last and are used verbatim as the sample name.
