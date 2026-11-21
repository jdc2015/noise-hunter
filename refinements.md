# Possible refinements

A companion to [glossary.md](glossary.md). The program's purpose is to explain
reads the sequencing pipeline rejected: it turns each unbinned list into a
frequency table so the failure shapes become legible. The items below are ordered
by how much they would add to that, starting with the one that makes the program
answer its own question rather than just present evidence.

## 1. Cross-sample pattern search — the motivating example

Today every file is counted alone. The useful finding is a sequence pattern that
appears in **all** — or most — of the 96 lists: a pattern in one unbinned list is a
curiosity, the same pattern everywhere is a cause.

- **What it would produce:** for each pattern, the samples it appears in and its
  count in each; the inverse view, a sample's patterns ranked by how widely they
  are shared.
- **It needs a definition of "pattern".** Exact sequence, a family within N
  mismatches, or a k-mer are three different features. Exact strings are cheap and
  already computed; families reuse the mismatch machinery in the search job; k-mers
  (see item 7) find shared stretches even when whole reads differ.
- **It needs a threshold.** Two reads in one sample and nothing elsewhere is not a
  finding. A starting rule: present in ≥ N samples with ≥ M reads each.
- **It needs a chance baseline.** An 8-mer occurs by chance about once per 65,536
  positions, and these reads are G-rich, so raw k-mer counts overstate significance.
  A composition-aware score is worth designing before the results are believed.

## 2. A per-sample noise profile

One row per sample with the named classes counted: reads containing index1, index2,
both, neither; reads with the 27-mer barcode; reads ending in a poly-G run; reads
matching the partner sample's swapped index pair. This is "patterns of noise" at
the sample level, and it is the table item 1 would rank. The counts are already
obtainable by hand — 1,129 of sample1's 1,246 reads contain index1 — so this is
mostly a matter of computing and reporting them.

## 3. Index-aware counting

Requires the sample sheet (or the 96 pairs embedded). Then each read can be asked:
does it contain its own sample's index, another sample's, both indices, and at what
positions? Turning the hand measurement into a column is the single cheapest step
toward the noise profile in item 2.

## 4. Swapped-index errors as a counted class

The sheet lists every pair in both orders for partner samples, and `bcl-convertor`
enforces order, so swapped reads are candidates for the unbinned list. Counting them
as *swapped-index-error* needs both indices in the read — and index2 sits far
downstream, beyond a 151-base read — so this may only be detectable on longer reads
or on the index reads themselves. Worth scoping before building.

## 5. Poly-G handling

- **Report effective read length** per sample: where the G-run begins. A histogram
  of that shows how much of each file is padding, which is a direct measure of short
  molecules.
- **Optional trimming** before counting, so padded reads collapse to their real
  sequence. Today two molecules with the same padding but different real lengths
  stay distinct strings, which inflates the unique count.

## 6. Barcode check against `probe_table.csv`

With the probe table: does a read carry the readA barcode, the readB barcode, both,
or neither? Which variants appear in the unbinned reads, and is one barcode failing
systematically? This is the closest the program can come to explaining the binner's
decisions without reimplementing the binner.

## 7. K-mer mode

Count k-mers (say 12–20 bases) instead of whole reads, so shared patterns surface
even when full reads differ. Cheap, and the natural engine for item 1.

## 8. Batch / headless mode

The TUI and GUI are interactive; 96 samples times several analyses is a batch job. A
mode that walks a folder and writes the noise profile as one CSV would make items 1
and 2 reproducible and scriptable. The original build plan excluded headless mode;
the cross-sample work is a good reason to revisit that.

## 9. Reporting

One tidy CSV per run — sample, class, count, share — so the numbers can go into a
spreadsheet or a statistics tool without re-parsing `noisecounts_*.csv`.

## 10. UI refinements

- Filter or hide poly-G reads in tables and charts.
- A minimum-count threshold for the charts.
- Sort samples by a noise class, once item 2 exists.
- Show a selected read's layout — barcode, index, insert, padding — beside the
  sequence, so a table row explains itself.

## Deliberately not proposed

UMI collapsing, alignment, variant calling, FASTQ quality handling. Those belong to
other tools, and part of this program's value is that it does not do them.

## Design questions to settle first

- **What is a "pattern"?** Exact, family, or k-mer — item 1 cannot be built without
  choosing.
- **Is a pattern's significance absolute or relative?** Raw counts are easy to read
  and easy to fool in G-rich data.
- **Where do index and barcode checks live?** In `noise_core`, so both front ends get
  them, or in a separate analysis step that reads the CSVs?
