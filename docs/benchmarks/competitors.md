# Competitive benchmark: keyhammer and Rust alternatives (2026-09-24)

This report compares the keyhammer engine with established Rust options for typo-tolerant word lookup, on the same dictionaries and typo pairs as the M0 report (`m0.md`) plus two more typo corpora. It is part of issue #37. The JavaScript libraries (MiniSearch, Fuse, uFuzzy, fuzzysort) and the WebAssembly build are compared in a follow-up.

## Summary

- Latency: keyhammer answers in about 90 / 210 / 250 us (p50) at 10 000 / 100 000 / 274 137 terms. SymSpell is faster at every size: about 8-11x at 10 000 and 100 000 terms and 4-5x at the full dictionary (p50 47-62 us; its p95 is about 1.5-2x lower than keyhammer's there). keyhammer is about 9-23x faster than the `fst` Levenshtein automaton (a separate single-run measurement outside the harness suggests most of it is building the automaton for each query; see Notes) and 10-180x faster than the `strsim` scan and the BK-tree.
- Memory and build: keyhammer's index takes 112-187 bytes per term, about a tenth of SymSpell's (1148-1688), and builds about 40x faster (77 ms against 3.0 s at the full dictionary). It takes far more memory than the `fst` map (5-8 bytes per term) and the brute force's plain term list (33 bytes per term).
- Quality against the brute force (OSA distance at most 2, then higher weight): not distinguishable on Birkbeck and on the Wikipedia list at every size. On the GitHub Typo Corpus keyhammer is behind at 100 000 terms (-0.016 MRR, 2.6 SE) and at the full dictionary (-0.024 MRR, 4.7 SE); SymSpell re-ranked by the same rule and the BK-tree are as good as the brute force there.
- Quality against `fst` (plain Levenshtein, no transpositions): keyhammer is ahead on the GitHub and Wikipedia corpora at 100 000 and 274 137 terms (+0.026 to +0.061 MRR, 2.5 to 8.3 SE) and not distinguishable on Birkbeck.
- Quality against SymSpell in the crate's own order: keyhammer is ahead by +0.08 to +0.15 MRR at 100 000 terms and above. That gap comes from the `symspell` crate's sort order (at equal distance the rarer word comes first), not from its candidate search: re-ranked by (distance, higher count, term), SymSpell matches the brute force.
- In short: keyhammer is not the fastest option here (SymSpell is) nor the smallest (`fst` is). It trades a 4-11x slower query than SymSpell for about a tenth of its memory, and its ranking is on par with a plain distance-then-frequency ranking on two corpora and slightly behind it on the third.

## Method

### Engines

| Engine | Crate and version | Licence | Candidates | Ranking of the top 10 |
|---|---|---|---|---|
| keyhammer | `keyhammer` 0.1.0 (this repository, path dependency) | AGPL-3.0-or-later | weighted edit cost at most 32 (two ordinary edits), QWERTY costs, `tsb` on | the default `Ranking::Coarse`: cost rounded up to whole units of 16, then higher weight, then term |
| symspell | `symspell` 0.5.2 | MIT | symmetric delete, maximum dictionary edit distance 2, prefix length 7 (the crate's default), `Verbosity::All`, count threshold 0 so that weight-0 words are kept, counts = the weights | the crate's own order: distance, then ascending count (see below) |
| symspell/rerank | the same index | MIT | the same | re-sorted by (distance, higher count, term) |
| fst | `fst` 0.4.7 with the `levenshtein` feature | Unlicense OR MIT | `fst::automaton::Levenshtein` with distance 2 over an `fst::Map` from term to weight | (Levenshtein distance recomputed with `strsim::levenshtein`, higher weight, term) |
| strsim | `strsim` 0.11.1 | MIT | linear scan with `osa_distance <= 2` (terms whose length differs by more than 2 are skipped, which does not change the result) | (distance, higher weight, term) |
| bk-tree | `bk-tree` 0.5.0 | MIT | BK-tree with `strsim::damerau_levenshtein` as the metric, tolerance 2 | (distance, higher weight, term) |

Transitive dependencies are MIT, Apache-2.0, Unlicense or BSD-3-Clause (`unidecode`, used by `symspell`), plus Unicode-3.0 (`unicode-ident`, build time only). Exact versions are pinned in `bench/competitors/Cargo.lock`. Built with rustc 1.98.1, release profile.

An `fst::Set` has no weights, so an `fst::Map` with the weight as the value is used, and its matches are ranked afterwards by the same weights as the others.

The edit models differ:

- keyhammer: weighted costs (neighbouring-key substitution 8, doubled-letter insertion or deletion 8, transposition 12, other edits 16, every edit on the first query byte x1.5), budget 32. Its candidate set is therefore not "unit distance at most 2": a first-byte edit (24) plus an ordinary edit (16) is over budget, while up to four cheap edits fit. The costs are provisional and were not calibrated.
- strsim: optimal string alignment (OSA) with unit costs, the edit model the engine's costs are built on.
- symspell and bk-tree: unrestricted Damerau-Levenshtein (the BK-tree needs a true metric, which OSA is not). It differs from OSA only when an edit falls inside a transposed pair; here the BK-tree, which ranks like the brute force, returned a different top 10 on at most 18 of 1000 queries (see the overlap table).
- fst: plain Levenshtein, without transpositions: two swapped letters count as two edits.

A keyhammer configuration with unit costs was planned but not run: `CostModel` has no public constructor other than `qwerty()` and its fields are private, and this benchmark does not modify the core.

SymSpell's order: `symspell` 0.5.2 sorts suggestions with its `Ord` implementation, which compares the distance and then the count in ascending order, so among suggestions at the same distance the least frequent word comes first. The original SymSpell orders by descending count. The `symspell` row keeps the crate's order, as a user of the crate gets it; the `symspell/rerank` row sorts the same suggestions by (distance, higher count, term).

Weights: every dictionary term has a weight in 0..65535 derived from its frequency in Norvig's `big.txt` (see `bench/prepare-m0-data.mjs`); most words have weight 0. Every engine gets the same weights (SymSpell as counts).

### Data

- Dictionaries: `words-10000.tsv`, `words-100000.tsv` and `words-full.tsv` (274 137 a-z words) from `bench/fetch-data.mjs` and `bench/prepare-m0-data.mjs`, as in the M0 report.
- Birkbeck: the 300 pairs of `tests.tsv` (Birkbeck spelling error corpus, as in M0). Their correct words are in every dictionary by construction. Mostly spelling errors.
- GitHub Typo Corpus (Hagiwara and Mita, 2020): the Hugging Face mirror `chirunder/github_typo_corrections` at commit `57d581ff` (one Parquet file, 40 664 263 bytes, 353 055 edits). The mirror states no licence; the texts come from public GitHub repositories and each follows its repository's licence. Edits whose two sides have the same number of tokens and exactly one differing token, both lower-case a-z, are kept: 25 880 unique pairs, from which 1000 are sampled with a fixed seed. These are mostly keyboard slips in code comments and documentation. The mirror lacks the original corpus's typo and language labels, so a few non-typo edits remain.
- Wikipedia "Lists of common misspellings/For machines", revision 1199637275 (99 150 bytes), CC BY-SA 4.0: 3754 usable single-correction pairs, 1000 sampled with a fixed seed. These are spelling errors by design.
- For both extra corpora the correct word (at least 3 letters) is in `words-full.tsv` and the typo (at least 2 letters) is not. `bench/fetch-typo-corpora.mjs` downloads both sources at the pinned revisions, checks their SHA-256 and writes `bench/data/gtc.tsv` (SHA-256 `091dfd81ef0f13b901bfec40dc1617b74867832b3b3efec35a3cb831378c04e2`) and `bench/data/wiki.tsv` (SHA-256 `d3dc05e3a52b1a2c5c06bac4bbfcaafdbfffbe3a501bcd95d457819008b9c3f9`); the samples depend on `words-full.tsv`. Data files are never committed.
- For each dictionary size a pair is used only when its correct word is in that dictionary and its typo is not. The GitHub and Wikipedia corpora therefore have only 58 and 92 usable pairs at 10 000 terms and 380 and 400 at 100 000 terms; all 1000 at the full dictionary. Queries are lower-cased. Every engine gets the same dictionary and the same queries and returns its top 10.

### Metrics

- MRR@10 (mean of 1/rank of the correct word, 0 when it is not in the top 10), R@1 and R@10.
- Paired difference in MRR, keyhammer minus each competitor: the mean of the per-query differences of reciprocal ranks; SE = sample standard deviation of the differences / sqrt(n); 95% interval = difference +- 1.96 SE (normal approximation). A difference below 1.5 SE is called not distinguishable.
- Correctness sanity: the mean overlap of each engine's top 10 with the `strsim` brute force's top 10, and the number of identical ordered lists. keyhammer's overlap is expected to be lower, since its costs, candidate set and ranking differ; the other engines should be close to 1 apart from their edit models. The harness also counts queries that returned an error (keyhammer search errors, `fst` automaton size limits) and keyhammer searches cut short by its `max_nodes` limit: both were 0 in every configuration.
- Latency: single thread. For each configuration a warm-up pass over the queries, then every query timed individually with `Instant`, the query set repeated until at least 1000 queries are timed. Every configuration was run 3 times (the runs interleave the engines); the tables give the median of the three runs for p50, p95, p99 and throughput (queries per second over the timed loop), and the text below notes the spread. Latency covers the search and the ranking to a top 10 kept inside the engine, not the formatting of results. `fst` and the BK-tree copy the matched keys or terms into owned buffers inside the timed call, while keyhammer resolves its term ids to strings in an untimed step; this slightly favours keyhammer, negligibly at the millisecond scale of those two engines.
- Build time: one build from an in-memory list of (term, weight), including any sorting (fst, strsim) and copying of the terms. Measured once, in the quality run (the latency run rebuilds the indexes but does not report it).
- Memory: a counting global allocator wrapping `System` tracks the bytes requested by live heap allocations. The index size is the live bytes after the build minus the live bytes before it. The dictionary read from disk is allocated before and is not counted; any temporary copy an engine makes during its build is dropped before the measurement. Allocator overhead (block headers, size-class rounding), the few bytes of the engine structs outside the heap and the search scratch buffers that grow during queries are not counted. The peak of extra live bytes during the build is reported as well. For `fst` the size of the serialised automaton is also given; for SymSpell the figure is the allocation delta of its build.

### Machine

One desktop PC: AMD Ryzen 7 9800X3D (8 cores, 16 threads), 16 GB RAM, Windows 11 Pro. The latency phase ran last, after the quality, correctness and memory measurements, and took 21 minutes. Before it no compiler, cargo or linker process was running; during it the process list was sampled every 30 seconds (42 samples) and none appeared (the only matches of the filter were two idle Node.js processes present throughout). Ordinary desktop applications (a browser among them) were open, so the machine was quiet but not dedicated to the benchmark. Over the three runs, the spread of keyhammer's p50 was within 3% in every configuration but one (7%, 100 000 terms on the GitHub corpus), and within a few percent for most other rows. The widest spreads were in SymSpell rows (p50 up to about 45% apart in one configuration), `fst` on Birkbeck at the full dictionary (about 20%) and the BK-tree on Birkbeck at 100 000 terms (p95 up to about 60%). The per-run ranges are printed by the harness.

## Results

Quality and memory come from one run, latency from a later run of the same harness. Build time and bytes per term depend only on the dictionary and are repeated in each table. The literal output of the harness has the same numbers.

### Birkbeck (300 pairs)

10 000 terms: 300 of 300 pairs usable, 170 (0.567) within OSA distance 2. Latency: 300 queries x 4 = 1200 timed per run, 3 runs.

| Engine | MRR@10 | R@1 | R@10 | p50 (us) | p95 (us) | p99 (us) | queries/s | build (ms) | bytes/term |
|---|---|---|---|---|---|---|---|---|---|
| keyhammer | 0.541 | 0.527 | 0.560 | 89.1 | 123.4 | 162.1 | 10894 | 2 | 186.8 |
| symspell | 0.486 | 0.447 | 0.550 | 8.4 | 15.4 | 22.2 | 108491 | 84 | 1687.5 |
| symspell/rerank | 0.533 | 0.507 | 0.567 | 8.2 | 15.5 | 22.5 | 108738 | 84 | 1687.5 |
| fst | 0.528 | 0.503 | 0.560 | 2000.3 | 3528.8 | 3903.4 | 509 | 6 | 8.2 |
| strsim | 0.533 | 0.507 | 0.567 | 1548.0 | 2191.0 | 2495.3 | 701 | 1 | 33.2 |
| bk-tree | 0.533 | 0.507 | 0.567 | 897.2 | 1195.5 | 1394.8 | 1146 | 20 | 170.7 |

100 000 terms: 300 of 300 pairs usable, 170 (0.567) within OSA distance 2. Latency: 300 queries x 4 = 1200 timed per run, 3 runs.

| Engine | MRR@10 | R@1 | R@10 | p50 (us) | p95 (us) | p99 (us) | queries/s | build (ms) | bytes/term |
|---|---|---|---|---|---|---|---|---|---|
| keyhammer | 0.470 | 0.430 | 0.550 | 207.9 | 298.1 | 376.4 | 4927 | 25 | 143.4 |
| symspell | 0.361 | 0.300 | 0.483 | 21.2 | 90.5 | 132.5 | 32742 | 1077 | 1212.5 |
| symspell/rerank | 0.475 | 0.437 | 0.553 | 21.0 | 93.8 | 137.9 | 32355 | 1077 | 1212.5 |
| fst | 0.471 | 0.433 | 0.547 | 2124.6 | 3664.6 | 4110.8 | 478 | 51 | 6.6 |
| strsim | 0.475 | 0.437 | 0.553 | 14675.6 | 20601.8 | 21469.6 | 76 | 16 | 33.2 |
| bk-tree | 0.475 | 0.437 | 0.553 | 6184.6 | 8293.3 | 8968.8 | 168 | 384 | 170.8 |

274 137 (full) terms: 300 of 300 pairs usable, 170 (0.567) within OSA distance 2. Latency: 300 queries x 4 = 1200 timed per run, 3 runs.

| Engine | MRR@10 | R@1 | R@10 | p50 (us) | p95 (us) | p99 (us) | queries/s | build (ms) | bytes/term |
|---|---|---|---|---|---|---|---|---|---|
| keyhammer | 0.429 | 0.380 | 0.517 | 250.0 | 368.7 | 449.3 | 4186 | 77 | 112.3 |
| symspell | 0.280 | 0.210 | 0.423 | 47.4 | 249.5 | 349.7 | 13123 | 3045 | 1147.6 |
| symspell/rerank | 0.430 | 0.380 | 0.517 | 46.6 | 260.3 | 374.3 | 13201 | 3045 | 1147.6 |
| fst | 0.425 | 0.377 | 0.510 | 2619.6 | 4684.1 | 5183.5 | 385 | 85 | 4.8 |
| strsim | 0.430 | 0.380 | 0.517 | 39043.8 | 54077.6 | 56623.8 | 29 | 40 | 33.2 |
| bk-tree | 0.430 | 0.380 | 0.517 | 14914.7 | 19966.8 | 21743.4 | 70 | 976 | 172.9 |

Paired MRR differences, keyhammer minus the competitor (SE = sd / sqrt(n); verdict: "not distinguishable" when abs(difference) < 1.5 SE, "weak" between 1.5 and 1.96 SE, otherwise the sign says who is ahead; the 95% interval is difference +- 1.96 SE):

| Terms | Competitor | Difference | SE | 95% interval | abs(diff)/SE | Queries that differ | Verdict |
|---|---|---|---|---|---|---|---|
| 10 000 | symspell | +0.0550 | 0.0131 | [+0.0294, +0.0806] | 4.2 | 40 | keyhammer ahead |
| 10 000 | symspell/rerank | +0.0074 | 0.0089 | [-0.0101, +0.0249] | 0.8 | 14 | not distinguishable |
| 10 000 | fst | +0.0124 | 0.0081 | [-0.0035, +0.0283] | 1.5 | 12 | keyhammer ahead (weak: interval includes 0) |
| 10 000 | strsim | +0.0074 | 0.0089 | [-0.0101, +0.0249] | 0.8 | 14 | not distinguishable |
| 10 000 | bk-tree | +0.0074 | 0.0089 | [-0.0101, +0.0249] | 0.8 | 14 | not distinguishable |
| 100 000 | symspell | +0.1094 | 0.0155 | [+0.0790, +0.1397] | 7.1 | 79 | keyhammer ahead |
| 100 000 | symspell/rerank | -0.0042 | 0.0085 | [-0.0208, +0.0124] | 0.5 | 28 | not distinguishable |
| 100 000 | fst | -0.0002 | 0.0084 | [-0.0168, +0.0163] | 0.0 | 27 | not distinguishable |
| 100 000 | strsim | -0.0042 | 0.0085 | [-0.0208, +0.0124] | 0.5 | 28 | not distinguishable |
| 100 000 | bk-tree | -0.0042 | 0.0085 | [-0.0208, +0.0124] | 0.5 | 28 | not distinguishable |
| 274 137 (full) | symspell | +0.1486 | 0.0170 | [+0.1152, +0.1819] | 8.7 | 96 | keyhammer ahead |
| 274 137 (full) | symspell/rerank | -0.0006 | 0.0076 | [-0.0156, +0.0143] | 0.1 | 26 | not distinguishable |
| 274 137 (full) | fst | +0.0042 | 0.0080 | [-0.0114, +0.0198] | 0.5 | 25 | not distinguishable |
| 274 137 (full) | strsim | -0.0006 | 0.0076 | [-0.0156, +0.0143] | 0.1 | 26 | not distinguishable |
| 274 137 (full) | bk-tree | -0.0006 | 0.0076 | [-0.0156, +0.0143] | 0.1 | 26 | not distinguishable |

### GitHub Typo Corpus

10 000 terms: 58 of 1000 pairs usable, 55 (0.948) within OSA distance 2. Latency: 58 queries x 18 = 1044 timed per run, 3 runs.

| Engine | MRR@10 | R@1 | R@10 | p50 (us) | p95 (us) | p99 (us) | queries/s | build (ms) | bytes/term |
|---|---|---|---|---|---|---|---|---|---|
| keyhammer | 0.948 | 0.948 | 0.948 | 89.1 | 126.9 | 154.3 | 11230 | 2 | 186.8 |
| symspell | 0.922 | 0.897 | 0.948 | 8.2 | 12.9 | 15.8 | 115276 | 84 | 1687.5 |
| symspell/rerank | 0.948 | 0.948 | 0.948 | 8.2 | 13.0 | 15.6 | 115048 | 84 | 1687.5 |
| fst | 0.940 | 0.931 | 0.948 | 1943.4 | 3616.3 | 4034.9 | 535 | 6 | 8.2 |
| strsim | 0.948 | 0.948 | 0.948 | 1542.8 | 2179.5 | 2320.9 | 747 | 1 | 33.2 |
| bk-tree | 0.948 | 0.948 | 0.948 | 938.9 | 1259.7 | 1473.6 | 1150 | 20 | 170.7 |

100 000 terms: 380 of 1000 pairs usable, 358 (0.942) within OSA distance 2. Latency: 380 queries x 3 = 1140 timed per run, 3 runs.

| Engine | MRR@10 | R@1 | R@10 | p50 (us) | p95 (us) | p99 (us) | queries/s | build (ms) | bytes/term |
|---|---|---|---|---|---|---|---|---|---|
| keyhammer | 0.876 | 0.842 | 0.929 | 205.3 | 294.6 | 364.8 | 4907 | 25 | 143.4 |
| symspell | 0.765 | 0.661 | 0.924 | 25.8 | 81.0 | 118.0 | 29941 | 1077 | 1212.5 |
| symspell/rerank | 0.895 | 0.863 | 0.937 | 25.4 | 84.2 | 122.4 | 29678 | 1077 | 1212.5 |
| fst | 0.849 | 0.797 | 0.924 | 2151.7 | 3810.5 | 4189.2 | 471 | 51 | 6.6 |
| strsim | 0.892 | 0.861 | 0.934 | 14801.1 | 20687.4 | 22942.5 | 75 | 16 | 33.2 |
| bk-tree | 0.895 | 0.863 | 0.937 | 6151.7 | 8393.1 | 9110.1 | 168 | 384 | 170.8 |

274 137 (full) terms: 1000 of 1000 pairs usable, 942 (0.942) within OSA distance 2. Latency: 1000 queries x 1 = 1000 timed per run, 3 runs.

| Engine | MRR@10 | R@1 | R@10 | p50 (us) | p95 (us) | p99 (us) | queries/s | build (ms) | bytes/term |
|---|---|---|---|---|---|---|---|---|---|
| keyhammer | 0.803 | 0.731 | 0.914 | 240.1 | 372.7 | 428.4 | 4255 | 77 | 112.3 |
| symspell | 0.655 | 0.534 | 0.880 | 62.2 | 249.0 | 356.2 | 11235 | 3045 | 1147.6 |
| symspell/rerank | 0.828 | 0.761 | 0.930 | 57.6 | 245.5 | 342.4 | 11904 | 3045 | 1147.6 |
| fst | 0.777 | 0.702 | 0.905 | 2223.3 | 3830.2 | 4276.0 | 457 | 85 | 4.8 |
| strsim | 0.827 | 0.760 | 0.929 | 39404.4 | 54692.9 | 56682.5 | 28 | 40 | 33.2 |
| bk-tree | 0.828 | 0.761 | 0.930 | 15115.3 | 20628.3 | 22664.2 | 69 | 976 | 172.9 |

Paired MRR differences, keyhammer minus the competitor (SE = sd / sqrt(n); verdict: "not distinguishable" when abs(difference) < 1.5 SE, "weak" between 1.5 and 1.96 SE, otherwise the sign says who is ahead; the 95% interval is difference +- 1.96 SE):

| Terms | Competitor | Difference | SE | 95% interval | abs(diff)/SE | Queries that differ | Verdict |
|---|---|---|---|---|---|---|---|
| 10 000 | symspell | +0.0259 | 0.0147 | [-0.0029, +0.0546] | 1.8 | 3 | keyhammer ahead (weak: interval includes 0) |
| 10 000 | symspell/rerank | +0.0000 | 0.0000 | [+0.0000, +0.0000] | 0.0 | 0 | not distinguishable |
| 10 000 | fst | +0.0086 | 0.0086 | [-0.0083, +0.0255] | 1.0 | 1 | not distinguishable |
| 10 000 | strsim | +0.0000 | 0.0000 | [+0.0000, +0.0000] | 0.0 | 0 | not distinguishable |
| 10 000 | bk-tree | +0.0000 | 0.0000 | [+0.0000, +0.0000] | 0.0 | 0 | not distinguishable |
| 100 000 | symspell | +0.1110 | 0.0152 | [+0.0813, +0.1407] | 7.3 | 110 | keyhammer ahead |
| 100 000 | symspell/rerank | -0.0189 | 0.0068 | [-0.0322, -0.0056] | 2.8 | 27 | keyhammer behind |
| 100 000 | fst | +0.0266 | 0.0108 | [+0.0054, +0.0478] | 2.5 | 49 | keyhammer ahead |
| 100 000 | strsim | -0.0162 | 0.0063 | [-0.0286, -0.0039] | 2.6 | 26 | keyhammer behind |
| 100 000 | bk-tree | -0.0189 | 0.0068 | [-0.0322, -0.0056] | 2.8 | 27 | keyhammer behind |
| 274 137 (full) | symspell | +0.1477 | 0.0108 | [+0.1266, +0.1688] | 13.7 | 416 | keyhammer ahead |
| 274 137 (full) | symspell/rerank | -0.0252 | 0.0053 | [-0.0355, -0.0148] | 4.8 | 113 | keyhammer behind |
| 274 137 (full) | fst | +0.0262 | 0.0078 | [+0.0110, +0.0415] | 3.4 | 178 | keyhammer ahead |
| 274 137 (full) | strsim | -0.0242 | 0.0052 | [-0.0343, -0.0140] | 4.7 | 112 | keyhammer behind |
| 274 137 (full) | bk-tree | -0.0252 | 0.0053 | [-0.0355, -0.0148] | 4.8 | 113 | keyhammer behind |

### Wikipedia misspellings

10 000 terms: 92 of 1000 pairs usable, 91 (0.989) within OSA distance 2. Latency: 92 queries x 11 = 1012 timed per run, 3 runs.

| Engine | MRR@10 | R@1 | R@10 | p50 (us) | p95 (us) | p99 (us) | queries/s | build (ms) | bytes/term |
|---|---|---|---|---|---|---|---|---|---|
| keyhammer | 0.978 | 0.967 | 0.989 | 89.6 | 121.1 | 162.8 | 10905 | 2 | 186.8 |
| symspell | 0.955 | 0.935 | 0.989 | 8.1 | 12.8 | 15.4 | 115150 | 84 | 1687.5 |
| symspell/rerank | 0.984 | 0.978 | 0.989 | 7.9 | 12.1 | 14.7 | 119250 | 84 | 1687.5 |
| fst | 0.967 | 0.957 | 0.978 | 2090.0 | 3664.1 | 3992.4 | 458 | 6 | 8.2 |
| strsim | 0.984 | 0.978 | 0.989 | 1748.0 | 2143.6 | 2341.5 | 618 | 1 | 33.2 |
| bk-tree | 0.984 | 0.978 | 0.989 | 941.0 | 1179.7 | 1477.0 | 1107 | 20 | 170.7 |

100 000 terms: 400 of 1000 pairs usable, 393 (0.983) within OSA distance 2. Latency: 400 queries x 3 = 1200 timed per run, 3 runs.

| Engine | MRR@10 | R@1 | R@10 | p50 (us) | p95 (us) | p99 (us) | queries/s | build (ms) | bytes/term |
|---|---|---|---|---|---|---|---|---|---|
| keyhammer | 0.944 | 0.915 | 0.985 | 208.4 | 290.5 | 340.8 | 4841 | 25 | 143.4 |
| symspell | 0.863 | 0.800 | 0.963 | 20.8 | 57.2 | 104.8 | 37148 | 1077 | 1212.5 |
| symspell/rerank | 0.937 | 0.902 | 0.983 | 20.7 | 58.5 | 108.4 | 37554 | 1077 | 1212.5 |
| fst | 0.907 | 0.865 | 0.973 | 2309.8 | 4041.5 | 5046.9 | 406 | 51 | 6.6 |
| strsim | 0.937 | 0.902 | 0.983 | 17085.2 | 20678.2 | 21459.2 | 66 | 16 | 33.2 |
| bk-tree | 0.937 | 0.902 | 0.983 | 6144.2 | 8125.6 | 8739.3 | 170 | 384 | 170.8 |

274 137 (full) terms: 1000 of 1000 pairs usable, 981 (0.981) within OSA distance 2. Latency: 1000 queries x 1 = 1000 timed per run, 3 runs.

| Engine | MRR@10 | R@1 | R@10 | p50 (us) | p95 (us) | p99 (us) | queries/s | build (ms) | bytes/term |
|---|---|---|---|---|---|---|---|---|---|
| keyhammer | 0.905 | 0.855 | 0.978 | 258.4 | 384.9 | 509.4 | 3893 | 77 | 112.3 |
| symspell | 0.769 | 0.662 | 0.948 | 51.1 | 193.8 | 328.7 | 13803 | 3045 | 1147.6 |
| symspell/rerank | 0.902 | 0.850 | 0.979 | 47.8 | 190.9 | 306.2 | 14652 | 3045 | 1147.6 |
| fst | 0.845 | 0.781 | 0.954 | 2356.9 | 3952.1 | 4358.1 | 409 | 85 | 4.8 |
| strsim | 0.902 | 0.850 | 0.979 | 45273.9 | 54817.0 | 56617.1 | 25 | 40 | 33.2 |
| bk-tree | 0.902 | 0.850 | 0.979 | 15179.2 | 21044.1 | 25367.7 | 68 | 976 | 172.9 |

Paired MRR differences, keyhammer minus the competitor (SE = sd / sqrt(n); verdict: "not distinguishable" when abs(difference) < 1.5 SE, "weak" between 1.5 and 1.96 SE, otherwise the sign says who is ahead; the 95% interval is difference +- 1.96 SE):

| Terms | Competitor | Difference | SE | 95% interval | abs(diff)/SE | Queries that differ | Verdict |
|---|---|---|---|---|---|---|---|
| 10 000 | symspell | +0.0232 | 0.0143 | [-0.0048, +0.0511] | 1.6 | 6 | keyhammer ahead (weak: interval includes 0) |
| 10 000 | symspell/rerank | -0.0054 | 0.0094 | [-0.0240, +0.0131] | 0.6 | 3 | not distinguishable |
| 10 000 | fst | +0.0109 | 0.0154 | [-0.0193, +0.0411] | 0.7 | 5 | not distinguishable |
| 10 000 | strsim | -0.0054 | 0.0094 | [-0.0240, +0.0131] | 0.6 | 3 | not distinguishable |
| 10 000 | bk-tree | -0.0054 | 0.0094 | [-0.0240, +0.0131] | 0.6 | 3 | not distinguishable |
| 100 000 | symspell | +0.0813 | 0.0121 | [+0.0577, +0.1049] | 6.7 | 80 | keyhammer ahead |
| 100 000 | symspell/rerank | +0.0069 | 0.0056 | [-0.0041, +0.0180] | 1.2 | 16 | not distinguishable |
| 100 000 | fst | +0.0367 | 0.0084 | [+0.0202, +0.0532] | 4.4 | 30 | keyhammer ahead |
| 100 000 | strsim | +0.0069 | 0.0056 | [-0.0041, +0.0180] | 1.2 | 16 | not distinguishable |
| 100 000 | bk-tree | +0.0069 | 0.0056 | [-0.0041, +0.0180] | 1.2 | 16 | not distinguishable |
| 274 137 (full) | symspell | +0.1367 | 0.0097 | [+0.1176, +0.1557] | 14.1 | 335 | keyhammer ahead |
| 274 137 (full) | symspell/rerank | +0.0030 | 0.0043 | [-0.0053, +0.0113] | 0.7 | 69 | not distinguishable |
| 274 137 (full) | fst | +0.0608 | 0.0073 | [+0.0464, +0.0751] | 8.3 | 135 | keyhammer ahead |
| 274 137 (full) | strsim | +0.0030 | 0.0043 | [-0.0053, +0.0113] | 0.7 | 69 | not distinguishable |
| 274 137 (full) | bk-tree | +0.0030 | 0.0043 | [-0.0053, +0.0113] | 0.7 | 69 | not distinguishable |

### Overlap of the top 10 with the strsim brute force

Mean over the queries of the size of the intersection of the two lists divided by the length of the longer list (1 when both are empty), and the number of queries whose ordered top-10 lists are identical to the brute force's.

| Terms | Corpus | keyhammer | symspell | symspell/rerank | fst | strsim | bk-tree |
|---|---|---|---|---|---|---|---|
| 10 000 | Birkbeck | 0.842 (209/300) | 0.957 (226/300) | 0.999 (298/300) | 0.994 (290/300) | 1.000 (300/300) | 0.999 (298/300) |
| 10 000 | GitHub Typo Corpus | 0.848 (37/58) | 1.000 (41/58) | 1.000 (58/58) | 0.981 (54/58) | 1.000 (58/58) | 1.000 (58/58) |
| 10 000 | Wikipedia misspellings | 0.917 (75/92) | 0.995 (80/92) | 0.995 (91/92) | 0.981 (88/92) | 1.000 (92/92) | 0.995 (91/92) |
| 100 000 | Birkbeck | 0.775 (135/300) | 0.855 (141/300) | 0.999 (298/300) | 0.988 (277/300) | 1.000 (300/300) | 0.999 (298/300) |
| 100 000 | GitHub Typo Corpus | 0.764 (144/380) | 0.823 (146/380) | 0.998 (377/380) | 0.966 (317/380) | 1.000 (380/380) | 0.998 (377/380) |
| 100 000 | Wikipedia misspellings | 0.817 (197/400) | 0.898 (201/400) | 0.998 (394/400) | 0.970 (347/400) | 1.000 (400/400) | 0.998 (394/400) |
| 274 137 (full) | Birkbeck | 0.735 (99/300) | 0.761 (97/300) | 0.999 (299/300) | 0.988 (275/300) | 1.000 (300/300) | 0.999 (299/300) |
| 274 137 (full) | GitHub Typo Corpus | 0.750 (269/1000) | 0.755 (189/1000) | 0.997 (982/1000) | 0.950 (796/1000) | 1.000 (1000/1000) | 0.997 (982/1000) |
| 274 137 (full) | Wikipedia misspellings | 0.769 (324/1000) | 0.824 (255/1000) | 0.998 (988/1000) | 0.960 (815/1000) | 1.000 (1000/1000) | 0.998 (988/1000) |

### Build time and memory

| Terms | Engine | Build (ms) | Index heap bytes | Bytes/term | Peak extra heap during build | Note |
|---|---|---|---|---|---|---|
| 10 000 | keyhammer | 2 | 1868460 | 186.8 | 3054356 |  |
| 10 000 | symspell | 84 | 16874564 | 1687.5 | 17167201 | one index shared by both SymSpell rows |
| 10 000 | symspell/rerank | 84 | 16874564 | 1687.5 | 17167201 | one index shared by both SymSpell rows |
| 10 000 | fst | 6 | 82038 | 8.2 | 2521552 | fst size 79140 B |
| 10 000 | strsim | 1 | 332126 | 33.2 | 332126 |  |
| 10 000 | bk-tree | 20 | 1707186 | 170.7 | 1707410 |  |
| 100 000 | keyhammer | 25 | 14338152 | 143.4 | 24629072 |  |
| 100 000 | symspell | 1077 | 121252947 | 1212.5 | 121254847 | one index shared by both SymSpell rows |
| 100 000 | symspell/rerank | 1077 | 121252947 | 1212.5 | 121254847 | one index shared by both SymSpell rows |
| 100 000 | fst | 51 | 655478 | 6.6 | 6398112 | fst size 600489 B |
| 100 000 | strsim | 16 | 3322622 | 33.2 | 3322622 |  |
| 100 000 | bk-tree | 384 | 17079338 | 170.8 | 17079586 |  |
| 274 137 (full) | keyhammer | 77 | 30777678 | 112.3 | 60616990 |  |
| 274 137 (full) | symspell | 3045 | 314605160 | 1147.6 | 314605881 | one index shared by both SymSpell rows |
| 274 137 (full) | symspell/rerank | 3045 | 314605160 | 1147.6 | 314605881 | one index shared by both SymSpell rows |
| 274 137 (full) | fst | 85 | 1310838 | 4.8 | 11588288 | fst size 1032489 B |
| 274 137 (full) | strsim | 40 | 9110574 | 33.2 | 9110574 |  |
| 274 137 (full) | bk-tree | 976 | 47407978 | 172.9 | 47407978 |  |

### Notes on the results

- keyhammer and the brute force agree to within the noise at all sizes on Birkbeck. This matches the M0 addendum (keyhammer minus baseline -0.004 at the full dictionary there, -0.001 here; the small change comes from the tie-break, which is now the term in both).
- On the GitHub Typo Corpus at the full dictionary, keyhammer's R@10 (0.914) is below the brute force's (0.929): 16 queries have the correct word in the brute force's top 10 but not in keyhammer's (and 1 the other way). Checked one by one (a throwaway check outside the harness, single run): in 14 of the 16 the correct word is within keyhammer's budget but ranked out of its top 10, and all 14 edit the first letter (for example "hile" for "while", "cript" for "script"): the x1.5 first-byte factor makes such an edit cost 24, i.e. two units under the coarse ranking, so it ranks behind one-edit candidates. In the other 2 the cost is 40, over the budget of 32 (also first-letter edits). The loss in MRR is about 0.024, of which R@10 accounts for part and the ranking inside the top 10 for the rest.
- On the Wikipedia list, which is made of spelling errors, keyhammer is not distinguishable from the brute force; `fst` falls behind there partly because its automaton misses candidates that need a transposition plus another edit, and partly because this harness ranks its matches by plain Levenshtein, where a transposition such as "recieve" counts as two edits.
- `fst`'s latency is dominated by building the Levenshtein automaton for each query: in a separate throwaway measurement (single run, not part of the harness), `Levenshtein::new(query, 2)` alone took about 1.9 ms per Birkbeck query, out of about 2.0-2.6 ms in total. An application that reuses automata, or a different automaton crate, may do better; this was not tested.
- SymSpell's p95 grows faster with the dictionary than its p50 (p95 about 4-5x p50 at the full dictionary), while keyhammer's p95 stays about 1.5x its p50.
- SymSpell's index per term shrinks as the dictionary grows (1688, 1213, 1148 bytes) because delete variants are shared between terms; keyhammer's shrinks too (187, 143, 112) because trie prefixes are shared.

## Caveats

- One machine, one operating system, one compiler; latency numbers from other machines will differ, and so may the ratios.
- One dictionary of English a-z words and three English typo corpora; nothing here speaks for other languages, other alphabets or other keyboard layouts.
- Most dictionary words have weight 0, so ties are frequent and the final tie-break (term order) decides many ranks.
- `fst`'s Levenshtein automaton has no transpositions, SymSpell and the BK-tree use unrestricted Damerau-Levenshtein, the brute force OSA and keyhammer weighted costs; the "same radius of 2 edits" is the same nominal radius, not an identical candidate set.
- SymSpell's memory grows quickly with the maximum edit distance and with the prefix length; only distance 2 and prefix length 7 were measured. Its `Verbosity::All` mode is needed for a top 10 and is slower than the modes that return only the closest suggestions.
- The `symspell` crate's own order (rarer words first at equal distance) differs from the original SymSpell; both orders are reported.
- keyhammer's edit costs are provisional and were not calibrated, and a unit-cost keyhammer run was not possible without changing the core.
- Corpus limits: at 10 000 and 100 000 terms the GitHub and Wikipedia corpora have few usable pairs (58 to 400), so their intervals are wide. The GitHub mirror lacks the original typo labels, so a few non-typo edits remain. The Birkbeck and Wikipedia pairs are mostly spelling errors, which a keyboard model is not designed for; about 43% of the Birkbeck pairs are further than two edits and no engine here can find them.
- One sample per corpus with one seed; quality metrics are deterministic for that sample, but another sample would give somewhat different numbers.
- Memory is the heap bytes requested, without allocator overhead; the resident memory of a process would be higher for every engine.
- Build time is a single measurement.

## How to reproduce

From the repository root:

```sh
cd bench
npm ci
node fetch-data.mjs           # Birkbeck, big.txt (SHA-256 checked)
node prepare-m0-data.mjs      # dictionaries and tests.tsv
node fetch-typo-corpora.mjs   # gtc.tsv and wiki.tsv (pinned revisions, SHA-256 checked)
cd competitors
cargo run --release -- --help
cargo run --release -- --skip-latency   # quality, correctness, build time and memory
cargo run --release -- --latency-only   # latency; run it on an idle machine
```

The harness reads `../data` by default (`--data` and `--corpus-dir` change that) and prints Markdown tables. `--sizes`, `--corpora`, `--engines`, `--repetitions` and `--min-queries` restrict or extend a run. The tables above were assembled from its output of 2026-09-24.
