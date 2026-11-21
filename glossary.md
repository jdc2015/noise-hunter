# Glossary

## What this repository is for

`noise-hunter` exists to explain rejected reads. It takes the reads the sequencing
pipeline could not bin — one plain-text list per sample, read 1 only — and turns
each list into a frequency table of identical sequences. Those reads failed for
reasons that have shapes: an index found inside the read, a swapped index pair, a
short molecule padded with `G`s, or no 27-base allele-locus barcode at all. A
frequency table makes the shapes legible, and it is the evidence for the only
question worth asking of them: why did these reads fail, and do the failures
recur?

Two jobs do that. **Duplicate counting** collapses a list into a frequency table —
how many reads are exactly the same cleaned sequence — optionally after a length
filter that truncates long reads so near-duplicates fall together. **Sequence
search** asks how much of each list contains a sequence you type, across every
sample at once, optionally on either strand and optionally allowing mismatches,
which turns "is this family here, and how big is it" into one number per sample.

The repository holds a shared, dependency-free core (`noise_core`) and two front
ends over it — a terminal UI (`noise_counter`) and a desktop GUI (`noise_gui`) —
which write the same plain CSVs into a sibling `noise_results/` folder, checked
byte for byte against a checksum manifest.

**The next step, not yet built, is comparison.** A pattern in one unbinned list is
a curiosity; the same pattern in every sample is a finding. The program today
measures each file on its own, and comparing them — which sequences recur, how
widely, and whether a shared pattern points at one cause — is the work this purpose
points toward. See `refinements.md`.

What it deliberately does **not** do: parse the sample index or the 27-base
allele-locus barcodes, assign reads to variants, read or write the binned files,
collapse PCR duplicates by UMI, align reads, or look at FASTQ qualities. It
treats each line as an opaque sequence and counts strings.

## The pipeline, before this program

Reads arrive here through two upstream steps, neither of them part of this
repository:

1. **Sequencing** — one machine run produces 25 million to 2 billion reads. The
   run has a unique name; `PP00005` is one.
2. **`bcl-convertor`** — reads the two short index reads for each cluster, matches
   them against a **sample sheet** listing each sample's pair, and writes one
   FASTQ file set per sample, named `sample1`, `sample2`, … That labelling of
   read 1 output is its job, and it happens upstream of everything here.
3. **`fastq-binner`** — reads each sample's FASTQ and tries to assign each read to
   a variant through its allele-locus barcodes. Reads it can assign become a
   `*_binned_results.csv` count table; reads it cannot — the read carries some
   error — are written out as `sampleN_unbinned_results.csv`.

This program starts where step 3 leaves off: it takes the unbinned reads and
counts them, looking for the pattern in what failed.

## Data and samples

- **Sequencing run** — one run of the machine, 25 million to 2 billion reads,
  identified by a unique name such as `PP00005`.
