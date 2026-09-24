# Competitive benchmark, part 2: the WebAssembly build and JavaScript libraries (2026-09-24)

This report compares the WebAssembly build of the keyhammer engine, run in Node, with the JavaScript fuzzy-search libraries MiniSearch, Fuse.js, uFuzzy and fuzzysort, on the same dictionaries and typo corpora as the Rust report (`competitors.md`). It is part of issue #45 (a follow-up of #37). The JavaScript libraries are general search libraries, ranked here by their own scoring and given no word weights; keyhammer is a word-lookup engine that gets the weights. The comparison is therefore not between identical tasks, and a variant that re-ranks the libraries' candidates with keyhammer's tie-break rule is reported separately and labelled.

## Summary

- Quality against each library's own scoring: keyhammer's MRR@10 is higher than every library's on all three corpora at all three dictionary sizes (36 comparisons, each at least 2.1 SE, all "keyhammer ahead"), and so is `keyhammer/hr`'s (each at least 2.3 SE). At the full dictionary (274 137 terms) keyhammer's MRR@10 is 0.429, 0.803 and 0.905 (Birkbeck, GitHub, Wikipedia) against MiniSearch's 0.373, 0.706 and 0.818; the paired differences are +0.0557, +0.0966 and +0.0870 (3.8, 8.6 and 8.3 SE). Fuse, uFuzzy and fuzzysort are further behind: paired differences of +0.25 to +0.55 at the full dictionary.
- That lead depends on ties and weights: the libraries get no weights. In the labelled variant that re-sorts each library's first 100 candidates by (OSA distance, higher weight, term), keyhammer is ahead in 25 of the 36 comparisons (24 clearly, 1 weakly), not distinguishable in 7 and behind in 4: against Fuse on Birkbeck at 10 000 terms (-0.1213, 5.3 SE) and 100 000 terms (-0.0569, 2.8 SE), and against MiniSearch on the GitHub corpus at 100 000 terms (-0.0145, 2.1 SE) and 274 137 terms (-0.0170, 3.0 SE). At the full dictionary against MiniSearch re-sorted, keyhammer is not distinguishable on Birkbeck (+0.0012, 0.2 SE), behind on the GitHub corpus and ahead on Wikipedia (+0.0151, 3.0 SE).
- Latency (p50 on Birkbeck at 10 000 / 100 000 / 274 137 terms): keyhammer 195.9 / 443.8 / 543.4 us, about 2.2 times the native build's p50 in the Rust report (89.1 / 207.9 / 250.0 us; a different run and harness). MiniSearch is faster at 10 000 and 100 000 terms (144.7 and 385.6 us) and slightly slower at the full dictionary (587.9 us; p95 1222.6 against keyhammer's 866.3 us). fuzzysort is faster at 10 000 and 100 000 terms but finds the word far less often. uFuzzy matches keyhammer at 10 000 terms (200.5 us) and is 9.5 times slower at the full dictionary (5149.5 us); Fuse takes 8571.8 / 86381.9 / 226256.3 us, about 44, 195 and 416 times keyhammer's p50. `keyhammer/hr` takes 1052.2 / 2406.0 / 1779.6 us.
- Build, memory and load at the full dictionary: keyhammer builds in 128 ms (MiniSearch 474, fuzzysort 60, Fuse 8; uFuzzy has no index). keyhammer's index takes 0.3 bytes per term of JavaScript heap plus a growth of the wasm linear memory of 209.9 bytes per term (an upper bound, see Method); MiniSearch takes 969.0 bytes per term of heap, fuzzysort 385.3 and Fuse 56.4 (references to the caller's strings). The module (39 218 bytes, 16 896 gzip) loads in about 1.12 ms in Node (read, compile, instantiate; median of 15 fresh processes).
- The engine loses on speed to MiniSearch and fuzzysort at 10 000 and 100 000 terms, is much slower with the high-recall budget, and builds slower than Fuse and fuzzysort; see "Where keyhammer loses".

At the full dictionary (274 137 terms), from the tables below:

| Engine | MRR@10 Birkbeck | MRR@10 GitHub | MRR@10 Wikipedia | R@1 Birkbeck | R@10 Birkbeck |
|---|---|---|---|---|---|
| keyhammer | 0.429 | 0.803 | 0.905 | 0.380 | 0.517 |
| keyhammer/hr | 0.493 | 0.811 | 0.909 | 0.430 | 0.620 |
| minisearch | 0.373 | 0.706 | 0.818 | 0.310 | 0.483 |
| fuse | 0.158 | 0.296 | 0.410 | 0.077 | 0.340 |
| ufuzzy | 0.175 | 0.541 | 0.656 | 0.150 | 0.237 |
| fuzzysort | 0.121 | 0.276 | 0.359 | 0.100 | 0.163 |
| minisearch/rerank (variant) | 0.428 | 0.820 | 0.890 | 0.380 | 0.510 |
| fuse/rerank (variant) | 0.455 | 0.738 | 0.830 | 0.417 | 0.540 |
| ufuzzy/rerank (variant) | 0.264 | 0.720 | 0.814 | 0.247 | 0.293 |
| fuzzysort/rerank (variant) | 0.156 | 0.304 | 0.381 | 0.143 | 0.170 |

Latency at the full dictionary (median of three runs):

| Engine | Birkbeck p50 (us) | Birkbeck p95 (us) | Birkbeck p99 (us) | GitHub p50 (us) | Wikipedia p50 (us) | queries/s (Birkbeck) |
|---|---|---|---|---|---|---|
| keyhammer | 543.4 | 866.3 | 1131.0 | 515.7 | 555.6 | 1912 |
| keyhammer/hr | 1779.6 | 6191.9 | 8631.4 | 1219.9 | 2189.0 | 396 |
| minisearch | 587.9 | 1222.6 | 1660.6 | 615.0 | 577.9 | 1527 |
| fuse | 226256.3 | 344883.9 | 446488.7 | 240602.3 | 275490.0 | 4 |
| ufuzzy | 5149.5 | 6346.2 | 7232.2 | 7221.8 | 7719.4 | 190 |
| fuzzysort | 1200.0 | 4293.7 | 8352.7 | 1165.3 | 1077.7 | 595 |

Build time and index memory at the full dictionary:

| Engine | build (ms) | JS heap delta (bytes/term) | wasm linear memory growth (bytes/term) |
|---|---|---|---|
| keyhammer | 128 | 0.3 | 209.9 |
| keyhammer/hr | 127 | 0.3 | 209.9 |
| minisearch | 474 | 969.0 | - |
| fuse | 8 | 56.4 | - |
| ufuzzy | 0 | 0.3 | - |
| fuzzysort | 60 | 385.3 | - |


## Method

### Engines

| Engine | Package and version | Licence | Configuration | Ranking of the top 10 |
|---|---|---|---|---|
| keyhammer | `keyhammer_wasm.wasm` from this repository (`wasm` profile: size-optimised, 39 218 bytes raw, 16 896 bytes gzip) | AGPL-3.0-or-later | `kh_search` with k = 10, budget 32 (two ordinary edits), ranking 0 (`Coarse`), QWERTY costs; the settings of the Rust report, except that the subtree bound (`tsb`) is not available through the wasm API and stays off | cost rounded up to whole units of 16, then higher weight, then term |
| keyhammer/hr | the same module | AGPL-3.0-or-later | the same with budget 48 | the same |
| minisearch | `minisearch` 7.2.0 | MIT | `new MiniSearch({ fields: ['t'] })`, `addAll`, `search(q, { fuzzy: 2, prefix: false })`; results mapped back to the dictionary term by id | MiniSearch's own score |
| fuse | `fuse.js` 7.5.0 | Apache-2.0 | `new Fuse(terms, { threshold: 0.4, ignoreLocation: true })`, `search(q, { limit: 10 })` | Fuse's own score |
| ufuzzy | `@leeoniya/ufuzzy` 1.0.19 | MIT | `new uFuzzy({ intraMode: 1 })` (one error per term: substitution, transposition, insertion or deletion), `search(terms, q, 0, 1e9)` | uFuzzy's own ranking (see below) |
| fuzzysort | `fuzzysort` 4.0.2 | MIT | targets prepared with `fuzzysort.prepare`, `go(q, prepared, { limit: 10, threshold: 0 })` | fuzzysort's own score |

`keyhammer/hr` is the wasm equivalent of `SearchConfig::high_recall()` (budget 48). The wasm interface has no function for the preset, so the budget is passed as an argument; the preset also turns the subtree bound on, which changes the work done and not the results (`recall-preset.md`). Since the wasm interface does not expose `tsb`, both keyhammer rows run without it, and their latency is not comparable with the Rust report's, which had it on.

Configuration choices, made before the measurements:

- Fuse: `threshold: 0.4` and `ignoreLocation: true`. Fuse's defaults are a threshold of 0.6 and a match that depends on the position in the string, which is not what a whole-word lookup wants. A sensitivity check is given below.
- uFuzzy: `intraMode: 1`. Its `search` ranks the matches only when there are at most `infoThresh` of them (default 1000) and otherwise returns them unranked, in dictionary order. To measure its own ranking, `infoThresh` is set to 1e9 so that it always ranks. The default setting would be faster on queries with many matches and would return them in arbitrary order; that was not measured.
- fuzzysort: `threshold: 0`, which its documentation describes as "any match" (the default is 0.5).
- MiniSearch: `fuzzy: 2`, an absolute maximum edit distance of 2, and no prefix search.

Sensitivity of two settings (a throwaway run of the same harness, quality only, one run, 10 000 terms; MRR@10 on Birkbeck, 300 pairs, and on the GitHub Typo Corpus, 58 usable pairs; the settings above are in bold):

| Setting | MRR@10 Birkbeck | MRR@10 GitHub |
|---|---|---|
| fuse, threshold 0.2 | 0.323 | 0.601 |
| fuse, threshold 0.3 | 0.442 | 0.679 |
| **fuse, threshold 0.4** | 0.483 | 0.687 |
| fuse, threshold 0.5 | 0.487 | 0.687 |
| fuse, threshold 0.6 (its default) | 0.487 | 0.687 |
| **fuzzysort, threshold 0** | 0.168 | 0.328 |
| fuzzysort, threshold 0.5 (its default) | 0.010 | 0.069 |

