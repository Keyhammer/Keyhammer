# Serialized index format: size, load time, search on the view (issue #26), 2026-09-24

What the binary index format of `docs/design/index-format.md` costs: file size against the
in-memory trie, time to write and to load (validate) a file, and whether search on the
borrowed view gives the same results and work as search on the owned trie. One machine, a
shared one; timings are indications, the sizes and work counters are exact and deterministic.

## What was measured

- Harness: `bench/src/bin/index.rs`. Reproduce with
  `cargo run --release -p keyhammer-bench --bin index -- bench/data` (add `--latency` for the
  last four columns of the second table). Data as in `m0.md`: 10 000, 100 000 and 274 137 words
  with corpus-frequency weights, and the 300 Birkbeck misspellings of `tests.tsv` as queries
  (data not committed, see `bench/`).
- The harness asserts, for every dictionary, that `to_bytes` gives the same bytes twice and
  for a second, independent build, and that for all 300 queries the view returns the same hits
  and the same `Stats` as the owned trie, for `search` and for `search_prefix`
  (`SearchConfig::default()`, QWERTY). It would stop at the first difference; it did not.
- Sizes: the file is `to_bytes().len()`. The in-memory figure is an **estimate** of the
  trie's heap: every array's length times its element size (labels at 1 byte each, since these
  dictionaries have no label above U+00FF; the other per-node arrays at 22 bytes per node),
  plus per term the weight, the input index, the text and a 24-byte `String` header (64-bit
  target). It leaves out allocator overhead and spare capacity, so the real heap is somewhat
  larger than shown.
- Times: median of 21 runs each (`from_bytes`, `to_trie`, `to_bytes`); "build" is a single
  `Trie::build` run. "Of which CRC" is the median load time of a copy whose last byte is
  flipped: that load reads the header, checksums the whole file and stops at the checksum, so
  it is the checksum's share. Latency: three rounds over the 300 queries, the first untimed,
  owned and view searches alternated with the order swapped each round; percentiles over the
  600 timed queries per side.
- Machine: Windows 11, 16 logical processors, shared with other jobs. The load reading was
  14 to 16% just before and after the runs quoted here (it had been above 90% an hour earlier).
  Three runs of the harness with `--latency`; the numbers below are the first. In the other
  two the median timings were within 8% of these for 100 000 and 274 137 words and within
  15% for 10 000 words (times of about 1 ms), and the latency percentiles within 9%; the single-run build time ranged from 67 to 77 ms (274 137 words). Sizes and
  work counters were identical.

## Results

| Words | Nodes | File bytes | File bytes/term | In-memory heap (est.) | Heap bytes/term | File / heap |
| --- | --- | --- | --- | --- | --- | --- |
| 10000 | 53925 | 1594496 | 159.4 | 1740203 | 174.0 | 0.92 |
| 100000 | 331991 | 10554696 | 105.5 | 12222349 | 122.2 | 0.86 |
| 274137 | 606247 | 21035392 | 76.7 | 25911523 | 94.5 | 0.81 |

| Words | Build ms | to_bytes ms | from_bytes ms (validate) | of which CRC ms | to_trie ms | nodes expanded, view (exact + prefix, 300 q) | DP rows, view | owned p50 us | view p50 us | owned p95 us | view p95 us |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 10000 | 2.7 | 1.05 | 1.77 | 0.60 | 0.85 | 145171 | 553633 | 95.7 | 94.6 | 136.7 | 136.3 |
| 100000 | 26.1 | 6.73 | 13.30 | 3.58 | 6.43 | 235560 | 1132354 | 216.8 | 212.4 | 306.2 | 301.8 |
| 274137 | 76.6 | 14.99 | 31.68 | 7.16 | 16.38 | 235481 | 1236137 | 269.1 | 262.3 | 407.1 | 400.1 |

The node and row counts are those of the owned trie too (asserted per query).

## Reading

- **Size.** The file is 8 to 19% smaller than the estimated heap of the in-memory trie,
  although it stores 4 bytes per label against 1 in memory: the per-term `String` headers
  (24 bytes each) are replaced by a 4-byte offset, and the two per-node child fields (6 bytes)
  by one 4-byte offset. Per node the file has 26 bytes against 25 in memory. A 1-byte label
  section (a compact profile) would save 3 bytes per node, 1.8 MB (8.6%) of the 274 137-word
  file; not done, see the design note, section 2.3.
- **Load.** Validating the 21 MB file of 274 137 words takes about 32 ms, of which about 7 ms
  is the CRC; building the trie from the word list took 67 to 77 ms over the three runs. So
  loading is about twice as fast as rebuilding and allocates nothing; making an owned trie
  from the view (`to_trie`) adds about 16 ms. The structural checks, not the checksum,
  dominate: one pass over 606 247 nodes and one root-to-leaf walk per term. Not measured:
  which of the two passes costs more.
- **Search on the view.** Same hits and work counters as the owned trie (asserted). The
  measured latency on the view was within about 3% of the owned trie at p50 and p95, and not
  slower in these runs; this is one machine under light, varying load, so no claim beyond "no
  visible cost" is made. Hypothesis (not measured): the per-read bounds checks are cheap next to
  the DP rows, and the fixed 4-byte labels of the view avoid the branch between narrow and wide
  labels that the owned trie takes on every label read.
- **Owned path.** Its code is unchanged except that the search is now generic over a
  crate-private trait, instantiated for `Trie`. Results and work counters are identical (the
  existing `tests/ascii_regression.rs` digests, and a digest of hits and node counts over the
  300 queries times 5 rounds, exact and prefix, 274 137 words, equal on `main` and on this
  branch). Its latency was compared against `main` (commit 60c04e3) with a small program run
  20 times per side, alternating: with the default 16 codegen units this branch was about 3%
  slower at the minimum (694 against 671 ms for the 3 000 searches); built with one codegen
  unit it was about 3% faster (657 against 677 ms). The sign follows the codegen partition,
  so this is read as noise of code placement, not as a cost of the change; it is not a
  controlled measurement (the load reading was about 90% during these runs).
