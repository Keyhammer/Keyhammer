# The high-recall preset (`SearchConfig::high_recall()`), 2026-09-24

`SearchConfig::high_recall()` is `SearchConfig::default()` with `budget = 48` (three ordinary edits' worth of cost) instead of 32 and with the subtree bound on (`tsb = true`; `SearchConfig::default()` itself has `tsb = false`, budget 32). It is opt-in. This report measures what the preset buys and what it costs, inside this repository, with the M0 harness. It is part of issue #41. In the tables, "default" means `SearchConfig::default()` with `tsb: true` (the harness row `new+tsb`), so that the two sides differ only in the budget; the bound does not change results.

## Summary

- At every dictionary size the preset returned the right word within the top 10 more often and had a higher MRR@10 than the default with the bound on (budget 32). The paired MRR differences are +0.120 (SE 0.018) at 10 000 words, +0.082 (SE 0.015) at 100 000 and +0.064 (SE 0.013) at 274 137.
- It expands about 4.6-5.2 times as many trie nodes per query and its p95 latency is about 5.5-7.3 times higher (both with the subtree bound on, median of three runs): 0.71 ms, 2.16 ms and 2.95 ms at 10 000, 100 000 and 274 137 words, against 0.13, 0.31 and 0.40 ms for the default.
- The default stays 32. Budget 64 is still accepted (it is the engine's maximum) but is not offered as a preset.

## Method

- Harness: `bench/src/bin/m0.rs`, run as `cargo run --release -p keyhammer-bench --bin m0 -- bench/data` after the data scripts in `bench/` (data is not committed). It now also runs two extra rows, `hr` (budget 48 with the bound off) and `hr+tsb` (budget 48 with the bound on, which is what `high_recall()` returns), and prints R@10, the largest node count of one query, the number of truncated searches, and the paired difference of `hr+tsb` against `new+tsb`.
- Data: the 300 Birkbeck typo pairs of the M0 report against three dictionaries of 10 000, 100 000 and 274 137 English words (a-z only). Single corpus, single sample, one machine (16 logical cores, Windows), one build of the code.
- Both configurations use `k = 10`, `ranking = Coarse` (the default) and QWERTY costs. The comparison is the preset against the default, both with `tsb: true`. The subtree bound does not change the results (only the work), so MRR, R@10 and nodes do not depend on it for the same budget; latency does.
- MRR@10 and R@10: the reciprocal rank of the right word among the 10 returned (0 when absent), and the share of queries with the right word among them. The paired difference is the mean of the per-query differences with its standard error (sample standard deviation over the square root of 300).
- Latency: one timed search per query, p50 and p95 over the 300 queries, without warm-up. The harness was run three times in a row on an otherwise idle machine; the tables show the median of the three runs for each figure. MRR, R@10 and nodes were identical in all three runs (they are deterministic).

## Results

Quality (`tsb` on for both; identical with it off):

| words | budget | MRR@10 | R@10 | paired MRR difference vs default (SE) | 95% interval |
|---|---|---|---|---|---|
| 10 000 | 32 | 0.541 | 0.560 | | |
| 10 000 | 48 | 0.660 | 0.727 | +0.1199 (0.0175) | [+0.086, +0.154] |
| 100 000 | 32 | 0.470 | 0.550 | | |
| 100 000 | 48 | 0.552 | 0.670 | +0.0818 (0.0148) | [+0.053, +0.111] |
| 274 137 | 32 | 0.429 | 0.517 | | |
| 274 137 | 48 | 0.493 | 0.620 | +0.0639 (0.0130) | [+0.038, +0.089] |

Work and latency (`tsb` on; median of three runs; microseconds):

| words | budget | nodes/query | largest single query | p50 | p95 | p95 ratio (48 / 32) | nodes ratio |
|---|---|---|---|---|---|---|---|
| 10 000 | 32 | 491 | 788 | 92 | 128 | | |
| 10 000 | 48 | 2551 | 4750 | 517 | 706 | 5.5x | 5.2x |
| 100 000 | 32 | 911 | 1806 | 213 | 305 | | |
| 100 000 | 48 | 4610 | 11203 | 1231 | 2159 | 7.1x | 5.1x |
| 274 137 | 32 | 995 | 2033 | 272 | 401 | | |
| 274 137 | 48 | 4590 | 14679 | 957 | 2945 | 7.3x | 4.6x |

Note, without the subtree bound (row `hr`, i.e. `high_recall()` with `tsb` set to `false`; median of three): the preset expands 3562 / 6613 / 6660 nodes per query and its p95 is 806 / 2561 / 3715 us at 10 000 / 100 000 / 274 137 words, against 155 / 401 / 493 us for the default without the bound (5.2x, 6.4x and 7.5x). The bound, which the preset turns on, lowered the preset's node count by 28-31% and its p95 by about 12-21% in these runs. No search reached the node limit (`max_nodes = 100 000`); the largest single query expanded 21 764 nodes (no bound) or 14 679 (bound).

## Comparison with the figures in the issue

The issue text (from a throwaway experiment outside the repository) claimed, at 274 137 words: MRR +0.064 (SE 0.013), about 4.6x the nodes and 6.6x the p95.

- MRR difference: reproduced (+0.0639, SE 0.0130).
- Nodes: reproduced (4590 against 995 per query, 4.6x).
- p95: 7.3x in the median of three runs (single runs: 6.7x, 7.8x, 7.2x), a little above the 6.6x claimed. The issue's p95 came from a single run; the spread here between runs is about 6.7x to 7.8x, so the two are consistent within run-to-run noise, but the claim of 6.6x is not reproduced as a point value.
- The issue reports p95 of 440 us and 2895 us. Here (median of three) the default is 401 us and the preset 2945 us; different machine, so absolute times are not comparable.
- The issue's table also lists budget 64 (+0.076 MRR over 32, 9x the nodes, 24x the p95). It was not re-measured here; nothing in this repository offers or recommends it.

## Caveats

- One corpus (Birkbeck, 300 pairs), mostly spelling errors rather than finger slips; one sample, one machine, three consecutive runs of the same build. The three runs measure timing noise only, not sampling variation of the pairs.
- The 95% intervals use a normal approximation for the mean of 300 paired differences. The differences are per-query reciprocal-rank changes, and only 31 to 50 queries differ between the two configurations, so the standard errors are approximate.
- The latency is per query and includes only the search; the 300 queries are the same set for every run, and there is no warm-up.
- The larger budget widens the set of candidates within reach (more pairs can be found at all). This report compares budget 48 with budget 32 under the same cost model only; it does not test whether the keyboard-aware costs improve ranking, and nothing here should be read as saying so.
- Results on other dictionaries, other typo distributions (real finger slips, other languages) or other hardware are not measured here. Measure the preset on your own data before enabling it.
- Oracle equality at budget 48 (exact top-k identical to brute force, subtree bound on and off, both rankings) is covered by `the_high_recall_preset_matches_the_oracle` in `crates/keyhammer/tests/oracle.rs`, on small random dictionaries.