Fuse's MRR at 0.4 is within 0.004 of the best setting on Birkbeck and equal to it on the GitHub sample. Other settings of the libraries (for example MiniSearch's `fuzzy` fraction or uFuzzy's other intra options) were not explored, and a setting tuned on the test pairs could do better.

The libraries take no weights. Their ties are broken by their own order (for stable sorts, the order of the dictionary file, which is shuffled), while keyhammer uses the weights. This favours keyhammer whenever a library returns several equally scored candidates. The `/rerank` rows (quality only) show how much the difference matters: for each library, the first 100 candidates in its own order are re-sorted by (OSA distance, higher weight, term), the rule of the `symspell/rerank` row of the Rust report, and the top 10 of that is scored. This is a separate variant, not the library as its users would run it. It also depends on the library's candidate list and on the cap of 100.

Every engine gets the same terms (the libraries in the file's order), and in all rows a search returns the top 10 terms as JavaScript strings.

The WebAssembly module is built as `bindings/wasm/README.md` describes, with the three `--remap-path-prefix` flags, so that no build-machine path is embedded (`bindings/wasm/test.mjs` checks that, and passed with 33 checks). It is loaded with the plain `WebAssembly` API; each keyhammer row has its own instance, since a module holds one index. A query is encoded into a scratch buffer that is allocated once, `kh_search` is called, and the results text is decoded and split in JavaScript.

### Data

The same as the Rust report: `words-10000.tsv`, `words-100000.tsv` and `words-full.tsv` (274 137 a-z words, weights from Norvig's `big.txt`), the 300 Birkbeck pairs of `tests.tsv`, and the GitHub Typo Corpus and Wikipedia samples of `bench/fetch-typo-corpora.mjs`. The two sample files have the SHA-256 values that script prints, the same as in the Rust report (`gtc.tsv` `091dfd81ef0f13b901bfec40dc1617b74867832b3b3efec35a3cb831378c04e2`, `wiki.tsv` `d3dc05e3a52b1a2c5c06bac4bbfcaafdbfffbe3a501bcd95d457819008b9c3f9`). For each dictionary size a pair is used only when its correct word is in that dictionary and its typo is not, so the GitHub and Wikipedia corpora have only 58 and 92 usable pairs at 10 000 terms and 380 and 400 at 100 000 terms. Queries are lower-cased. The sources, licences and sampling of the corpora are described in the Rust report.

A check that this harness runs the same engine as the Rust one: keyhammer's MRR@10, R@1 and R@10 are identical to the `keyhammer` rows of the Rust report in all nine configurations (three corpora by three sizes), and the `keyhammer/hr` rows of Birkbeck equal the budget-48 figures of `recall-preset.md` (MRR@10 and R@10: 0.660 and 0.727 at 10 000 terms, 0.552 and 0.670 at 100 000, 0.493 and 0.620 at 274 137).

### Metrics

- MRR@10, R@1 and R@10, and the number of queries for which an engine returned nothing.
- Paired difference in MRR, as in the Rust report: the mean of the per-query differences of reciprocal ranks, SE = sample standard deviation of the differences / sqrt(n), 95% interval = difference +- 1.96 SE (normal approximation); a difference below 1.5 SE is called not distinguishable. `keyhammer` is compared with every other row and `keyhammer/hr` with the rows other than `keyhammer`.
- Latency: single thread, in one Node process, one call of the engine's `search(query)` that returns the top 10 terms as strings, timed with `performance.now()`. For keyhammer the call includes encoding the query, the wasm call, decoding the results text and splitting it into terms; for the libraries, the library call and the mapping of its results to term strings (MiniSearch and uFuzzy return ids or indexes, which are looked up in the term array). For each configuration and repetition: a warm-up of up to 200 queries (at least 20, stopping after 5 seconds), a forced garbage collection, then the query set repeated until at least 1000 queries were timed. A run stops early once it has lasted 20 seconds and has at least 100 timed queries, so that Fuse can be measured at all; the tables give the number of timed queries per run ("timed"). Three repetitions, the engines interleaved inside each repetition; the tables give the median of the three runs for p50, p95, p99 and queries per second, and the ranges of p50 and p95. A percentile is `sorted[floor(n * p)]`.
- Build time: the time to build an engine from the in-memory dictionary, the median of three builds in a fresh Node process. For keyhammer it covers serialising the terms and weights to text, compiling and instantiating the module (`WebAssembly.instantiate` on the bytes) and `kh_build`. The libraries build from the term array; uFuzzy has no index, so its "build" is creating the object.
- Index memory: `process.memoryUsage().heapUsed` after two forced collections, after minus before the first build, in a fresh Node process for each engine and size, with the term array already allocated and not counted. The libraries keep references to the caller's strings, which the harness has anyway, so the term strings themselves (801 920, 8 132 504 and 22 877 440 bytes of JavaScript heap at 10 000, 100 000 and 274 137 terms, measured by the harness) are in no library's figure. keyhammer copies the terms into its own trie in WebAssembly linear memory, and that copy is in its figure. For keyhammer the tables also give the growth of the module's linear memory (`memory.buffer.byteLength`) from before the build to after it. Linear memory does not shrink, and the growth includes the temporary buffer that holds the dictionary text and the scratch space of the build, so it is an upper bound on the size of the index, not a measurement of it. The Rust report's memory figure (bytes requested by live allocations after the build, 112 to 187 bytes per term) is not comparable with either column.
- WebAssembly load: in a fresh Node process, reading the module from local disk, `WebAssembly.compile` and `WebAssembly.instantiate`; 15 processes, median and range. This is the cost of the module in Node with V8's compiler. A browser adds the network transfer (16 896 bytes gzip) and uses its own pipeline; neither is measured here.

### Machine

One desktop PC: AMD Ryzen 7 9800X3D (8 cores, 16 threads), 16 GB RAM, Windows 11 Pro, Node.js v24.19.0 (V8 13.6.233.17-node.51), started with `--expose-gc --max-old-space-size=8192`. The latency phase ran last, after the quality, memory and load-time phases, and took 14 minutes (the quality phase before it took 27 minutes; the machine was not sampled during it). During the latency phase the process list was sampled every 30 seconds (27 samples). Besides the harness, two idle Node.js processes and Visual Studio were present throughout; in three samples one short-lived process of something else on the machine also appeared (one sample each: `python`, `cargo` with `cargo-deny`, `rustc`). So the machine was quiet but not dedicated: ordinary desktop applications, a browser among them, were open, and there was a little other activity. A second latency run, meant to check this, was abandoned because a Miri test run (`cargo miri test -p keyhammer`) by another process on the machine kept it busy for the next hour; the latency tables are therefore a single run of three repetitions. Spread over the three repetitions, as (max - min) / median of p50: keyhammer within 5% in 8 of the 9 configurations (15% on Wikipedia at the full dictionary), `keyhammer/hr` up to 35%, MiniSearch up to 16%, Fuse up to 20%, uFuzzy up to 29% and fuzzysort up to 72% (more than 5% in 6 of 9). The per-run ranges are in the tables.

## Results

Quality, and (unless noted) build time and memory, come from one run of the harness, latency from a later run. Library versions: minisearch 7.2.0, fuse.js 7.5.0, @leeoniya/ufuzzy 1.0.19, fuzzysort 4.0.2.

### Build time and index memory

Build time is the median of three builds; the heap and wasm figures come from the first build of each process. "Other buffers" is the change of `arrayBuffers` in `process.memoryUsage()` (the wasm memory is not counted there).

#### 10 000 terms

| Engine | Build (ms), median of 3 | Build range (ms) | JS heap delta (bytes) | Heap bytes/term | Other buffers delta (bytes) | Wasm linear memory growth (bytes) | Wasm bytes/term |
|---|---|---|---|---|---|---|---|
| keyhammer | 6 | 6-9 | 89936 | 9.0 | 4 | 2818048 | 281.8 |
| keyhammer/hr | 6 | 5-9 | 90192 | 9.0 | 4 | 2818048 | 281.8 |
| minisearch | 11 | 6-13 | 10209128 | 1020.9 | 0 | - | - |
| fuse | 1 | 0-2 | 661592 | 66.2 | 0 | - | - |
| ufuzzy | 0 | 0-5 | 83024 | 8.3 | 0 | - | - |
| fuzzysort | 1 | 1-2 | 3956864 | 395.7 | 0 | - | - |

#### 100 000 terms

| Engine | Build (ms), median of 3 | Build range (ms) | JS heap delta (bytes) | Heap bytes/term | Other buffers delta (bytes) | Wasm linear memory growth (bytes) | Wasm bytes/term |
|---|---|---|---|---|---|---|---|
| keyhammer | 51 | 50-59 | 89688 | 0.9 | 4 | 23003136 | 230.0 |
| keyhammer/hr | 50 | 49-58 | 89944 | 0.9 | 4 | 23003136 | 230.0 |
| minisearch | 116 | 109-123 | 95699152 | 957.0 | 0 | - | - |
| fuse | 4 | 3-5 | 5704152 | 57.0 | 0 | - | - |
| ufuzzy | 0 | 0-5 | 82896 | 0.8 | 0 | - | - |
| fuzzysort | 14 | 13-17 | 38599968 | 386.0 | 0 | - | - |

#### 274 137 (full) terms

| Engine | Build (ms), median of 3 | Build range (ms) | JS heap delta (bytes) | Heap bytes/term | Other buffers delta (bytes) | Wasm linear memory growth (bytes) | Wasm bytes/term |
|---|---|---|---|---|---|---|---|
| keyhammer | 128 | 125-145 | 89760 | 0.3 | 4 | 57540608 | 209.9 |
| keyhammer/hr | 127 | 126-143 | 89872 | 0.3 | 4 | 57540608 | 209.9 |
| minisearch | 474 | 447-482 | 265647568 | 969.0 | 0 | - | - |
| fuse | 8 | 8-11 | 15455408 | 56.4 | 0 | - | - |
| ufuzzy | 0 | 0-5 | 82872 | 0.3 | 0 | - | - |
| fuzzysort | 60 | 58-61 | 105624144 | 385.3 | 0 | - | - |

### WebAssembly load time

Module: 39218 bytes raw, 16896 bytes gzip (level 9). Each of 15 runs is a fresh Node process: read the file from local disk, WebAssembly.compile, WebAssembly.instantiate. The network transfer of a browser is not measured.

| Step | median (ms) | range (ms) |
|---|---|---|
| read the file | 0.47 | 0.44-0.85 |
| compile | 0.56 | 0.23-1.02 |
| instantiate | 0.04 | 0.03-0.05 |
| total | 1.12 | 0.74-1.53 |

### Birkbeck (300 pairs)

#### Quality

10000 terms: 300 of 300 pairs usable, 170 (0.567) within OSA distance 2.

| Engine | MRR@10 | R@1 | R@10 | queries with no result |
|---|---|---|---|---|
| keyhammer | 0.541 | 0.527 | 0.560 | 92 |
| keyhammer/hr | 0.660 | 0.623 | 0.727 | 26 |
| minisearch | 0.515 | 0.493 | 0.557 | 94 |
| minisearch/rerank | 0.528 | 0.503 | 0.560 | 94 |
| fuse | 0.483 | 0.413 | 0.623 | 5 |
| fuse/rerank | 0.662 | 0.633 | 0.710 | 5 |
| ufuzzy | 0.283 | 0.267 | 0.310 | 153 |
| ufuzzy/rerank | 0.312 | 0.310 | 0.313 | 153 |
| fuzzysort | 0.168 | 0.160 | 0.180 | 189 |
| fuzzysort/rerank | 0.186 | 0.183 | 0.190 | 189 |

100000 terms: 300 of 300 pairs usable, 170 (0.567) within OSA distance 2.

| Engine | MRR@10 | R@1 | R@10 | queries with no result |
|---|---|---|---|---|
| keyhammer | 0.470 | 0.430 | 0.550 | 57 |
| keyhammer/hr | 0.552 | 0.497 | 0.670 | 13 |
| minisearch | 0.432 | 0.387 | 0.523 | 60 |
| minisearch/rerank | 0.473 | 0.437 | 0.550 | 60 |
| fuse | 0.266 | 0.173 | 0.450 | 1 |
| fuse/rerank | 0.527 | 0.487 | 0.613 | 1 |
| ufuzzy | 0.207 | 0.180 | 0.273 | 126 |
| ufuzzy/rerank | 0.292 | 0.280 | 0.310 | 126 |
| fuzzysort | 0.141 | 0.123 | 0.170 | 156 |
| fuzzysort/rerank | 0.168 | 0.160 | 0.177 | 156 |

274 137 (full) terms: 300 of 300 pairs usable, 170 (0.567) within OSA distance 2.

| Engine | MRR@10 | R@1 | R@10 | queries with no result |
|---|---|---|---|---|
| keyhammer | 0.429 | 0.380 | 0.517 | 43 |
| keyhammer/hr | 0.493 | 0.430 | 0.620 | 9 |
| minisearch | 0.373 | 0.310 | 0.483 | 45 |
| minisearch/rerank | 0.428 | 0.380 | 0.510 | 45 |
| fuse | 0.158 | 0.077 | 0.340 | 1 |
| fuse/rerank | 0.455 | 0.417 | 0.540 | 1 |
| ufuzzy | 0.175 | 0.150 | 0.237 | 118 |
| ufuzzy/rerank | 0.264 | 0.247 | 0.293 | 118 |
| fuzzysort | 0.121 | 0.100 | 0.163 | 145 |
| fuzzysort/rerank | 0.156 | 0.143 | 0.170 | 145 |

Paired MRR differences, keyhammer minus the competitor (SE = sd / sqrt(n); verdict: "not distinguishable" when abs(difference) < 1.5 SE, "weak" between 1.5 and 1.96 SE, otherwise the sign says who is ahead; the 95% interval is difference +- 1.96 SE). `keyhammer/hr` is compared with the JS libraries and `keyhammer` with everything else.

| Terms | Reference | Competitor | Difference | SE | 95% interval | abs(diff)/SE | Queries that differ | Verdict |
|---|---|---|---|---|---|---|---|---|
| 10000 | keyhammer | keyhammer/hr | -0.1199 | 0.0175 | [-0.1542, -0.0855] | 6.8 | 50 | keyhammer behind |
| 10000 | keyhammer | minisearch | +0.0260 | 0.0101 | [+0.0062, +0.0458] | 2.6 | 23 | keyhammer ahead |
| 10000 | keyhammer | minisearch/rerank | +0.0124 | 0.0081 | [-0.0035, +0.0283] | 1.5 | 12 | keyhammer ahead (weak: interval includes 0) |
| 10000 | keyhammer | fuse | +0.0572 | 0.0266 | [+0.0051, +0.1093] | 2.1 | 117 | keyhammer ahead |
| 10000 | keyhammer | fuse/rerank | -0.1213 | 0.0229 | [-0.1661, -0.0765] | 5.3 | 71 | keyhammer behind |
| 10000 | keyhammer | ufuzzy | +0.2579 | 0.0251 | [+0.2086, +0.3071] | 10.3 | 92 | keyhammer ahead |
| 10000 | keyhammer | ufuzzy/rerank | +0.2289 | 0.0248 | [+0.1803, +0.2775] | 9.2 | 78 | keyhammer ahead |
| 10000 | keyhammer | fuzzysort | +0.3724 | 0.0290 | [+0.3155, +0.4293] | 12.8 | 129 | keyhammer ahead |
| 10000 | keyhammer | fuzzysort/rerank | +0.3544 | 0.0291 | [+0.2974, +0.4114] | 12.2 | 123 | keyhammer ahead |
| 10000 | keyhammer/hr | minisearch | +0.1459 | 0.0190 | [+0.1087, +0.1830] | 7.7 | 68 | keyhammer ahead |
| 10000 | keyhammer/hr | minisearch/rerank | +0.1323 | 0.0183 | [+0.0965, +0.1681] | 7.2 | 57 | keyhammer ahead |
| 10000 | keyhammer/hr | fuse | +0.1771 | 0.0231 | [+0.1318, +0.2223] | 7.7 | 117 | keyhammer ahead |
| 10000 | keyhammer/hr | fuse/rerank | -0.0014 | 0.0170 | [-0.0348, +0.0320] | 0.1 | 53 | not distinguishable |
| 10000 | keyhammer/hr | ufuzzy | +0.3778 | 0.0270 | [+0.3248, +0.4307] | 14.0 | 141 | keyhammer ahead |
| 10000 | keyhammer/hr | ufuzzy/rerank | +0.3488 | 0.0271 | [+0.2956, +0.4019] | 12.9 | 127 | keyhammer ahead |
| 10000 | keyhammer/hr | fuzzysort | +0.4923 | 0.0283 | [+0.4368, +0.5478] | 17.4 | 173 | keyhammer ahead |
| 10000 | keyhammer/hr | fuzzysort/rerank | +0.4743 | 0.0286 | [+0.4183, +0.5304] | 16.6 | 167 | keyhammer ahead |
| 100000 | keyhammer | keyhammer/hr | -0.0818 | 0.0148 | [-0.1108, -0.0529] | 5.5 | 36 | keyhammer behind |
| 100000 | keyhammer | minisearch | +0.0381 | 0.0132 | [+0.0122, +0.0639] | 2.9 | 56 | keyhammer ahead |
| 100000 | keyhammer | minisearch/rerank | -0.0029 | 0.0081 | [-0.0188, +0.0130] | 0.4 | 25 | not distinguishable |
| 100000 | keyhammer | fuse | +0.2046 | 0.0241 | [+0.1575, +0.2518] | 8.5 | 144 | keyhammer ahead |
| 100000 | keyhammer | fuse/rerank | -0.0569 | 0.0206 | [-0.0973, -0.0166] | 2.8 | 87 | keyhammer behind |
| 100000 | keyhammer | ufuzzy | +0.2630 | 0.0239 | [+0.2161, +0.3099] | 11.0 | 114 | keyhammer ahead |
| 100000 | keyhammer | ufuzzy/rerank | +0.1780 | 0.0226 | [+0.1336, +0.2223] | 7.9 | 84 | keyhammer ahead |
| 100000 | keyhammer | fuzzysort | +0.3293 | 0.0285 | [+0.2735, +0.3851] | 11.6 | 138 | keyhammer ahead |
| 100000 | keyhammer | fuzzysort/rerank | +0.3027 | 0.0298 | [+0.2442, +0.3611] | 10.1 | 136 | keyhammer ahead |
| 100000 | keyhammer/hr | minisearch | +0.1199 | 0.0187 | [+0.0833, +0.1565] | 6.4 | 90 | keyhammer ahead |
| 100000 | keyhammer/hr | minisearch/rerank | +0.0789 | 0.0162 | [+0.0471, +0.1108] | 4.9 | 59 | keyhammer ahead |
| 100000 | keyhammer/hr | fuse | +0.2865 | 0.0224 | [+0.2426, +0.3303] | 12.8 | 158 | keyhammer ahead |
| 100000 | keyhammer/hr | fuse/rerank | +0.0249 | 0.0155 | [-0.0055, +0.0553] | 1.6 | 72 | keyhammer ahead (weak: interval includes 0) |
| 100000 | keyhammer/hr | ufuzzy | +0.3448 | 0.0254 | [+0.2950, +0.3947] | 13.6 | 150 | keyhammer ahead |
| 100000 | keyhammer/hr | ufuzzy/rerank | +0.2598 | 0.0252 | [+0.2104, +0.3091] | 10.3 | 120 | keyhammer ahead |
| 100000 | keyhammer/hr | fuzzysort | +0.4111 | 0.0290 | [+0.3543, +0.4680] | 14.2 | 172 | keyhammer ahead |
| 100000 | keyhammer/hr | fuzzysort/rerank | +0.3845 | 0.0306 | [+0.3245, +0.4445] | 12.6 | 170 | keyhammer ahead |
| 274 137 (full) | keyhammer | keyhammer/hr | -0.0639 | 0.0130 | [-0.0894, -0.0384] | 4.9 | 31 | keyhammer behind |
| 274 137 (full) | keyhammer | minisearch | +0.0557 | 0.0148 | [+0.0268, +0.0847] | 3.8 | 69 | keyhammer ahead |
| 274 137 (full) | keyhammer | minisearch/rerank | +0.0012 | 0.0074 | [-0.0134, +0.0158] | 0.2 | 25 | not distinguishable |
| 274 137 (full) | keyhammer | fuse | +0.2715 | 0.0229 | [+0.2267, +0.3163] | 11.9 | 145 | keyhammer ahead |
| 274 137 (full) | keyhammer | fuse/rerank | -0.0262 | 0.0180 | [-0.0616, +0.0092] | 1.5 | 72 | not distinguishable |
| 274 137 (full) | keyhammer | ufuzzy | +0.2543 | 0.0230 | [+0.2092, +0.2994] | 11.0 | 113 | keyhammer ahead |
| 274 137 (full) | keyhammer | ufuzzy/rerank | +0.1648 | 0.0204 | [+0.1248, +0.2047] | 8.1 | 76 | keyhammer ahead |
| 274 137 (full) | keyhammer | fuzzysort | +0.3081 | 0.0278 | [+0.2536, +0.3626] | 11.1 | 138 | keyhammer ahead |
| 274 137 (full) | keyhammer | fuzzysort/rerank | +0.2735 | 0.0291 | [+0.2164, +0.3306] | 9.4 | 131 | keyhammer ahead |
| 274 137 (full) | keyhammer/hr | minisearch | +0.1197 | 0.0188 | [+0.0829, +0.1565] | 6.4 | 99 | keyhammer ahead |
| 274 137 (full) | keyhammer/hr | minisearch/rerank | +0.0651 | 0.0146 | [+0.0365, +0.0938] | 4.5 | 55 | keyhammer ahead |
| 274 137 (full) | keyhammer/hr | fuse | +0.3354 | 0.0226 | [+0.2911, +0.3797] | 14.8 | 167 | keyhammer ahead |
| 274 137 (full) | keyhammer/hr | fuse/rerank | +0.0377 | 0.0146 | [+0.0090, +0.0664] | 2.6 | 68 | keyhammer ahead |
| 274 137 (full) | keyhammer/hr | ufuzzy | +0.3182 | 0.0243 | [+0.2706, +0.3659] | 13.1 | 144 | keyhammer ahead |
| 274 137 (full) | keyhammer/hr | ufuzzy/rerank | +0.2287 | 0.0227 | [+0.1842, +0.2732] | 10.1 | 107 | keyhammer ahead |
| 274 137 (full) | keyhammer/hr | fuzzysort | +0.3720 | 0.0284 | [+0.3164, +0.4277] | 13.1 | 167 | keyhammer ahead |
| 274 137 (full) | keyhammer/hr | fuzzysort/rerank | +0.3374 | 0.0300 | [+0.2787, +0.3962] | 11.3 | 160 | keyhammer ahead |

#### Latency

| Terms | Engine | timed | p50 (us) | p95 (us) | p99 (us) | queries/s | p50 range | p95 range |
|---|---|---|---|---|---|---|---|---|
| 10000 | keyhammer | 1200 | 195.9 | 273.3 | 355.6 | 4943 | 194.6-196.2 | 266.3-301.4 |
| 10000 | keyhammer/hr | 1200 | 1052.2 | 1357.6 | 1543.0 | 1039 | 1041.6-1057.5 | 1357.1-1377.5 |
| 10000 | minisearch | 1200 | 144.7 | 194.1 | 263.7 | 6731 | 143.7-146.0 | 180.5-204.8 |
| 10000 | fuse | 1200 | 8571.8 | 13574.6 | 18144.3 | 116 | 8499.4-8587.6 | 13400.2-14229.5 |
| 10000 | ufuzzy | 1200 | 200.5 | 423.4 | 688.7 | 4367 | 200.0-202.1 | 422.8-439.0 |
| 10000 | fuzzysort | 1200 | 38.7 | 80.2 | 135.4 | 21687 | 38.6-38.7 | 79.8-86.0 |
| 100000 | keyhammer | 1200 | 443.8 | 640.6 | 788.7 | 2337 | 442.4-451.1 | 640.3-670.2 |
| 100000 | keyhammer/hr | 1200 | 2406.0 | 4133.9 | 4536.9 | 471 | 2395.1-2435.5 | 4092.5-4152.0 |
| 100000 | minisearch | 1200 | 385.6 | 555.4 | 739.6 | 2515 | 384.3-393.8 | 543.5-603.6 |
| 100000 | fuse | 233 | 86381.9 | 129611.0 | 158667.9 | 12 | 85986.8-89880.4 | 126907.2-140594.1 |
| 100000 | ufuzzy | 1200 | 1904.3 | 2529.1 | 2841.4 | 508 | 1881.5-1908.6 | 2418.8-2724.3 |
| 100000 | fuzzysort | 1200 | 373.4 | 861.4 | 1695.5 | 2165 | 367.7-397.8 | 810.1-900.3 |
| 274 137 (full) | keyhammer | 1200 | 543.4 | 866.3 | 1131.0 | 1912 | 539.4-545.6 | 865.6-922.1 |
| 274 137 (full) | keyhammer/hr | 1200 | 1779.6 | 6191.9 | 8631.4 | 396 | 1685.7-2316.9 | 5685.7-8671.4 |
| 274 137 (full) | minisearch | 1200 | 587.9 | 1222.6 | 1660.6 | 1527 | 583.3-602.2 | 1158.7-1266.9 |
| 274 137 (full) | fuse | 100 | 226256.3 | 344883.9 | 446488.7 | 4 | 223336.4-233178.9 | 340616.8-374218.9 |
| 274 137 (full) | ufuzzy | 1200 | 5149.5 | 6346.2 | 7232.2 | 190 | 5017.7-5166.4 | 6147.8-7013.2 |
| 274 137 (full) | fuzzysort | 1200 | 1200.0 | 4293.7 | 8352.7 | 595 | 1147.7-1306.1 | 3983.5-4709.9 |

### GitHub Typo Corpus

#### Quality

10000 terms: 58 of 1000 pairs usable, 55 (0.948) within OSA distance 2.

| Engine | MRR@10 | R@1 | R@10 | queries with no result |
|---|---|---|---|---|
| keyhammer | 0.948 | 0.948 | 0.948 | 2 |
| keyhammer/hr | 0.966 | 0.966 | 0.966 | 1 |
| minisearch | 0.902 | 0.862 | 0.948 | 2 |
| minisearch/rerank | 0.948 | 0.948 | 0.948 | 2 |
| fuse | 0.687 | 0.517 | 0.897 | 0 |
| fuse/rerank | 0.948 | 0.948 | 0.948 | 0 |
| ufuzzy | 0.733 | 0.655 | 0.828 | 9 |
| ufuzzy/rerank | 0.828 | 0.828 | 0.828 | 9 |
| fuzzysort | 0.328 | 0.328 | 0.328 | 30 |
| fuzzysort/rerank | 0.328 | 0.328 | 0.328 | 30 |

100000 terms: 380 of 1000 pairs usable, 358 (0.942) within OSA distance 2.

| Engine | MRR@10 | R@1 | R@10 | queries with no result |
|---|---|---|---|---|
| keyhammer | 0.876 | 0.842 | 0.929 | 5 |
| keyhammer/hr | 0.892 | 0.853 | 0.955 | 0 |
| minisearch | 0.792 | 0.721 | 0.903 | 8 |
| minisearch/rerank | 0.890 | 0.858 | 0.932 | 8 |
| fuse | 0.431 | 0.305 | 0.695 | 0 |
| fuse/rerank | 0.822 | 0.792 | 0.863 | 0 |
| ufuzzy | 0.584 | 0.526 | 0.708 | 40 |
| ufuzzy/rerank | 0.773 | 0.758 | 0.792 | 40 |
| fuzzysort | 0.293 | 0.284 | 0.311 | 153 |
| fuzzysort/rerank | 0.307 | 0.305 | 0.311 | 153 |

274 137 (full) terms: 1000 of 1000 pairs usable, 942 (0.942) within OSA distance 2.

| Engine | MRR@10 | R@1 | R@10 | queries with no result |
|---|---|---|---|---|
| keyhammer | 0.803 | 0.731 | 0.914 | 7 |
| keyhammer/hr | 0.811 | 0.735 | 0.935 | 2 |
| minisearch | 0.706 | 0.610 | 0.881 | 9 |
| minisearch/rerank | 0.820 | 0.756 | 0.918 | 9 |
| fuse | 0.296 | 0.172 | 0.559 | 0 |
| fuse/rerank | 0.738 | 0.688 | 0.812 | 0 |
| ufuzzy | 0.541 | 0.491 | 0.656 | 90 |
| ufuzzy/rerank | 0.720 | 0.682 | 0.769 | 90 |
| fuzzysort | 0.276 | 0.260 | 0.306 | 385 |
| fuzzysort/rerank | 0.304 | 0.297 | 0.313 | 385 |

Paired MRR differences, keyhammer minus the competitor (SE = sd / sqrt(n); verdict: "not distinguishable" when abs(difference) < 1.5 SE, "weak" between 1.5 and 1.96 SE, otherwise the sign says who is ahead; the 95% interval is difference +- 1.96 SE). `keyhammer/hr` is compared with the JS libraries and `keyhammer` with everything else.

| Terms | Reference | Competitor | Difference | SE | 95% interval | abs(diff)/SE | Queries that differ | Verdict |
|---|---|---|---|---|---|---|---|---|
| 10000 | keyhammer | keyhammer/hr | -0.0172 | 0.0172 | [-0.0510, +0.0166] | 1.0 | 1 | not distinguishable |
| 10000 | keyhammer | minisearch | +0.0460 | 0.0200 | [+0.0068, +0.0852] | 2.3 | 5 | keyhammer ahead |
| 10000 | keyhammer | minisearch/rerank | +0.0000 | 0.0000 | [+0.0000, +0.0000] | 0.0 | 0 | not distinguishable |
| 10000 | keyhammer | fuse | +0.2609 | 0.0459 | [+0.1710, +0.3508] | 5.7 | 26 | keyhammer ahead |
| 10000 | keyhammer | fuse/rerank | +0.0000 | 0.0246 | [-0.0482, +0.0482] | 0.0 | 2 | not distinguishable |
| 10000 | keyhammer | ufuzzy | +0.2155 | 0.0473 | [+0.1229, +0.3082] | 4.6 | 17 | keyhammer ahead |
| 10000 | keyhammer | ufuzzy/rerank | +0.1207 | 0.0431 | [+0.0361, +0.2053] | 2.8 | 7 | keyhammer ahead |
| 10000 | keyhammer | fuzzysort | +0.6207 | 0.0688 | [+0.4858, +0.7556] | 9.0 | 38 | keyhammer ahead |
| 10000 | keyhammer | fuzzysort/rerank | +0.6207 | 0.0688 | [+0.4858, +0.7556] | 9.0 | 38 | keyhammer ahead |
| 10000 | keyhammer/hr | minisearch | +0.0632 | 0.0259 | [+0.0125, +0.1139] | 2.4 | 6 | keyhammer ahead |
| 10000 | keyhammer/hr | minisearch/rerank | +0.0172 | 0.0172 | [-0.0166, +0.0510] | 1.0 | 1 | not distinguishable |
| 10000 | keyhammer/hr | fuse | +0.2782 | 0.0441 | [+0.1918, +0.3645] | 6.3 | 26 | keyhammer ahead |
| 10000 | keyhammer/hr | fuse/rerank | +0.0172 | 0.0172 | [-0.0166, +0.0510] | 1.0 | 1 | not distinguishable |
| 10000 | keyhammer/hr | ufuzzy | +0.2328 | 0.0490 | [+0.1367, +0.3288] | 4.7 | 18 | keyhammer ahead |
| 10000 | keyhammer/hr | ufuzzy/rerank | +0.1379 | 0.0457 | [+0.0484, +0.2275] | 3.0 | 8 | keyhammer ahead |
| 10000 | keyhammer/hr | fuzzysort | +0.6379 | 0.0682 | [+0.5042, +0.7717] | 9.3 | 39 | keyhammer ahead |
| 10000 | keyhammer/hr | fuzzysort/rerank | +0.6379 | 0.0682 | [+0.5042, +0.7717] | 9.3 | 39 | keyhammer ahead |
| 100000 | keyhammer | keyhammer/hr | -0.0157 | 0.0057 | [-0.0269, -0.0046] | 2.8 | 10 | keyhammer behind |
| 100000 | keyhammer | minisearch | +0.0841 | 0.0157 | [+0.0533, +0.1149] | 5.3 | 96 | keyhammer ahead |
| 100000 | keyhammer | minisearch/rerank | -0.0145 | 0.0069 | [-0.0281, -0.0009] | 2.1 | 27 | keyhammer behind |
| 100000 | keyhammer | fuse | +0.4448 | 0.0229 | [+0.4000, +0.4897] | 19.4 | 253 | keyhammer ahead |
| 100000 | keyhammer | fuse/rerank | +0.0540 | 0.0165 | [+0.0216, +0.0864] | 3.3 | 63 | keyhammer ahead |
| 100000 | keyhammer | ufuzzy | +0.2915 | 0.0225 | [+0.2474, +0.3356] | 13.0 | 164 | keyhammer ahead |
| 100000 | keyhammer | ufuzzy/rerank | +0.1026 | 0.0174 | [+0.0686, +0.1366] | 5.9 | 68 | keyhammer ahead |
| 100000 | keyhammer | fuzzysort | +0.5827 | 0.0263 | [+0.5312, +0.6342] | 22.2 | 261 | keyhammer ahead |
| 100000 | keyhammer | fuzzysort/rerank | +0.5688 | 0.0267 | [+0.5164, +0.6212] | 21.3 | 253 | keyhammer ahead |
| 100000 | keyhammer/hr | minisearch | +0.0998 | 0.0165 | [+0.0675, +0.1322] | 6.0 | 106 | keyhammer ahead |
| 100000 | keyhammer/hr | minisearch/rerank | +0.0013 | 0.0090 | [-0.0164, +0.0189] | 0.1 | 37 | not distinguishable |
| 100000 | keyhammer/hr | fuse | +0.4606 | 0.0221 | [+0.4172, +0.5039] | 20.8 | 254 | keyhammer ahead |
| 100000 | keyhammer/hr | fuse/rerank | +0.0697 | 0.0152 | [+0.0400, +0.0994] | 4.6 | 60 | keyhammer ahead |
| 100000 | keyhammer/hr | ufuzzy | +0.3072 | 0.0226 | [+0.2629, +0.3515] | 13.6 | 172 | keyhammer ahead |
| 100000 | keyhammer/hr | ufuzzy/rerank | +0.1183 | 0.0178 | [+0.0834, +0.1533] | 6.6 | 77 | keyhammer ahead |
| 100000 | keyhammer/hr | fuzzysort | +0.5984 | 0.0259 | [+0.5476, +0.6493] | 23.1 | 271 | keyhammer ahead |
| 100000 | keyhammer/hr | fuzzysort/rerank | +0.5846 | 0.0264 | [+0.5327, +0.6364] | 22.1 | 263 | keyhammer ahead |
| 274 137 (full) | keyhammer | keyhammer/hr | -0.0078 | 0.0022 | [-0.0121, -0.0034] | 3.5 | 21 | keyhammer behind |
| 274 137 (full) | keyhammer | minisearch | +0.0966 | 0.0113 | [+0.0745, +0.1187] | 8.6 | 365 | keyhammer ahead |
| 274 137 (full) | keyhammer | minisearch/rerank | -0.0170 | 0.0057 | [-0.0283, -0.0058] | 3.0 | 122 | keyhammer behind |
| 274 137 (full) | keyhammer | fuse | +0.5068 | 0.0133 | [+0.4808, +0.5328] | 38.2 | 762 | keyhammer ahead |
| 274 137 (full) | keyhammer | fuse/rerank | +0.0645 | 0.0106 | [+0.0436, +0.0854] | 6.1 | 229 | keyhammer ahead |
| 274 137 (full) | keyhammer | ufuzzy | +0.2617 | 0.0136 | [+0.2350, +0.2883] | 19.2 | 452 | keyhammer ahead |
| 274 137 (full) | keyhammer | ufuzzy/rerank | +0.0828 | 0.0105 | [+0.0621, +0.1034] | 7.9 | 219 | keyhammer ahead |
| 274 137 (full) | keyhammer | fuzzysort | +0.5273 | 0.0170 | [+0.4940, +0.5606] | 31.1 | 722 | keyhammer ahead |
| 274 137 (full) | keyhammer | fuzzysort/rerank | +0.4993 | 0.0177 | [+0.4647, +0.5340] | 28.3 | 701 | keyhammer ahead |
| 274 137 (full) | keyhammer/hr | minisearch | +0.1044 | 0.0114 | [+0.0821, +0.1267] | 9.2 | 385 | keyhammer ahead |
| 274 137 (full) | keyhammer/hr | minisearch/rerank | -0.0093 | 0.0061 | [-0.0213, +0.0027] | 1.5 | 142 | keyhammer behind (weak: interval includes 0) |
| 274 137 (full) | keyhammer/hr | fuse | +0.5146 | 0.0129 | [+0.4894, +0.5398] | 40.0 | 769 | keyhammer ahead |
| 274 137 (full) | keyhammer/hr | fuse/rerank | +0.0723 | 0.0103 | [+0.0520, +0.0925] | 7.0 | 223 | keyhammer ahead |
| 274 137 (full) | keyhammer/hr | ufuzzy | +0.2694 | 0.0136 | [+0.2427, +0.2962] | 19.8 | 472 | keyhammer ahead |
| 274 137 (full) | keyhammer/hr | ufuzzy/rerank | +0.0906 | 0.0107 | [+0.0696, +0.1115] | 8.5 | 239 | keyhammer ahead |
| 274 137 (full) | keyhammer/hr | fuzzysort | +0.5351 | 0.0169 | [+0.5020, +0.5681] | 31.7 | 743 | keyhammer ahead |
| 274 137 (full) | keyhammer/hr | fuzzysort/rerank | +0.5071 | 0.0176 | [+0.4726, +0.5416] | 28.8 | 722 | keyhammer ahead |

#### Latency

| Terms | Engine | timed | p50 (us) | p95 (us) | p99 (us) | queries/s | p50 range | p95 range |
|---|---|---|---|---|---|---|---|---|
| 10000 | keyhammer | 1044 | 192.4 | 273.5 | 322.8 | 5178 | 191.3-193.0 | 271.4-276.1 |
| 10000 | keyhammer/hr | 1044 | 1025.2 | 1394.6 | 1818.2 | 1071 | 1020.9-1119.3 | 1349.4-2090.3 |
| 10000 | minisearch | 1044 | 141.7 | 189.1 | 240.1 | 6924 | 141.7-143.7 | 181.2-216.1 |
| 10000 | fuse | 1044 | 8799.8 | 15093.8 | 19003.5 | 115 | 8753.5-8841.3 | 15038.6-15255.4 |
| 10000 | ufuzzy | 1044 | 201.9 | 264.7 | 327.1 | 4825 | 200.1-206.4 | 258.4-321.7 |
| 10000 | fuzzysort | 1044 | 40.2 | 73.5 | 97.6 | 21337 | 39.8-58.1 | 68.0-75.6 |
| 100000 | keyhammer | 1140 | 437.3 | 635.4 | 750.3 | 2325 | 435.7-440.9 | 631.5-675.4 |
| 100000 | keyhammer/hr | 1140 | 2202.8 | 4219.5 | 4891.5 | 473 | 2200.6-2267.0 | 4180.7-4371.9 |
| 100000 | minisearch | 1140 | 393.8 | 547.8 | 707.6 | 2474 | 392.6-394.6 | 537.0-618.3 |
| 100000 | fuse | 220 | 88598.2 | 159184.5 | 186466.2 | 11 | 87594.2-88712.6 | 156191.3-160065.8 |
| 100000 | ufuzzy | 1140 | 1930.6 | 2538.4 | 2946.8 | 502 | 1928.4-1932.3 | 2532.7-2550.6 |
| 100000 | fuzzysort | 1140 | 352.3 | 640.8 | 1229.5 | 2399 | 351.8-370.5 | 637.2-709.6 |
| 274 137 (full) | keyhammer | 1000 | 515.7 | 829.1 | 1183.9 | 1989 | 505.4-521.0 | 800.6-861.8 |
| 274 137 (full) | keyhammer/hr | 1000 | 1219.9 | 5512.0 | 7298.1 | 470 | 1213.6-1361.5 | 5444.8-6148.1 |
| 274 137 (full) | minisearch | 1000 | 615.0 | 1094.9 | 1870.6 | 1479 | 596.9-652.4 | 1070.7-1219.0 |
| 274 137 (full) | fuse | 100 | 240602.3 | 419688.4 | 573671.9 | 4 | 239744.6-288226.6 | 416727.1-495379.7 |
| 274 137 (full) | ufuzzy | 1000 | 7221.8 | 14790.1 | 17044.1 | 122 | 6800.1-7858.1 | 14789.1-17434.3 |
| 274 137 (full) | fuzzysort | 1000 | 1165.3 | 3997.0 | 9905.7 | 616 | 1143.3-1326.1 | 3876.8-4235.9 |

### Wikipedia misspellings

#### Quality

10000 terms: 92 of 1000 pairs usable, 91 (0.989) within OSA distance 2.

| Engine | MRR@10 | R@1 | R@10 | queries with no result |
|---|---|---|---|---|
| keyhammer | 0.978 | 0.967 | 0.989 | 1 |
| keyhammer/hr | 0.989 | 0.978 | 1.000 | 0 |
| minisearch | 0.948 | 0.924 | 0.978 | 2 |
| minisearch/rerank | 0.973 | 0.967 | 0.978 | 2 |
| fuse | 0.859 | 0.793 | 0.967 | 0 |
| fuse/rerank | 0.973 | 0.967 | 0.978 | 0 |
| ufuzzy | 0.793 | 0.772 | 0.815 | 15 |
| ufuzzy/rerank | 0.815 | 0.815 | 0.815 | 15 |
| fuzzysort | 0.384 | 0.380 | 0.391 | 51 |
| fuzzysort/rerank | 0.391 | 0.391 | 0.391 | 51 |

100000 terms: 400 of 1000 pairs usable, 393 (0.983) within OSA distance 2.

| Engine | MRR@10 | R@1 | R@10 | queries with no result |
|---|---|---|---|---|
| keyhammer | 0.944 | 0.915 | 0.985 | 4 |
| keyhammer/hr | 0.951 | 0.920 | 0.998 | 0 |
| minisearch | 0.861 | 0.813 | 0.938 | 7 |
| minisearch/rerank | 0.929 | 0.895 | 0.973 | 7 |
| fuse | 0.516 | 0.375 | 0.792 | 0 |
| fuse/rerank | 0.898 | 0.873 | 0.930 | 0 |
| ufuzzy | 0.689 | 0.640 | 0.787 | 48 |
| ufuzzy/rerank | 0.828 | 0.815 | 0.843 | 48 |
| fuzzysort | 0.344 | 0.338 | 0.355 | 195 |
| fuzzysort/rerank | 0.360 | 0.360 | 0.360 | 195 |

274 137 (full) terms: 1000 of 1000 pairs usable, 981 (0.981) within OSA distance 2.

| Engine | MRR@10 | R@1 | R@10 | queries with no result |
|---|---|---|---|---|
| keyhammer | 0.905 | 0.855 | 0.978 | 7 |
| keyhammer/hr | 0.909 | 0.858 | 0.984 | 1 |
| minisearch | 0.818 | 0.766 | 0.907 | 12 |
| minisearch/rerank | 0.890 | 0.840 | 0.962 | 12 |
| fuse | 0.410 | 0.277 | 0.688 | 0 |
| fuse/rerank | 0.830 | 0.788 | 0.885 | 0 |
| ufuzzy | 0.656 | 0.607 | 0.767 | 93 |
| ufuzzy/rerank | 0.814 | 0.785 | 0.850 | 93 |
| fuzzysort | 0.359 | 0.350 | 0.377 | 411 |
| fuzzysort/rerank | 0.381 | 0.379 | 0.382 | 411 |

Paired MRR differences, keyhammer minus the competitor (SE = sd / sqrt(n); verdict: "not distinguishable" when abs(difference) < 1.5 SE, "weak" between 1.5 and 1.96 SE, otherwise the sign says who is ahead; the 95% interval is difference +- 1.96 SE). `keyhammer/hr` is compared with the JS libraries and `keyhammer` with everything else.

| Terms | Reference | Competitor | Difference | SE | 95% interval | abs(diff)/SE | Queries that differ | Verdict |
|---|---|---|---|---|---|---|---|---|
| 10000 | keyhammer | keyhammer/hr | -0.0109 | 0.0109 | [-0.0322, +0.0104] | 1.0 | 1 | not distinguishable |
| 10000 | keyhammer | minisearch | +0.0304 | 0.0145 | [+0.0021, +0.0588] | 2.1 | 5 | keyhammer ahead |
| 10000 | keyhammer | minisearch/rerank | +0.0054 | 0.0144 | [-0.0229, +0.0338] | 0.4 | 4 | not distinguishable |
| 10000 | keyhammer | fuse | +0.1191 | 0.0305 | [+0.0594, +0.1788] | 3.9 | 20 | keyhammer ahead |
| 10000 | keyhammer | fuse/rerank | +0.0054 | 0.0181 | [-0.0301, +0.0409] | 0.3 | 5 | not distinguishable |
| 10000 | keyhammer | ufuzzy | +0.1848 | 0.0384 | [+0.1095, +0.2601] | 4.8 | 20 | keyhammer ahead |
| 10000 | keyhammer | ufuzzy/rerank | +0.1630 | 0.0379 | [+0.0887, +0.2374] | 4.3 | 16 | keyhammer ahead |
| 10000 | keyhammer | fuzzysort | +0.5942 | 0.0506 | [+0.4950, +0.6934] | 11.7 | 56 | keyhammer ahead |
| 10000 | keyhammer | fuzzysort/rerank | +0.5870 | 0.0510 | [+0.4869, +0.6870] | 11.5 | 55 | keyhammer ahead |
| 10000 | keyhammer/hr | minisearch | +0.0413 | 0.0179 | [+0.0062, +0.0764] | 2.3 | 6 | keyhammer ahead |
| 10000 | keyhammer/hr | minisearch/rerank | +0.0163 | 0.0180 | [-0.0191, +0.0517] | 0.9 | 5 | not distinguishable |
| 10000 | keyhammer/hr | fuse | +0.1300 | 0.0300 | [+0.0712, +0.1887] | 4.3 | 20 | keyhammer ahead |
| 10000 | keyhammer/hr | fuse/rerank | +0.0163 | 0.0144 | [-0.0118, +0.0444] | 1.1 | 4 | not distinguishable |
| 10000 | keyhammer/hr | ufuzzy | +0.1957 | 0.0394 | [+0.1185, +0.2728] | 5.0 | 21 | keyhammer ahead |
| 10000 | keyhammer/hr | ufuzzy/rerank | +0.1739 | 0.0390 | [+0.0975, +0.2503] | 4.5 | 17 | keyhammer ahead |
| 10000 | keyhammer/hr | fuzzysort | +0.6051 | 0.0504 | [+0.5063, +0.7038] | 12.0 | 57 | keyhammer ahead |
| 10000 | keyhammer/hr | fuzzysort/rerank | +0.5978 | 0.0508 | [+0.4982, +0.6974] | 11.8 | 56 | keyhammer ahead |
| 100000 | keyhammer | keyhammer/hr | -0.0071 | 0.0038 | [-0.0145, +0.0003] | 1.9 | 5 | keyhammer behind (weak: interval includes 0) |
| 100000 | keyhammer | minisearch | +0.0826 | 0.0141 | [+0.0549, +0.1102] | 5.9 | 85 | keyhammer ahead |
| 100000 | keyhammer | minisearch/rerank | +0.0151 | 0.0071 | [+0.0012, +0.0290] | 2.1 | 20 | keyhammer ahead |
| 100000 | keyhammer | fuse | +0.4282 | 0.0204 | [+0.3882, +0.4683] | 21.0 | 248 | keyhammer ahead |
| 100000 | keyhammer | fuse/rerank | +0.0455 | 0.0120 | [+0.0219, +0.0690] | 3.8 | 42 | keyhammer ahead |
| 100000 | keyhammer | ufuzzy | +0.2553 | 0.0198 | [+0.2164, +0.2941] | 12.9 | 137 | keyhammer ahead |
| 100000 | keyhammer | ufuzzy/rerank | +0.1157 | 0.0164 | [+0.0835, +0.1479] | 7.0 | 63 | keyhammer ahead |
| 100000 | keyhammer | fuzzysort | +0.6001 | 0.0245 | [+0.5520, +0.6481] | 24.5 | 266 | keyhammer ahead |
| 100000 | keyhammer | fuzzysort/rerank | +0.5838 | 0.0250 | [+0.5348, +0.6328] | 23.4 | 258 | keyhammer ahead |
| 100000 | keyhammer/hr | minisearch | +0.0897 | 0.0145 | [+0.0613, +0.1181] | 6.2 | 90 | keyhammer ahead |
| 100000 | keyhammer/hr | minisearch/rerank | +0.0222 | 0.0080 | [+0.0065, +0.0379] | 2.8 | 25 | keyhammer ahead |
| 100000 | keyhammer/hr | fuse | +0.4353 | 0.0201 | [+0.3959, +0.4748] | 21.6 | 248 | keyhammer ahead |
| 100000 | keyhammer/hr | fuse/rerank | +0.0526 | 0.0113 | [+0.0305, +0.0747] | 4.7 | 39 | keyhammer ahead |
| 100000 | keyhammer/hr | ufuzzy | +0.2624 | 0.0199 | [+0.2233, +0.3015] | 13.2 | 142 | keyhammer ahead |
| 100000 | keyhammer/hr | ufuzzy/rerank | +0.1228 | 0.0167 | [+0.0900, +0.1556] | 7.3 | 68 | keyhammer ahead |
| 100000 | keyhammer/hr | fuzzysort | +0.6072 | 0.0244 | [+0.5594, +0.6550] | 24.9 | 271 | keyhammer ahead |
| 100000 | keyhammer/hr | fuzzysort/rerank | +0.5909 | 0.0249 | [+0.5422, +0.6397] | 23.8 | 263 | keyhammer ahead |
| 274 137 (full) | keyhammer | keyhammer/hr | -0.0040 | 0.0018 | [-0.0075, -0.0004] | 2.2 | 6 | keyhammer behind |
| 274 137 (full) | keyhammer | minisearch | +0.0870 | 0.0105 | [+0.0664, +0.1076] | 8.3 | 278 | keyhammer ahead |
| 274 137 (full) | keyhammer | minisearch/rerank | +0.0151 | 0.0050 | [+0.0053, +0.0249] | 3.0 | 77 | keyhammer ahead |
| 274 137 (full) | keyhammer | fuse | +0.4956 | 0.0132 | [+0.4698, +0.5214] | 37.6 | 709 | keyhammer ahead |
| 274 137 (full) | keyhammer | fuse/rerank | +0.0758 | 0.0093 | [+0.0577, +0.0940] | 8.2 | 156 | keyhammer ahead |
| 274 137 (full) | keyhammer | ufuzzy | +0.2497 | 0.0127 | [+0.2248, +0.2745] | 19.7 | 373 | keyhammer ahead |
| 274 137 (full) | keyhammer | ufuzzy/rerank | +0.0911 | 0.0097 | [+0.0721, +0.1101] | 9.4 | 158 | keyhammer ahead |
| 274 137 (full) | keyhammer | fuzzysort | +0.5461 | 0.0163 | [+0.5142, +0.5780] | 33.6 | 667 | keyhammer ahead |
| 274 137 (full) | keyhammer | fuzzysort/rerank | +0.5249 | 0.0167 | [+0.4921, +0.5577] | 31.4 | 650 | keyhammer ahead |
| 274 137 (full) | keyhammer/hr | minisearch | +0.0910 | 0.0107 | [+0.0701, +0.1118] | 8.5 | 284 | keyhammer ahead |
| 274 137 (full) | keyhammer/hr | minisearch/rerank | +0.0191 | 0.0053 | [+0.0087, +0.0295] | 3.6 | 83 | keyhammer ahead |
| 274 137 (full) | keyhammer/hr | fuse | +0.4995 | 0.0131 | [+0.4739, +0.5252] | 38.2 | 712 | keyhammer ahead |
| 274 137 (full) | keyhammer/hr | fuse/rerank | +0.0798 | 0.0091 | [+0.0619, +0.0976] | 8.8 | 154 | keyhammer ahead |
| 274 137 (full) | keyhammer/hr | ufuzzy | +0.2536 | 0.0127 | [+0.2287, +0.2786] | 19.9 | 379 | keyhammer ahead |
| 274 137 (full) | keyhammer/hr | ufuzzy/rerank | +0.0951 | 0.0098 | [+0.0758, +0.1143] | 9.7 | 164 | keyhammer ahead |
| 274 137 (full) | keyhammer/hr | fuzzysort | +0.5501 | 0.0162 | [+0.5183, +0.5819] | 33.9 | 673 | keyhammer ahead |
| 274 137 (full) | keyhammer/hr | fuzzysort/rerank | +0.5288 | 0.0167 | [+0.4961, +0.5616] | 31.6 | 656 | keyhammer ahead |

keyhammer search errors and searches cut short by the node limit: 0 in every configuration.

#### Latency

| Terms | Engine | timed | p50 (us) | p95 (us) | p99 (us) | queries/s | p50 range | p95 range |
|---|---|---|---|---|---|---|---|---|
| 10000 | keyhammer | 1012 | 199.1 | 262.3 | 306.7 | 4988 | 199.0-199.5 | 259.4-264.9 |
| 10000 | keyhammer/hr | 1012 | 1107.8 | 1459.6 | 1920.1 | 922 | 1104.2-1142.8 | 1343.0-1730.9 |
| 10000 | minisearch | 1012 | 143.3 | 218.7 | 270.4 | 6704 | 143.2-158.5 | 210.0-248.8 |
| 10000 | fuse | 1012 | 9136.5 | 15634.4 | 19037.9 | 100 | 9048.6-9180.1 | 15302.9-15680.6 |
| 10000 | ufuzzy | 1012 | 194.7 | 283.8 | 329.6 | 4934 | 193.7-196.1 | 244.3-316.8 |
| 10000 | fuzzysort | 1012 | 37.2 | 57.3 | 69.0 | 24598 | 37.2-37.4 | 56.7-59.2 |
| 100000 | keyhammer | 1200 | 454.8 | 627.6 | 725.1 | 2241 | 453.8-457.4 | 625.2-641.8 |
| 100000 | keyhammer/hr | 1200 | 2971.5 | 4182.9 | 4664.0 | 397 | 2966.4-2992.9 | 4082.2-4420.5 |
| 100000 | minisearch | 1200 | 379.7 | 497.4 | 661.5 | 2604 | 376.0-435.4 | 482.1-660.8 |
| 100000 | fuse | 200 | 92357.2 | 156453.4 | 188405.0 | 10 | 92267.4-92790.8 | 156296.0-166840.5 |
| 100000 | ufuzzy | 1200 | 1885.1 | 2560.9 | 3001.2 | 507 | 1879.1-1922.8 | 2534.9-2605.9 |
| 100000 | fuzzysort | 1200 | 343.8 | 541.1 | 887.2 | 2636 | 342.4-353.4 | 522.8-661.2 |
| 274 137 (full) | keyhammer | 1000 | 555.6 | 844.7 | 1182.4 | 1821 | 547.7-628.7 | 795.4-938.4 |
| 274 137 (full) | keyhammer/hr | 1000 | 2189.0 | 5610.8 | 6950.5 | 384 | 2188.3-2545.6 | 5535.7-6433.4 |
| 274 137 (full) | minisearch | 1000 | 577.9 | 903.3 | 1587.3 | 1625 | 564.5-641.4 | 857.1-957.8 |
| 274 137 (full) | fuse | 100 | 275490.0 | 527951.6 | 623966.4 | 3 | 265159.4-317607.0 | 444480.9-583181.5 |
| 274 137 (full) | ufuzzy | 1000 | 7719.4 | 14264.7 | 16230.2 | 120 | 7684.0-9894.9 | 14078.1-20930.5 |
| 274 137 (full) | fuzzysort | 1000 | 1077.7 | 2585.7 | 6366.3 | 736 | 1029.2-1802.5 | 2545.6-3617.0 |

## Notes on the results

- The same engine: keyhammer's MRR@10, R@1 and R@10 in this harness equal the Rust report's in all nine configurations, so the wasm build returns the same results as the native one on these queries, and any difference from the Rust report in latency or memory is not a difference in what is computed.
- keyhammer and MiniSearch: with MiniSearch's own scoring keyhammer is ahead everywhere. With MiniSearch's first 100 candidates re-sorted by (OSA distance, higher weight, term) the two are not distinguishable on Birkbeck at 100 000 and 274 137 terms and on the GitHub and Wikipedia corpora at 10 000 terms, keyhammer is weakly ahead on Birkbeck at 10 000 terms, ahead on Wikipedia at 100 000 and 274 137 terms, and behind on the GitHub corpus at 100 000 and 274 137 terms (see the paired tables). So most of keyhammer's lead over MiniSearch's own order disappears once the two share a tie-break rule that uses the weights. Whether MiniSearch's own order ignores the weights or its scoring formula is the cause was not investigated (hypothesis).
- Fuse: its own order is poor (MRR@10 0.158 on Birkbeck at the full dictionary), but its first 100 candidates re-sorted by the shared rule reach 0.455, above keyhammer's 0.429 there (the difference is not distinguishable, 1.5 SE). So in this setup Fuse retrieves good candidates and its ordering of them is the weak point; why its score orders them this way was not investigated. On Birkbeck the re-sorted Fuse list has the right word in its top 10 in 0.710, 0.613 and 0.540 of the queries at 10 000, 100 000 and 274 137 terms, above `keyhammer`'s 0.560, 0.550 and 0.517 and below `keyhammer/hr`'s 0.727, 0.670 and 0.620.
- uFuzzy and fuzzysort return nothing for many queries (Birkbeck at the full dictionary: 118 and 145 of 300; keyhammer 43, `keyhammer/hr` 9, MiniSearch 45, Fuse 1). uFuzzy with `intraMode: 1` allows one error per term, so a word at two edits cannot match. For fuzzysort, a spot check outside the harness with `fuzzysort.single` found no match for `recieve` against `receive`, `teh` against `the`, `wnat` against `want` and `definately` against `definitely`, and a match for `recve` against `receive` and `sedid` against `splendid`. That is consistent with matching the query's characters in order and having no tolerance for transposed, substituted or inserted letters (not verified in its source). It makes fuzzysort a poor fit for this task, which is not a verdict on the library.
- Scaling: from 10 000 to 274 137 terms (Birkbeck p50) keyhammer's latency grows 2.8 times, MiniSearch's 4.1 times, uFuzzy's 25.7 times, Fuse's 26.4 times and fuzzysort's 31.0 times. That is why keyhammer overtakes MiniSearch in p50 and p95 on all three corpora at the full dictionary in this single run (on Wikipedia within the run-to-run spread) while being behind at 10 000 terms. It is a measured trend over three sizes, not a model.
- Tails: at the full dictionary keyhammer's p95 and p99 on Birkbeck (866.3 and 1131.0 us) are lower than MiniSearch's (1222.6 and 1660.6 us), and the same holds on the GitHub and Wikipedia corpora (see the tables). fuzzysort's p95 is 3.6 times its p50 there (4293.7 against 1200.0 us).
- The cost of WebAssembly: keyhammer's p50 here is about 2.2, 2.1 and 2.2 times the Rust report's at 10 000, 100 000 and 274 137 terms on Birkbeck (195.9 against 89.1 us, 443.8 against 207.9 us, 543.4 against 250.0 us). A throwaway measurement outside the harness (single run, full dictionary, Birkbeck queries, three alternating passes, p50 in us) timed the query encoding plus `kh_search` alone at 535.8, 530.3 and 642.5 and the whole path with decoding and splitting the results at 531.9, 551.4 and 570.7: in that single run the marshalling was not distinguishable from noise, which suggests (not measured in the harness) that most of the time is spent inside the wasm call. How much of it is the missing subtree bound, the size-optimised (`opt-level = "z"`) build or the speed of WebAssembly execution was not separated (hypothesis: all three matter; `recall-preset.md` found that the bound lowers the native default's p95 by about 17 to 24% at these sizes).
- Build time: keyhammer builds the full dictionary in 128 ms here (125 to 145 ms over three builds), against 77 ms for the native build in the Rust report. The wasm figure includes serialising the dictionary to text and compiling and instantiating the module (about 0.6 ms, see the load table); the native one does not. MiniSearch takes 474 ms, fuzzysort 60 ms, Fuse 8 ms; uFuzzy builds nothing (0 ms).
- Memory: Fuse's 56.4 bytes per term is an array of records that point at the caller's strings; uFuzzy keeps nothing. keyhammer's 209.9 bytes per term is the growth of linear memory, measured after instantiation (so the module's initial linear memory is not in the figure), including the build's temporaries, so its real index is smaller; it is not the same kind of figure as the heap deltas (see Method), which is why it has its own column.
- `keyhammer/hr` against `keyhammer`: the paired MRR differences on Birkbeck (keyhammer minus `keyhammer/hr`) are -0.1199, -0.0818 and -0.0639 at 10 000, 100 000 and 274 137 terms (6.8, 5.5 and 4.9 SE), the same as in `recall-preset.md`. On the GitHub and Wikipedia corpora the differences are small (at most 0.0172 in absolute value) and clear only at the full dictionary (GitHub -0.0078, 3.5 SE; Wikipedia -0.0040, 2.2 SE) and at 100 000 terms on the GitHub corpus (-0.0157, 2.8 SE).

## Where keyhammer loses

- Speed at small and medium sizes: MiniSearch and fuzzysort have a lower p50 than keyhammer at 10 000 and 100 000 terms on all three corpora (keyhammer's p50 is 1.11 to 1.39 times MiniSearch's there). keyhammer overtakes MiniSearch only at the full dictionary, by 4% to 16% in p50 in this single run (on Wikipedia within the run-to-run spread). uFuzzy also has a lower p50 on Wikipedia at 10 000 terms (194.7 against 199.1 us, with non-overlapping ranges) and a lower p95 on the GitHub corpus at 10 000 terms (264.7 against 273.5 us). The engine is not the fastest option in JavaScript at the sizes most web applications use, in this setup.
- WebAssembly against native: this wasm build (size-optimised, without the subtree bound) has about 2.2 times the native build's p50, from a different run and harness; how much of that is WebAssembly itself was not separated (see Notes on the results). The comparison with SymSpell in the Rust report, which was 4 to 11 times faster than the native keyhammer, was not repeated here, and nothing here says that the native speed advantages carry over to Node.
- The high-recall budget: `keyhammer/hr` has a p50 2.4 to 6.5 times and a p95 5.0 to 7.1 times keyhammer's on the three corpora. At 274 137 terms its p95 is 5.5 to 6.2 ms and its p99 up to 8.6 ms. Its quality gain is clear on Birkbeck and small on the other two corpora.
- Ranking quality once ties are treated alike: Fuse's candidates re-sorted by the shared rule beat keyhammer's default on Birkbeck at 10 000 terms (-0.1213, 5.3 SE) and 100 000 terms (-0.0569, 2.8 SE), and MiniSearch's beat it on the GitHub corpus at 100 000 terms (-0.0145, 2.1 SE) and 274 137 terms (-0.0170, 3.0 SE). On Birkbeck at 10 000 terms `keyhammer/hr` is not distinguishable from re-sorted Fuse (-0.0014, 0.1 SE), which is consistent with the default budget being what holds the default back there (hypothesis). These re-sorted rows are not what a user of the libraries gets, but they show that the lead in the main tables is partly the weights and the tie-break.
- Recall with the libraries' own order: Fuse has the right word in its top 10 more often on Birkbeck at 10 000 terms (R@10 0.623 against 0.560).
- No result: with the default budget keyhammer returns nothing for 92, 57 and 43 of the 300 Birkbeck queries at 10 000, 100 000 and 274 137 terms (Fuse returns nothing for 5, 1 and 1); some of those pairs are beyond two edits, where no engine here can succeed.
- Build time and index size: Fuse builds in 8 ms and uFuzzy needs no build; Fuse's heap footprint per term is below keyhammer's linear-memory growth (56.4 against 209.9 bytes per term, which is an upper bound for keyhammer). Only MiniSearch (969.0) and fuzzysort (385.3) take more.
- Scope: this benchmark measures whole-word lookup of a misspelled word in a dictionary. It does not measure what the libraries also do and keyhammer does not: prefix and substring search, several words in a query, several fields, ranking of documents. keyhammer is a word-lookup engine and has no answer to those.
- Absolute quality is modest on Birkbeck: 43% of its pairs are more than two edits away, and the best MRR@10 keyhammer reaches at the full dictionary there is 0.493 (`keyhammer/hr`).

## Caveats

- One machine, one operating system, one Node.js and V8 version, one WebAssembly build profile. Numbers from other machines, other Node versions or a browser will differ, and so may the ratios. Node only: no browser was used and no network transfer was measured.
- One dictionary of English a-z words and three English typo corpora; nothing here speaks for other languages, alphabets or keyboard layouts.
- The keyhammer edit costs are provisional and were not calibrated.
- Small samples: at 10 000 terms the GitHub and Wikipedia corpora have 58 and 92 usable pairs, and 380 and 400 at 100 000 terms, so their intervals are wide. One sample per corpus with one seed; the quality figures are deterministic for that sample, and another sample would give somewhat different numbers.
- Library versions and settings: exact versions are pinned in `bench/js-competitors/package.json` and its lockfile. The settings are one reasonable configuration of each library chosen before measuring, and two of them (Fuse's and fuzzysort's thresholds) were checked with a small sensitivity run; the others were not explored. A different setting, tokenisation or scoring option could change how the libraries rank. In particular uFuzzy was run with `infoThresh` raised to 1e9, which costs time on queries with many matches; its default was not measured.
- The libraries are ranked by their own scoring and were given no weights, whereas keyhammer is given them and most dictionary words have weight 0 (so ties are frequent). The `/rerank` variant re-sorts the first 100 candidates in the library's own order by a rule that uses OSA distance and the weights; it is not a library configuration, its cap of 100 is arbitrary, and it was not run for latency.
- The libraries are general search libraries; this benchmark uses them for one narrow task (whole-word typo lookup over a word list) and is not a verdict on them for their intended uses.
- Latency: one run of three repetitions on a machine that was quiet but not dedicated, with a few short samples of other processes (see Machine) and a wide spread for `keyhammer/hr`, MiniSearch, uFuzzy and especially fuzzysort. Fuse's latency at 100 000 and 274 137 terms is measured on 100 to 233 queries per run (see the "timed" column), so its p95 and p99 are rough. These are the first N queries of the set, not the full set timed for the other engines, so Fuse's percentiles and the factors of 195 and 416 against keyhammer compare different query subsets. Latency includes JavaScript garbage-collection pauses, which may be part of the tails of the libraries that allocate per query (hypothesis; allocation was not measured). Warm-up is bounded (200 queries or 5 seconds).
- The keyhammer rows run the `wasm` profile (optimised for size) without the subtree bound. A speed-optimised build, or a wasm interface that exposes the bound, was not measured.
- Memory: the JavaScript heap deltas after forced collections exclude the caller's term strings for the libraries and include a copy of them inside keyhammer's wasm memory. keyhammer's linear-memory growth is measured after instantiation (the module's initial linear memory is not in it), includes build-time temporaries, is an upper bound, and does not shrink. The resident memory of the process was not measured. Build time is the median of three builds; the first (used for memory) runs in a fresh process, the other two in the same process after a collection. The build and memory tables come from the quality run, whose machine activity was not sampled.
- Load time is for Node, from local disk, in a fresh process, and does not include network transfer; a browser would fetch the module (16 896 bytes gzip) and compile it with its own pipeline, which was not measured.
- The Rust and JavaScript reports were run on the same machine but not at the same time; comparing their latency figures (for example the factor of 2.2) mixes two harnesses.

## Licences of the new dependencies

Four new dependencies, all in `bench/js-competitors` (development and benchmark only: they are not linked into the `keyhammer` crates, the wasm module or anything the project distributes). Their licences were read from each package's `package.json` `license` field and its `LICENSE` file; `npm ls --all` lists no transitive dependencies.

| Package | Version | Licence | Copyright notice in its LICENSE file |
|---|---|---|---|
| `minisearch` | 7.2.0 | MIT | Luca Ongaro |
| `fuse.js` | 7.5.0 | Apache-2.0 | (the Apache License 2.0 text) |
| `@leeoniya/ufuzzy` | 1.0.19 | MIT | Leon Sorokin |
| `fuzzysort` | 4.0.2 | MIT | Stephen Kamenar |

MIT is compatible with AGPL-3.0-or-later. Apache-2.0 is compatible with version 3 of the GPL and the AGPL (Apache-2.0 code may be included in such works; the reverse is not allowed), and this project is AGPL-3.0-or-later. Since these packages are only installed to run the benchmark and are not redistributed with the project, no notice obligation arises from this work; if they were ever bundled, their licence texts would have to travel with them. The older `bench/package.json` (used by the legacy `compare.mjs`) already lists the same four libraries, with caret ranges; the new folder pins exact versions.

## How to reproduce

From the repository root:

```sh
# 1. the WebAssembly module (paths remapped as in bindings/wasm/README.md; on Windows pass the paths in the form rustc sees them)
remap="--remap-path-prefix=$PWD=/src"
remap="$remap --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo"
remap="$remap --remap-path-prefix=${RUSTUP_HOME:-$HOME/.rustup}=/rustup"
RUSTFLAGS="$remap" cargo build -p keyhammer-wasm --target wasm32-unknown-unknown --profile wasm
node bindings/wasm/test.mjs

# 2. the data (shared with the Rust benchmark)
cd bench
npm ci
node fetch-data.mjs           # Birkbeck, big.txt (SHA-256 checked)
node prepare-m0-data.mjs      # dictionaries and tests.tsv
node fetch-typo-corpora.mjs   # gtc.tsv and wiki.tsv (pinned revisions, SHA-256 checked)

# 3. the benchmark
cd js-competitors
npm ci                        # pinned versions, lockfile committed
node --expose-gc --max-old-space-size=8192 run.mjs --help
node --expose-gc --max-old-space-size=8192 run.mjs --skip-latency   # quality, build time, memory, load time
node --expose-gc --max-old-space-size=8192 run.mjs --latency-only   # latency; run it last, on a quiet machine
```

The harness reads `../data` by default and the module from `target/wasm32-unknown-unknown/wasm/keyhammer_wasm.wasm` (`--data`, `--corpus-dir` and `--wasm` change that) and prints Markdown tables. `--sizes`, `--corpora`, `--engines`, `--queries`, `--repetitions`, `--min-queries`, `--max-run-seconds`, `--builds` and `--load-runs` restrict or extend a run; `--builds-only` prints only the build, memory and load tables. CI runs a small smoke configuration (`--sizes 10000 --corpora birkbeck --queries 20`) so that the script keeps working; that run is not the benchmark. The tables above were assembled from the harness output of 2026-09-24.
