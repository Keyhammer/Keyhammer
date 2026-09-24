# Prefix mode: work per query against the exact mode (issue #30), 2026-09-24

`Searcher::search_prefix` matches the query against the start of terms (see
`docs/design/prefix-mode.md`). This note measures deterministic work counters
only. No latency was measured.

## What was measured

- Harness: `bench/src/bin/prefix.rs`. Reproduce with
  `cargo run --release -p keyhammer-bench --bin prefix -- bench/data` (same data as
  `m0.md`: 10 000, 100 000 and 274 137 words with corpus-frequency weights; data not
  committed, see `bench/`). The output is deterministic: two runs were byte-identical.
- Queries: the 300 Birkbeck misspellings of `tests.tsv`, each cut to its first 3, 4, 5
  or 6 letters (a query is only cut if it is longer than that; the typo may or may not lie
  inside the kept part). Only Birkbeck was used; the Wikipedia corpora were not.
- Config: `SearchConfig::default()` (`k = 10`, budget 32, `Ranking::Coarse`) with `tsb` set
  explicitly, QWERTY costs.
- Columns: mean and 95th percentile of `Stats::nodes_expanded`; mean of
  `Stats::rows_computed` (banded DP rows, the expensive part of an expansion); mean of
  `Stats::nodes_pushed`; the share of queries whose intended (untypo'd) word is in the
  top 10.

## Read this first

- `nodes_expanded` is not the same unit in the two modes. A prefix search also expands
  nodes below a "settled" node without computing a DP row (they are ordered by weight only,
  see the design note), so an expansion is cheaper there. `rows_computed` counts the DP
  rows and is the fairer measure of arithmetic work; even it ignores the heap work.
  Latency was not measured, so no speed claim is made.
- "In top 10" is not a like-for-like recall figure: the exact mode is asked to match the
  whole word from a truncated query and is expected to fail; it is shown only to make clear
  that the two modes answer different questions. It is one query per pair with the intended word
  as the only accepted answer, so it also undercounts a prefix mode that returns other valid
  completions.
- One corpus (Birkbeck), one cut rule, three dictionaries. Hypothesis, not measured:
  results on other typo corpora or on real autocomplete logs may differ.

## Results

| Words | Letters kept | Mode | expanded mean | expanded p95 | rows mean | pushed mean | intended word in top 10 |
|---|---|---|---|---|---|---|---|
| 10 000 | 3 | exact, tsb on | 413.7 | 588 | 1672.9 | 442.3 | 6.0% |
| 10 000 | 3 | prefix, tsb off | 134.2 | 437 | 235.9 | 301.6 | 63.3% |
| 10 000 | 3 | prefix, tsb on | 133.8 | 437 | 234.7 | 296.1 | 63.3% |
| 10 000 | 4 | exact, tsb on | 469.4 | 625 | 1865.8 | 480.3 | 10.0% |
| 10 000 | 4 | prefix, tsb off | 194.2 | 465 | 494.9 | 479.9 | 70.0% |
| 10 000 | 4 | prefix, tsb on | 187.5 | 462 | 481.2 | 454.0 | 70.0% |
| 10 000 | 5 | exact, tsb on | 489.6 | 646 | 1949.5 | 494.7 | 16.7% |
| 10 000 | 5 | prefix, tsb off | 329.2 | 942 | 1164.1 | 617.1 | 71.7% |
| 10 000 | 5 | prefix, tsb on | 284.7 | 783 | 1085.8 | 537.2 | 71.7% |
| 10 000 | 6 | exact, tsb on | 495.0 | 656 | 1975.8 | 498.5 | 24.3% |
| 10 000 | 6 | prefix, tsb off | 652.8 | 1042 | 1976.2 | 758.2 | 72.0% |
| 10 000 | 6 | prefix, tsb on | 500.9 | 793 | 1739.3 | 594.5 | 72.0% |
| 100 000 | 3 | exact, tsb on | 127.4 | 272 | 1260.7 | 605.3 | 4.0% |
| 100 000 | 3 | prefix, tsb off | 91.6 | 251 | 118.8 | 241.5 | 36.7% |
| 100 000 | 3 | prefix, tsb on | 91.6 | 251 | 118.8 | 240.0 | 36.7% |
| 100 000 | 4 | exact, tsb on | 308.2 | 777 | 2507.6 | 798.3 | 7.7% |
| 100 000 | 4 | prefix, tsb off | 108.1 | 311 | 400.8 | 412.2 | 46.3% |
| 100 000 | 4 | prefix, tsb on | 106.3 | 301 | 396.0 | 398.4 | 46.3% |
| 100 000 | 5 | exact, tsb on | 666.5 | 1254 | 3877.2 | 962.4 | 14.3% |
| 100 000 | 5 | prefix, tsb off | 181.4 | 437 | 780.1 | 660.1 | 50.0% |
| 100 000 | 5 | prefix, tsb on | 168.0 | 404 | 751.3 | 614.1 | 50.0% |
| 100 000 | 6 | exact, tsb on | 890.6 | 1378 | 4456.7 | 1019.3 | 23.7% |
| 100 000 | 6 | prefix, tsb off | 412.0 | 1536 | 2014.5 | 957.8 | 58.0% |
| 100 000 | 6 | prefix, tsb on | 333.8 | 1120 | 1853.7 | 807.0 | 58.0% |
| 274 137 | 3 | exact, tsb on | 87.8 | 121 | 959.7 | 667.9 | 3.0% |
| 274 137 | 3 | prefix, tsb off | 64.4 | 194 | 95.5 | 218.5 | 27.3% |
| 274 137 | 3 | prefix, tsb on | 64.4 | 194 | 95.5 | 217.6 | 27.3% |
| 274 137 | 4 | exact, tsb on | 169.2 | 380 | 1625.3 | 822.4 | 4.0% |
| 274 137 | 4 | prefix, tsb off | 81.2 | 169 | 309.4 | 347.3 | 28.7% |
| 274 137 | 4 | prefix, tsb on | 80.6 | 169 | 306.7 | 338.4 | 28.7% |
| 274 137 | 5 | exact, tsb on | 434.1 | 1087 | 3401.2 | 1035.3 | 9.3% |
| 274 137 | 5 | prefix, tsb off | 131.0 | 337 | 652.1 | 587.8 | 41.3% |
| 274 137 | 5 | prefix, tsb on | 123.0 | 302 | 632.7 | 553.6 | 41.3% |
| 274 137 | 6 | exact, tsb on | 785.3 | 1542 | 4719.3 | 1162.5 | 18.7% |
| 274 137 | 6 | prefix, tsb off | 246.3 | 667 | 1515.4 | 871.2 | 45.3% |
| 274 137 | 6 | prefix, tsb on | 210.5 | 577 | 1424.5 | 760.1 | 45.3% |

## Observations

- With `tsb` on, prefix mode computes fewer DP rows than exact mode on every one of the 12 (dictionary,
  length) cells; for example at 100 000 words and 4 letters 396.0 against 2507.6, and at
  274 137 words and 6 letters 1424.5 against 4719.3.
- Mean expanded nodes are lower in 11 of 12 cells; the exception is 10 000 words with 6 letters
  (500.9 prefix against 495.0 exact). The 95th percentile is higher in prefix mode in three cells: 10 000 words with 5 letters (783 against 646), 10 000 with 6 letters (793 against 656) and 274 137 with 3 letters (194 against 121).
- The subtree bound (`tsb`) matters more in prefix mode as queries get longer: at 274 137
  words and 6 letters it lowers expanded nodes from 246.3 to 210.5 and rows from 1515.4 to
  1424.5 (results identical, as in the tests).
- The counts are not comparable across dictionaries: the 10 000-word dictionary is built with
  all 300 intended words plus 9 700 others, so a query is far more likely to have a close
  neighbour there.