- **`bcl-convertor`** — the demultiplexing step (Illumina's `bcl-convert`): turns
  the run into FASTQ files, one set per sample, using the sample sheet.
- **Sample sheet** — the file pairing each sample name with its two 8-base
  indices. The source of truth for demultiplexing.
- **Demultiplexing** — sorting a run's reads into per-sample FASTQ files by their
  indices. Done by `bcl-convertor`, not here.
- **FASTQ** — the per-sample read format `bcl-convertor` writes. This repository
  never reads FASTQ; its input is plain text, one sequence per line.
- **Read** — one line of a `*_unbinned_reads` file. Nothing else about it is
  known to this program: no quality scores, no pairing, no coordinates. A line
  that is entirely `N` is not a read — sample1 has 1,246 lines and 1,244 reads —
  which is why counts are quoted against usable reads.
- **`sampleN_unbinned_results.csv`** — `fastq-binner`'s output for one sample: the
  reads it could not assign. This is the file the pipeline produces.
- **`*_unbinned_reads`** — this repository's input: the same reads, stored as
  plain text with one sequence per line and no header, named with the
  `_unbinned_reads` suffix. That suffix is what the program matches — the
  upstream `.csv` name is **not** recognised — and a "sample file" is one of
  these. "Unbinned" is literal, not a stage.
- **`*_binned_results.csv`** — `sample,variant,readA,readB`: a **count table of the
  27-base prefix per variant, with no sequences in it**, written by
  `fastq-binner`. **Not read or written by this repository**; it sits beside the
  reads and is why listings filter on the `_unbinned_reads` suffix.
- **Read 1 / Read 2** — the two reads of a pair. **Read 1 is the only read this
  pipeline counts**: `bcl-convertor` labels read 1 output by the index reads, and
  `fastq-binner` counts the 27-base sequences in read 1.
- **`fastq-binner` / binner** — the upstream program that assigns reads to
  variants. Its successes are the binned count tables; its failures are
  `sampleN_unbinned_results.csv`, the reads this program examines. Not part of
  this repository.
- **Allele-locus barcode** — a **27-base sequence** that `fastq-binner` counts in
  read 1. Each variant has **two**, one counted as `readA` and one as `readB`, and
  they are defined in `probe_table.csv`. This is **not** the 8-base sample index:
  the barcode is counted from read 1, the index is a separate short read used for
  demultiplexing.
- **`probe_table.csv`** — the file `fastq-binner` reads to know each variant's two
  27-base allele-locus barcodes. Not in this repository, and nothing here reads it;
  it is what makes `readA`/`readB` meaningful in the binned tables.
- **Sample / `sampleN`** — one **selection bin**, i.e. one population that was
  sorted or selected and then sequenced. In the binned files the column is also
  called `sample`, so upstream uses the same word for the same thing.
- **Sample name** — the input file name minus `_unbinned_reads`
  (`sample1_unbinned_reads` → `sample1`). Used verbatim in every CSV, and as the
  row label in both front ends.
- **Sample index** — one of the two 8-base indices that together identify a
  sample. The indices are **separate short reads**, not part of read 1, and
  `bcl-convertor` uses them to label each read 1 output as `sampleN`; that is
  upstream of this program. The same 8-mer also appears inline in read 1:
  **index1's usual slot is base 28**, immediately after the 27-base prefix, and
  **index2 sits much further downstream, beyond these reads**. **Finding either
  index anywhere in a read is a noise signature** — "anywhere" meaning any
  position, including that usual slot. Measured with index1 in that slot: sample1
  966 of 1,244 reads, sample2 1,692 of 2,119, sample8 337 of 914. On the
  anywhere-counts rule most reads in these files are index-positive: 1,129 of
  sample1's 1,244 contain index1, none contain index2, and 115 contain neither.
- **Index pair / swapped pair** — a sample's two indices together identify it. In
  the sheet, for every `i` the partner sample `97 − i` lists the same two 8-mers
  in the **opposite order** (`sample48`/`sample49`, `sample1`/`sample96`), and all
  96 rows pair this way. A swapped pair is a **manufacturing error**: it should be
  rare, and worth counting. `bcl-convertor` checks index order, so a swapped read
  is not silently demultiplexed as the partner sample — which is exactly why such
  reads are candidates for the unbinned list.
- **Swapped-index error** — a read whose two indices appear in the opposite order
  to its sample's pair. Expected to be rare; not yet counted by anything here. It
  is one of the named noise classes this program is meant to quantify.
- **Constant prefix** — the **27** bases that construct-derived reads begin with
  (`CACCAGACACGTCTGAACTCCAGTCAC`), the sequence in the README's worked example.
  (The README called it 29-base; the string is 27 characters, and the glossary is
  the corrected record.) It is the `readA` allele-locus barcode for the variants in
  this sample set — the same 27-mer in every sample checked (samples 1, 2, 3 and
  40 all lead with it). "Constant" is therefore a fact about this sample set, not
  about all variants. Only a subset of a sample's reads carry it: 888 of sample1's
  1,244, 286 of sample40's 1,436. That subset is exactly what the worked-example
  search counts, which is why the query and the prefix are the same string.
- **Read layout** — as measured in this sample set: `[27-base prefix][index1]
  [insert]`, most reads 151 bases long, and — when the molecule ended early — a
  poly-G run filling the rest. In sample1 the prefix starts at base 1, index1 at
  base 28, and 966 of 1,244 reads place index1 there. Nothing enforces this
  layout; it is an observation about the data.
- **Poly-G read** — a **short read**: the molecule ended early (say at base 55)
  and the sequencer filled the rest with `G`s to reach the programmed length of
  151. The G-run is padding, not sequence — the read's real content ends where it
  starts, and it can be trimmed or ignored. It is common here: 1,134 of sample1's
  1,244 reads, 1,998 of sample2's 2,119, 112 of sample96's 115. Nothing in this
  code trims it; the reads are counted as they are. It is not a fringe case: the
  median effective length in sample1 is **65 bases**, so half its reads carry no
  more than 65 bases of real sequence.
- **Background read** — a read that does not begin with the construct prefix
  (the majority of sample40: 1,150 of 1,436). Where these come from is **not yet
  known**, and they are not assumed to be contamination. Identifying and counting
  them is one of the reasons this program exists: a prefix search counts reads
  without the prefix rather than discarding them.
- **Variant / `readA` / `readB`** — columns of the binned files. `readA` and
  `readB` count the variant's two 27-base allele-locus barcodes in the sequencer's
  read 1 output; the barcodes come from `probe_table.csv`.
- **Index read** — the short read carrying one index, separate from read 1. There
  are two per cluster (index1, index2), and `bcl-convertor` uses them to label
  read 1 output as `sampleN`.
- **Noise** — the thing being hunted, in named classes: an **index found in the
  read**, at any position (index1 or index2), **swapped-index errors**, **poly-G
  short reads**, and reads that do not carry the 27-base allele-locus barcode.
  None of these is computed by this code yet — the counts are what you read them
  from.
- **Noise pattern** — a recurring shape in a list's frequency table: a family of
  near-identical sequences, the error tail around an abundant one, or a set of
  reads that share a defect. Reading these patterns is the program's purpose;
  comparing them between files is planned, not built.
- **`counted_results/`** — the committed sample set: 96 samples, each with its
  reads and its binned CSV. The golden tests read from here.
- **`legacy_results/`** — one preserved CSV from an older, hand-named run. Kept
  for reference; nothing reads it.
- **`PP00005`** — the unique name of the sequencing-machine run that produced all
  the reads. It survives in one hand-named legacy CSV; nothing in the program
  parses it.

## Counting duplicates (job 1)

- **Duplicate count** — how many reads in a sample are exactly the same cleaned
  sequence. Not UMI-collapsed and not error-corrected: identical strings only.
- **Cleaned sequence** — a read after: trimming surrounding whitespace, stripping
  trailing `N`/`n` bases, and uppercasing. Internal `N`s, `X`, spaces and any
  other non-`A/C/G/T` character make the line invalid.
- **Skipped** — a line that is empty after cleaning, or still contains anything
  but `A/C/G/T`. Reported per sample and in the totals; never counted.
- **Read / used / skipped / unique** — the four figures per sample: lines in the
  file, lines that became sequences, lines rejected, and distinct cleaned
  sequences.
- **Length filter** — an optional `N`: sequences longer than `N` are truncated to
  their first `N` bases before counting, collapsing near-duplicates. Shorter
  reads keep their length, so one CSV can hold mixed lengths. `0` is rejected;
  blank means full length. It is **not** part of the output file name.
- **`noisecounts_<sample>.csv`** — one per sample, header
  `sequence,counts,length`, ordered by count descending with ties alphabetical.
- **Totals** — the sums across a run (`file_count`, `total_lines`, `total_used`,
  `total_skipped`, `total_unique`). Shown in the GUI; the TUI has a totals
  screen.

## Searching for a sequence (job 2)

- **Query / target sequence** — the sequence you type. Trimmed and uppercased;
  anything but `A/C/G/T` is rejected. There is no minimum length, despite the
  GUI hint suggesting 20 bases.
- **Containment** — a read counts as a hit when the query appears anywhere inside
  it. A read counts at most **once** per sample, however many times the query
  occurs in it.
- **Hit / exact** — a read containing the query as typed. The `exact` number is
  unchanged by any option.
- **Reverse complement (rc)** — the query read on the opposite strand: `A↔T`,
  `C↔G`, reversed. Searching it too is opt-in (GUI only).
- **Combined** — reads matching either the query or its reverse complement,
  counted once. Without rc it equals `exact`.
- **Mismatch tolerance / `d`** — allow a window to differ from the query in up to
  `d` positions. `0` means exact. Opt-in, GUI only.
- **Hamming distance** — the number of positions at which two equal-length
  sequences differ; the only distance this code uses.
- **Sliding window** — every `query.len()`-wide slice of a read. Tolerance mode
  compares the query against each one.
- **Nearest window / closest match** — the read window with the smallest Hamming
  distance, with the full read it came from. Recorded while a sample has **zero**
  exact hits, as a diagnostic; never written to the CSV. In tolerance mode the
  best distance is also reported for samples that do have hits.
- **`best_distance` / `best_mismatches`** — the smallest distance seen in a
  sample, over both strands when rc is on. `None` when every read is shorter than
  the query.
- **`within_<d>`** — reads whose best window is within `d` mismatches.
- **`sequencecount.csv`** — one combined file: `sample,<SEQ>`, one row per sample
  in browse order, then `total`. Options add columns; see the table below.
- **Total row** — the last row: sums of the per-sample columns. It has no single
  best distance, so that field is left empty.

| Mode | Header |
|---|---|
| default | `sample,<SEQ>` |
| reverse complement | `sample,<SEQ>,<SEQ>_rc,combined` |
| mismatch tolerance `d` | `sample,<SEQ>,within_<d>,best_mismatches` |
| both | the union |

## Output

- **`noise_results/`** — the output folder, always a **sibling** of the input
  folder, never inside it and never chosen separately.
- **Fixed names** — outputs are never timestamped or suffixed, so a run replaces
  earlier output. The TUI overwrites silently; the GUI lists what would be
  replaced and asks first.
- **Overwrite guard** — the GUI's pre-run confirmation. It exists because fixed
  names mean an rc run would otherwise destroy an exact-only result with a
  different header.
- **Export** — the GUI's `Export CSV…`: writes the table on screen to a path you
  choose, through the same writer a run uses, so the bytes match.

## Program and UI

- **Workspace / crate** — one Cargo workspace, three crates: `noise_core`,
  `noise_counter`, `noise_gui`.
- **`noise_core`** — all counting, search and file-discovery logic; std-only, no
  dependencies. Both front ends call it, which is what keeps their output
  identical.
- **TUI / `noise_counter`** — the terminal front end (ratatui + crossterm). Keys
  only; no flags.
- **GUI / `noise_gui`** — the desktop front end (egui / eframe). Mouse-driven,
  with `--folder` and `--renderer` as its only flags.
- **Browse & count / Search sequence / Results** — the GUI's three tabs, in
  order, bound to `1`, `2`, `3`.
- **Live counts** — the GUI's per-sample counts for the query currently typed,
  recomputed as inputs change and **never written to disk**. Paused while
  mismatch tolerance is on, because a tolerant scan costs every selected sample;
  that mode runs only on an explicit **Run**.
- **Run / job** — one start of a job: a count run (writes one CSV per sample) or a
  search run (writes `sequencecount.csv`). One job at a time; live scans are not
  jobs and never overlap each other either.
- **Selection** — which sample files a run covers. Opening a folder selects
  everything; checkboxes, `Select all`, `Select none` and `Select filtered`
  adjust it. Typing in the filter box never unselects.
- **Filter box** — narrows the sample list by file name or sample name. Not to be
  confused with the **length filter**, which changes what is counted.
- **Natural order** — numeric ordering of sample names, so `sample2` sorts before
  `sample10` rather than after it.
- **Results views** — `Counts` (one row per sample from a count run), `Counts for
  query` (per-sample search results), `Sequences` (one sample's
  `noisecounts_*.csv`, first 5,000 rows).
- **Charts** — per-sample counts (largest 24), a distribution histogram, and
  used-versus-skipped per sample. They read the same in-memory results as the
  tables, so a bar and a row cannot disagree; they scroll inside a bounded
  region so the table always keeps its height.
- **Worker thread / rayon** — all counting happens off the UI thread, with files
  scanned in parallel and their results put back into browse order.
- **Progress bar** — driven only by explicit runs; a live scan finishes in a
  fraction of a second and must never disturb it.
- **Notice / message** — an in-window line with a severity (info, warning,
  error). Errors are never printed to a console, because a GUI launched from a
  launcher has none.

## Verification and build

- **`baseline.sha256`** — 193 checksums of v0.4.0 output (96 samples at full
  length, 96 at length 30, one combined search). The regression contract.
- **Golden test** — the `#[ignore]`d test that re-runs both jobs over the real
  samples and compares every CSV against `baseline.sha256`; run with
  `--ignored`. There is one in `noise_core` and one that goes through the GUI's
  parallel path.
- **Fixture** — a throwaway sample folder built by a test, with its sibling
  `noise_results/`, so unit tests need no real data.
- **Layout test** — runs the real Results tab headlessly at two window sizes and
  fails if its content spills past the panel, which is how a clipped table is
  caught without a human looking.
- **Release profile / LTO** — one codegen unit and link-time optimisation, set
  once in the workspace root; it makes release builds slow to link and fast to
  run.
- **`APP_VERSION`** — the TUI's own version constant, shown in its header. The
  crates inherit `version` from the workspace; this constant is bumped with it.

## Easy to confuse

| This | Not this |
|---|---|
| **sample** — one selection bin | a sample *index* (the 8-base barcode inside reads) |
| **counts** — number of identical reads | molecules (nothing is UMI-collapsed) |
| **exact** — query as typed | **combined** (either strand), **within_<d>** (tolerant) |
| **nearest window** — a diagnostic string, not written | **within_<d>** — a written column |
| **filter box** — narrows the file list | **length filter** — truncates sequences |
| **`counted_results/`** — input samples | **`noise_results/`** — generated output |
| **live counts** — in memory, never written | a **run**, which writes CSVs |
| **background read** — lacks the construct prefix, still counted | **skipped line** — invalid characters, never counted |
| **`*_binned_results.csv`** — upstream count table, no sequences | **`*_unbinned_reads`** — reads, this program's input |
| **`sampleN_unbinned_results.csv`** — the binner's name for them | **`sampleN_unbinned_reads`** — the name this program matches |
| **index1 at base 28** — its usual slot, still a signal | **index1 anywhere else** — also a signal |
| **allele-locus barcode** — 27 bases counted from read 1 | **sample index** — 8-base short read used to demultiplex |

## Open questions

These are the places where the glossary says "not yet known" on purpose. They are
the questions the definitions above are waiting on.

- **Which 27-mer is `readB` here?** `probe_table.csv` would name it; that file is
  not in this repository, and no read checked carries a second 27-mer in the
  readA slot.
- **Where do reads without the construct prefix come from?** Unknown. They are
  counted rather than filtered, but they are not yet explained; in sample40 they
  are the majority.
- **Do the index positions cluster?** Most index1 hits sit at base 28. Whether that
  is one mechanism (index read through into read 1) or coincidence is not measured.

## Planned, not designed

- **Counting index-positive reads.** Flagging every read that contains index1 or
  index2 at any position, as a named noise class, is the most direct use of the
  definition above — and is not implemented.
- **Counting swapped-index errors.** The sheet's partner samples list the same two
  indices in the opposite order; `bcl-convertor` rejects out-of-order pairs, so
  those reads are candidates for the unbinned list. Counting them as
  *swapped-index-error* is a named class to implement, not something measured yet.
- **Cross-file noise patterns.** The same pattern may appear in more than one
  unbinned list. Comparing them — which sequences recur across files, and whether
  a shared pattern points at a common cause — is a feature still to be planned.
  Nothing in the code does it today: each file is counted on its own.

