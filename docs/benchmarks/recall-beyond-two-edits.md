# Recall beyond two edits and phonetic candidates, 2026-09-24

Issue #22 asked two things. (1) How much of what the default budget (32, two ordinary edits) cannot reach is a matter of three or more edits, and what would budgets 48 and 64 buy and cost? (2) How much is a matter of spelling confusions (ie/ei, ph/f, double letters, silent letters) that an edit-distance model does not reach cheaply, and does a phonetic or spelling-rule candidate source help? Everything here is measured with harnesses in `bench/` (nothing in `crates/keyhammer` changed); numbers are from one machine and one build, and anything not measured is marked as a hypothesis.

## Summary

- **Reach.** On the 300 Birkbeck pairs 132 (44.0%) are unreachable at budget 32 (the right word's exact cost is above 32). By unit edit distance (OSA) between the typo and the right word: 5 of them are 2 edits away (only first-letter or non-neighbour costs push them above 32), 52 are 3 edits and 75 are 4 or more. Budget 48 reaches 56 of the 132 (all but 4 of the 3-or-fewer-edit ones, plus 3 of the 4+ group); budget 64 reaches 94; 38 pairs (12.7% of all) stay out of reach at 64. So, on Birkbeck, most of the unreachable set is **3 or more unit edits**, not something a cheaper edit model would fix.
- **Budgets.** At 274 137 words, R@10 / MRR@10 on Birkbeck are 0.517 / 0.429 (32), 0.620 / 0.493 (48) and 0.643 / 0.505 (64). Going 48 to 64 adds +0.0121 MRR (SE 0.0054) and +0.0233 R@10 (SE 0.0087) for about twice the nodes (4590 to 9124 per query) and 3.8 times the p95 (3884 to 14 804 us, on a busy machine, see Latency). On the GitHub Typo Corpus and Wikipedia samples at 274 137 words budget 64 adds at most +0.0010 MRR over 48 (SE 0.0010 or less; 0.0000 on Wikipedia at all sizes; the largest gain on the smaller dictionaries is +0.0018, SE 0.0012, on GitHub at 10 000 words). This confirms the earlier proposal: keep 32 as the default, keep 48 as the opt-in `high_recall()` preset, do not offer 64.
- **Phonetic and spelling-rule candidates (prototypes outside the core, held-out tests).** Handwritten and mined spelling-substitution rules used as extra low-cost operations gained almost nothing: +0.0040 MRR (SE 0.0015) on a 3000-pair Birkbeck sample in a two-fold split, +0.0089 (SE 0.0064, not significant) on the 300 untouched M0 pairs when trained on that sample, zero on Wikipedia and GitHub (nothing was admitted on training data), and a significant loss when trained on Birkbeck and applied to Wikipedia (-0.0045, SE 0.0010) or GitHub (-0.0042, SE 0.0007). They cost 2 to 3.7 times the default's nodes. The one useful family was cheap vowel-for-vowel substitutions (e/i, i/e, a/e at cost 8).
- **Phonetic key injection** (Soundex key bucket, ranked by unit edit distance, up to 5 slots after the engine's first 5 hits) gained as much as the budget-48 preset on Birkbeck: +0.0587 MRR (SE 0.0038) and +0.1120 R@10 (SE 0.0063) in the two-fold split of the Birkbeck sample; on the untouched M0 pairs +0.0652 (SE 0.0136) with the configuration selected on Wikipedia only (+0.0685, SE 0.0137, when selected on the Birkbeck sample). Against the preset the paired MRR differences are +0.0043 (SE 0.0034) on the sample and +0.0013 (SE 0.0107) on M0 (Wikipedia-selected), i.e. no measurable difference. About two thirds of the sample's MRR gain (+0.0387 of +0.0587) comes from queries where the right word ends at rank 1, mostly queries for which budget 32 returns no hit or fewer than 5 (see the diagnostic below). It does so at the default's search work plus about 170 unit-distance evaluations per query (mean Soundex bucket), which are not timed here. On the two corpora of mostly one- or two-edit errors it is small: +0.0041 (SE 0.0011) MRR on GitHub, +0.0026 (SE 0.0008) on Wikipedia, and R@10 can go down when trained on Birkbeck and applied to GitHub (-0.0023, SE 0.0029, not significant).
- **Verdict.** Do not add spelling-rule operations to the cost model (the gain is within noise where it was independent and the price is 2 to 3.7 times the nodes). Phonetic candidates may be a lower-node alternative to budget 48 for spelling-type errors (latency not measured), but this evidence is one spelling-heavy corpus with an English-only key; it justifies a follow-up (below), not a core change. Exploratory: the protocol was fixed in the source but not committed as a separate pre-registration (unlike `tiebreak-preregistration.md`). Nothing here changes the public API.

## Method

### Data (all outside the repository)

| corpus | pairs | how it was built |
|---|---|---|
| Birkbeck, 300 M0 pairs | 300 | `tests.tsv` of the M0 report (`prepare-m0-data.mjs`). Used for the budget sweep and as an untouched test set in the phonetic experiments. |
| Birkbeck sample | 3000 | From `missp.dat` (Birkbeck spelling error corpus): same filter as M0 (typo `^[a-z]{3,}$`, right word `^[a-z]{3,}$` and in `words-full.tsv`, typo not in it, typo different from the word), unique pairs, excluding the 300 M0 pairs: 29 266 usable pairs, a fixed-seed shuffle (LCG seed 20260926) and the first 3000. |
| GitHub Typo Corpus | 1000 (sweep), 3000 (phonetic: `gtc.tsv` plus `gtc-holdout.tsv`) | `fetch-typo-corpora.mjs --holdout`, pinned revisions and SHA-256 as in that script. |
| Wikipedia list | 1000 (sweep), 3754 (phonetic: `wiki.tsv` plus `wiki-holdout.tsv`, i.e. all usable pairs) | same script. |
| finger slips | not run | The corpus needs a 1.5 GB download under a non-commercial licence (`docs/benchmarks/finger-slips.md`) and is not present here. That corpus is filtered to at most 2 unit edits, so budgets above 32 can only matter through first-letter costs; this is a hypothesis, not measured. |

Dictionaries for the sweep: `words-10000.tsv` and `words-100000.tsv` for Birkbeck (they contain the Birkbeck targets, as in M0); for the other corpora a dictionary of the same size that contains all the corpus's right words plus a deterministic random fill (seed 20260924) from `words-full.tsv`; the full 274 137 words for all. The phonetic experiments use the full dictionary only.

### Harnesses

- `bench/src/bin/recall.rs`: the budget sweep and the unreachable-pair decomposition; `--latency` adds the timing run. `SearchConfig { k: 10, budget, tsb: true, ..default }`, `Coarse` ranking, QWERTY costs. "Reachable at B" means the right word's exact cost is at most B, taken from one search per query with budget 64, `Ranking::Exact`, k = 500 000 (never truncated, asserted). The paired difference is between budgets on the same queries (mean of per-query differences, SE = sample standard deviation over the square root of n).
- `bench/src/bin/phonetic.rs`: the phonetic and rule experiments (protocol below). `bench/src/recall_common.rs` holds the shared code, including a Metaphone-like key and Soundex written from the published algorithms before any data was looked at.

### Phonetic and rule experiments: what was built and how choices were made

All three methods sit outside the core and use only the public API (`Trie::term`, `weight`, `Searcher::search`).

1. **Rules as extra operations** (`rules, handwritten list` and `rules, mined from training pairs`). A rule `from -> to` with cost r rewrites the query at each occurrence of `from`, the rewritten query is searched with the unchanged engine at budget 32 (k = 30), the hits' costs are shifted by r, hits above 32 are dropped, and the result is merged with the plain search by lowest cost per term and ranked as `Coarse` does (whole units of 16, then weight, then id). This is the same result a core operation would give apart from the k = 30 truncation per variant and from rule applications that overlap other edits. The handwritten list (122 directions from 61 alternations: ie/ei, ph/f, ck/k, c/k, c/s, s/z, doubled consonants, kn/n, gn/n, wr/r, mb/m, gh, h, wh/w, single vowel swaps a/e, e/i, i/y, o/u, a/o, u/a, endings ance/ence, ant/ent, able/ible, er/or, ar/er, ary/ery, tion/sion, cion/tion, ch/sh, tch/ch, dge/ge, x, qu/kw, j/g, ce/se, ci/si, sc, ps, y/ie, ee/ea, oo/ou, ai/ay, ea/e, ou/o, au/o, ui/u, ure/er, eur/er) is fixed in the source. The mined list takes each training pair, cuts the longest common prefix and suffix, keeps the residue (typo side at most 3 letters, right side at most 3) with 0 or 1 letter of context on each side, and keeps the 80 most frequent with count at least 4 in the training half.
2. **Phonetic key injection** (`phonetic key injection`). Every dictionary word is indexed by its Metaphone-like key and by its Soundex key. For a query, the words sharing the query's key are ranked by unit OSA distance to the query, then higher weight, then id; the top `m` that are not already in the engine's first min(`10 - m`, hits) take the next `m` slots, so when the engine returns fewer than `10 - m` hits within budget 32 (0 hits for 376 of the 3000 sample queries) the phonetic candidates rank higher, first when it returns none (remaining engine hits fill any slots left). A gate can restrict this to queries whose best engine cost is above 16 or 24.
3. **Selection on the training half only.** Rules: greedy forward selection over (rule, cost in {8, 16, 24}), adding the pair with the largest paired gain in training MRR@10 provided its paired z (gain over its standard error) is at least 2, up to 20 rules; if none qualifies, none is used. Phonetic: a grid of key (2) x m in {1, 2, 3, 5} x gate in {always, >16, >24}; the best training MRR is used if its paired z against no injection is at least 2, otherwise nothing. The grid, the thresholds and the seeds were fixed in the source before the first scoring run. The scoring run showed the selected `m` at the edge of the grid (5) for Birkbeck; the grid was not widened afterwards. After that first run only reporting was added (bucket sizes, rule coverage, the "vs preset" column); no method, threshold or split changed.
4. **Held-out protocol.** Within a corpus: a fixed shuffle (seed 20260925) into two halves, selection on one and scoring on the other, and the reverse; each query is scored once, with the configuration selected without it, and the tables pool the two test halves. Across corpora: selection on the whole of one corpus (Wikipedia, or the Birkbeck sample) and scoring on the others, including the 300 M0 pairs, which no selection ever saw.

## Results

### 1a. Budgets 32, 48 and 64

R@10 is the share of queries with the right word among the 10 returned; MRR@10 as in the earlier reports. "dMRR" and "dR@10" are paired differences against the previous budget row (SE in brackets). No search reached the node limit.

Birkbeck (300 M0 pairs); reachable pairs are 168 / 224 / 262 of 300 at every dictionary size (they do not depend on it):

| words | budget | R@10 | MRR@10 | nodes/query | dMRR vs previous (SE) | dR@10 vs previous (SE) |
|---|---|---|---|---|---|---|
| 10 000 | 32 | 0.560 | 0.541 | 491 | | |
| 10 000 | 48 | 0.727 | 0.660 | 2551 | +0.1199 (0.0175) | +0.1667 (0.0216) |
| 10 000 | 64 | 0.810 | 0.708 | 5966 | +0.0478 (0.0110) | +0.0833 (0.0160) |
| 100 000 | 32 | 0.550 | 0.470 | 911 | | |
| 100 000 | 48 | 0.670 | 0.552 | 4610 | +0.0818 (0.0148) | +0.1200 (0.0188) |
| 100 000 | 64 | 0.713 | 0.575 | 10 495 | +0.0223 (0.0077) | +0.0433 (0.0118) |
| 274 137 | 32 | 0.517 | 0.429 | 995 | | |
| 274 137 | 48 | 0.620 | 0.493 | 4590 | +0.0639 (0.0130) | +0.1033 (0.0176) |
| 274 137 | 64 | 0.643 | 0.505 | 9124 | +0.0121 (0.0054) | +0.0233 (0.0087) |

The 32 to 48 rows reproduce `recall-preset.md` (0.0639, SE 0.0130; 995 and 4590 nodes). The budget-64 row reproduces the issue's spike numbers for nodes (9124) and MRR (0.505); the 48 to 64 gain is +0.0121 here, as in the issue.

Reachable does not mean found: at 274 137 words and budget 64, 262 pairs are within reach but R@10 is 0.643 (193 pairs). The other 69 are ranked below 10 by the engine's ranking (mostly common words with lower edit cost crowd them out).

GitHub Typo Corpus (1000 pairs), reachable 939 / 966 / 981:

| words | budget | R@10 | MRR@10 | nodes/query | dMRR vs previous (SE) | dR@10 vs previous (SE) |
|---|---|---|---|---|---|---|
| 10 000 | 32 / 48 / 64 | 0.939 / 0.963 / 0.966 | 0.910 / 0.932 / 0.934 | 473 / 2396 / 5297 | +0.0213 (0.0044) / +0.0018 (0.0012) | +0.0240 (0.0048) / +0.0030 (0.0017) |
| 100 000 | 32 / 48 / 64 | 0.930 / 0.952 / 0.953 | 0.857 / 0.871 / 0.872 | 906 / 4238 / 8271 | +0.0141 (0.0033) / +0.0010 (0.0010) | +0.0220 (0.0046) / +0.0010 (0.0010) |
| 274 137 | 32 / 48 / 64 | 0.914 / 0.935 / 0.936 | 0.803 / 0.811 / 0.812 | 981 / 3885 / 6345 | +0.0078 (0.0022) / +0.0010 (0.0010) | +0.0210 (0.0045) / +0.0010 (0.0010) |

Wikipedia list (1000 pairs), reachable 983 / 995 / 996:

| words | budget | R@10 | MRR@10 | nodes/query | dMRR vs previous (SE) | dR@10 vs previous (SE) |
|---|---|---|---|---|---|---|
| 10 000 | 32 / 48 / 64 | 0.983 / 0.995 / 0.995 | 0.965 / 0.973 / 0.973 | 469 / 2598 / 6384 | +0.0077 (0.0025) / +0.0000 (0.0000) | +0.0120 (0.0034) / +0.0000 (0.0000) |
| 100 000 | 32 / 48 / 64 | 0.982 / 0.992 / 0.992 | 0.936 / 0.941 / 0.941 | 939 / 5130 / 10 302 | +0.0048 (0.0019) / +0.0000 (0.0000) | +0.0100 (0.0031) / +0.0000 (0.0000) |
| 274 137 | 32 / 48 / 64 | 0.978 / 0.984 / 0.984 | 0.905 / 0.909 / 0.909 | 1063 / 4917 / 8046 | +0.0040 (0.0018) / +0.0000 (0.0000) | +0.0060 (0.0024) / +0.0000 (0.0000) |

Where the errors are mostly one or two edits (these two corpora), the default already reaches 94 to 98% and there is little to buy; budget 64 buys nothing measurable over 48.

### 1a (continued). What the unreachable pairs are (274 137 words)

| corpus | pairs | unreachable at 32 | at 48 | at 64 |
|---|---|---|---|---|
| Birkbeck (300) | 300 | 132 (44.0%) | 76 | 38 |
| GitHub (1000) | 1000 | 61 (6.1%) | 34 | 19 |
| Wikipedia (1000) | 1000 | 17 (1.7%) | 5 | 4 |

Birkbeck (300) by unit OSA distance between typo and right word. "Reached at 48/64" counts the pairs unreachable at 32 that a larger budget brings within reach; the last two columns count those unreachable at 32 whose typo and right word have the same key:

| unit OSA distance | pairs | unreachable at 32 | reached at 48 | reached at 64 | still unreachable at 64 | same Metaphone key | same Soundex key |
|---|---|---|---|---|---|---|---|
| 1 | 90 | 0 | 0 | 0 | 0 | 0 | 0 |
| 2 | 80 | 5 | 5 | 5 | 0 | 2 | 0 |
| 3 | 55 | 52 | 48 | 52 | 0 | 15 | 24 |
| 4 or more | 75 | 75 | 3 | 37 | 38 | 12 | 20 |

Of the 38 pairs unreachable at 64, 3 share the Metaphone-like key of the right word (over all 300 pairs, 126 do, most of them already reachable). On the GitHub and Wikipedia samples, 0 of the 19 and 0 of the 4 pairs left at 64 share it. Why the 5 two-edit pairs are unreachable was not examined pair by pair (hypothesis: the x1.5 factor on the first byte, so an ordinary edit there costs 24).

### 1b. How phonetic are the unreachable pairs? (274 137 words)

Among the pairs unreachable at 32, how many have the right word in the same key bucket as the query, and in the first 10 of the bucket ranked by unit edit distance then weight (which is what the injection uses):

| corpus | pairs | unreachable at 32 | Metaphone bucket holds the word | in its top 10 | Soundex bucket | in its top 10 | reachable at 32 but not in the default's top 10 |
|---|---|---|---|---|---|---|---|
| Birkbeck M0 | 300 | 132 | 29 | 27 | 44 | 38 | 13 |
| Birkbeck sample | 3000 | 1315 | 242 | 216 | 451 | 361 | 110 |
| Wikipedia | 3754 | 51 | 14 | 14 | 28 | 23 | 17 |
| GitHub | 3000 | 163 | 6 | 6 | 41 | 31 | 71 |

So on Birkbeck about a fifth (Metaphone-like) to a third (Soundex) of the pairs the default cannot reach share a coarse phonetic key with the right word; the finer key is more precise and covers less. Spelling rules, by comparison, are narrower: how many of the same pairs a single handwritten rule (applied once, at any position) would bring within budget 32:

| corpus | unreachable at 32 | rule at cost 8 | at 16 | at 24 |
|---|---|---|---|---|
| Birkbeck M0 | 132 | 13 | 2 | 0 |
| Birkbeck sample | 1315 | 134 | 36 | 2 |
| Wikipedia | 51 | 6 | 2 | 2 |
| GitHub | 163 | 6 | 3 | 0 |

(A lower bound: each variant search keeps 30 hits.) Even at the cheapest cost only about a tenth of the unreachable Birkbeck pairs are one handwritten spelling rule away. The rest need several changes or changes outside the list.

### 2. Held-out tests of the prototypes

Every table pools test scores; "default" is budget 32, "preset" is budget 48 (`high_recall()`), all with the subtree bound on and `Coarse` ranking. The nodes column counts only search nodes (for the rules, the plain search plus every variant search); the phonetic method adds candidate scans that are not nodes (see the cost table below).

**Within-corpus two-fold, Birkbeck sample (n = 3000, 1500 training and 1500 test pairs per fold).** Selected on the training halves: `e->i @8` (fold 1); `i->e @8, a->e @8` (fold 2), by both the handwritten list and the mined candidates; phonetic: Soundex key, m = 5, no gate (both folds).

| method | MRR@10 | R@1 | R@10 | nodes/query | dMRR vs default (SE) | dR@10 vs default (SE) | dMRR vs preset (SE) |
|---|---|---|---|---|---|---|---|
| default (budget 32) | 0.422 | 0.367 | 0.525 | 1116 | | | |
| preset (budget 48) | 0.476 | 0.403 | 0.627 | 4405 | +0.0544 (0.0036) | +0.1017 (0.0055) | |
| rules, handwritten list | 0.426 | 0.370 | 0.530 | 2514 | +0.0040 (0.0015) | +0.0050 (0.0015) | -0.0504 (0.0037) |
| rules, mined from training pairs | 0.426 | 0.370 | 0.530 | 2514 | +0.0040 (0.0015) | +0.0050 (0.0015) | -0.0504 (0.0037) |
| phonetic key injection | 0.481 | 0.405 | 0.637 | 1116 | +0.0587 (0.0038) | +0.1120 (0.0063) | +0.0043 (0.0034) |

**Within-corpus two-fold, Wikipedia (n = 3754) and GitHub (n = 3000).** Rules: none admitted in any fold (handwritten or mined), so those rows equal the default. Phonetic: Soundex, m = 3 (gate >16 in fold 1, none in fold 2) on Wikipedia; Soundex, m = 5, gate >16 on GitHub.

| corpus | method | MRR@10 | R@1 | R@10 | dMRR vs default (SE) | dR@10 vs default (SE) | dMRR vs preset (SE) |
|---|---|---|---|---|---|---|---|
| Wikipedia | default | 0.908 | 0.856 | 0.982 | | | |
| Wikipedia | preset | 0.912 | 0.858 | 0.989 | +0.0032 (0.0008) | +0.0067 (0.0013) | |
| Wikipedia | phonetic | 0.911 | 0.858 | 0.986 | +0.0026 (0.0008) | +0.0040 (0.0015) | -0.0006 (0.0006) |
| GitHub | default | 0.811 | 0.740 | 0.922 | | | |
| GitHub | preset | 0.816 | 0.744 | 0.933 | +0.0053 (0.0011) | +0.0113 (0.0019) | |
| GitHub | phonetic | 0.815 | 0.743 | 0.929 | +0.0041 (0.0011) | +0.0067 (0.0016) | -0.0012 (0.0008) |

**Cross-corpus, the 300 M0 pairs (no selection ever saw these pairs, but see the overlap of intended words below; n = 300).** Trained on the whole Wikipedia list (nothing admitted for the rules; phonetic: Soundex, m = 3, no gate), and trained on the Birkbeck sample (rules `e->i @8, i->e @8, a->e @8`; phonetic: Soundex, m = 5, no gate):

| trained on | method | MRR@10 | R@1 | R@10 | nodes/query | dMRR vs default (SE) | dR@10 vs default (SE) | dMRR vs preset (SE) |
|---|---|---|---|---|---|---|---|---|
| | default | 0.429 | 0.380 | 0.517 | 1139 | | | |
| | preset | 0.493 | 0.430 | 0.620 | 4590 | +0.0639 (0.0130) | +0.1033 (0.0176) | |
| Wikipedia | phonetic | 0.494 | 0.437 | 0.613 | 1139 | +0.0652 (0.0136) | +0.0967 (0.0195) | +0.0013 (0.0107) |
| Birkbeck sample | rules (both lists) | 0.438 | 0.390 | 0.523 | 4010 | +0.0089 (0.0064) | +0.0067 (0.0067) | -0.0550 (0.0130) |
| Birkbeck sample | phonetic | 0.498 | 0.437 | 0.630 | 1139 | +0.0685 (0.0137) | +0.1133 (0.0217) | +0.0046 (0.0109) |

The `default` row here is a k = 30 search cut to 10 (the phonetic and rule code merges lists of 30), hence 1139 nodes per query against 995 at k = 10 in the budget sweep; results are the same.

The Wikipedia-trained configuration is the cleanest independent test: neither the corpus nor the selection saw Birkbeck at all, and it still gains +0.0652 MRR (SE 0.0136) on the 300 pairs.

**Cross-corpus, the other direction (Birkbeck-trained rules and phonetic applied to Wikipedia and GitHub).** Rules: -0.0045 MRR (SE 0.0010) on Wikipedia and -0.0042 (SE 0.0007) on GitHub, significant losses, at 3.5 to 3.7 times the nodes. Phonetic: +0.0021 (SE 0.0008) and +0.0032 (SE 0.0011) MRR, R@10 +0.0003 (SE 0.0019) and -0.0023 (SE 0.0029). Trained on Wikipedia and applied to the Birkbeck sample (n = 3000): phonetic +0.0570 (SE 0.0037) MRR, +0.1057 (SE 0.0058) R@10; on GitHub +0.0038 (SE 0.0011) MRR, +0.0023 (SE 0.0024) R@10.

**Where the phonetic gain comes from** (printed by `phonetic` after each pooled or cross-corpus table; "phonetic rank" is the rank of the right word in the final list). Birkbeck sample, two-fold pooled (n = 3000): the default returns 0 hits for 376 queries and fewer than 5 for 1243. On M0 (n = 300): 0 hits for 43, fewer than 5 for 126.

| test set | rank of the right word | queries gained | contribution to the MRR gain (sum / n) |
|---|---|---|---|
| Birkbeck sample | 1 | 116 | +0.0387 |
| Birkbeck sample | 2 to 5 | 129 | +0.0154 |
| Birkbeck sample | 6 to 10 | 146 | +0.0059 |
| Birkbeck sample | queries that got worse (30) | | -0.0013 |
| M0, Wikipedia-selected | 1 | 17 | +0.0567 |
| M0, Wikipedia-selected | 2 to 5 | 5 | +0.0057 |
| M0, Wikipedia-selected | 6 to 10 | 11 | +0.0043 |
| M0, Wikipedia-selected | queries that got worse (4) | | -0.0015 |

So the method acts mainly as a fallback on empty or short engine lists; it is not mostly filling low slots. The harness prints the same table for every other test set.

**Cost of the phonetic candidates.** The unit-distance evaluations per query are the size of the query's key bucket (274 137 words):

| corpus | key | mean | median | p95 | max |
|---|---|---|---|---|---|
| Birkbeck M0 | Metaphone-like | 15 | 4 | 67 | 183 |
| Birkbeck M0 | Soundex | 161 | 120 | 473 | 905 |
| Birkbeck sample | Soundex | 168 | 114 | 483 | 1626 |
| Wikipedia | Soundex | 202 | 126 | 640 | 1626 |
| GitHub | Soundex | 195 | 119 | 533 | 1626 |

Soundex was selected in every case. Its buckets hold on average 160 to 200 words per query, each needing one unit edit distance (a full dynamic-programming matrix in this prototype) and a sort; compare the roughly 1000 trie nodes the default expands. The time was not measured (timing was limited to the budget sweep in this run), so the claim that the phonetic route is cheaper than budget 48 is a node-count statement plus this scan count, not a latency result (hypothesis for latency).

### Latency of the budget sweep

One timed search per query after one untimed pass; p50 and p95 in microseconds; median of three rounds (the round values of p95 are shown); `k = 10`, bound on. Run last, in one go, on a machine that was **not quiet**: while it ran, other agents' builds and tests were on the same 16-thread machine (34 samples of the total CPU load every 5 seconds: mean 53%, maximum 99%, up to 17 `rustc`/`cargo` processes at a time). A quiet-machine check before the run showed load of 8 to 40% with `cargo` and `rustc` processes present, and the machine did not become quiet, so the run was not repeated on an idle machine. Absolute times are therefore inflated and noisy (the rounds of the 64 budget vary by up to 40%); the ratios between budgets are more stable and agree with `recall-preset.md`, measured on an idle machine (p95 401 us and 2945 us at 32 and 48, ratio 7.3x, at 274 137 words; here 574 us and 3884 us, 6.8x).

| corpus | words | budget | p50 | p95 | p95 / p95 at 32 | p95 per round |
|---|---|---|---|---|---|---|
| Birkbeck | 10 000 | 32 | 94 | 148 | 1.0x | 147 / 151 / 148 |
| Birkbeck | 10 000 | 48 | 528 | 770 | 5.2x | 744 / 770 / 930 |
| Birkbeck | 10 000 | 64 | 1434 | 2818 | 19.0x | 2818 / 2191 / 3999 |
| Birkbeck | 100 000 | 32 | 297 | 473 | 1.0x | 473 / 483 / 457 |
| Birkbeck | 100 000 | 48 | 1635 | 3224 | 6.8x | 3224 / 3613 / 3196 |
| Birkbeck | 100 000 | 64 | 2646 | 11 255 | 23.8x | 12 769 / 11 115 / 11 255 |
| Birkbeck | 274 137 | 32 | 356 | 574 | 1.0x | 565 / 574 / 588 |
| Birkbeck | 274 137 | 48 | 1272 | 3884 | 6.8x | 4348 / 3730 / 3884 |
| Birkbeck | 274 137 | 64 | 1624 | 14 804 | 25.8x | 14 987 / 14 804 / 13 805 |
| GitHub | 10 000 | 32 | 100 | 169 | 1.0x | 213 / 169 / 166 |
| GitHub | 10 000 | 48 | 636 | 980 | 5.8x | 1054 / 924 / 980 |
| GitHub | 10 000 | 64 | 1281 | 2844 | 16.8x | 2609 / 2844 / 2896 |
| GitHub | 100 000 | 32 | 275 | 451 | 1.0x | 451 / 454 / 450 |
| GitHub | 100 000 | 48 | 1376 | 3043 | 6.7x | 3357 / 2941 / 3043 |
| GitHub | 100 000 | 64 | 2005 | 10 826 | 24.0x | 11 472 / 10 826 / 10 259 |
| GitHub | 274 137 | 32 | 346 | 575 | 1.0x | 624 / 575 / 524 |
| GitHub | 274 137 | 48 | 944 | 3892 | 6.8x | 3892 / 3630 / 4419 |
| GitHub | 274 137 | 64 | 1158 | 11 813 | 20.6x | 11 813 / 12 130 / 11 072 |
| Wikipedia | 10 000 | 32 | 118 | 180 | 1.0x | 176 / 180 / 180 |
| Wikipedia | 10 000 | 48 | 634 | 932 | 5.2x | 932 / 920 / 1023 |
| Wikipedia | 10 000 | 64 | 1728 | 2857 | 15.9x | 2748 / 2857 / 2996 |
| Wikipedia | 100 000 | 32 | 299 | 481 | 1.0x | 481 / 459 / 490 |
| Wikipedia | 100 000 | 48 | 2021 | 3278 | 6.8x | 3278 / 3267 / 4674 |
| Wikipedia | 100 000 | 64 | 3392 | 11 921 | 24.8x | 11 921 / 11 011 / 11 941 |
| Wikipedia | 274 137 | 32 | 414 | 654 | 1.0x | 1107 / 654 / 623 |
| Wikipedia | 274 137 | 48 | 1869 | 4370 | 6.7x | 4370 / 4365 / 4545 |
| Wikipedia | 274 137 | 64 | 2695 | 15 460 | 23.6x | 15 460 / 15 560 / 15 045 |

Budget 64 costs 16 to 26 times the default's p95 (the issue reported 24x at 274 137 words); budget 48 costs 5 to 7 times.

## Verdict

1. **Budget 64: do not offer it.** On Birkbeck it reaches 38 more pairs than 48 (262 against 224 of 300) but the gain in ranked results is +0.0121 MRR (SE 0.0054) and +0.0233 R@10 (SE 0.0087) at 274 137 words, for about twice the nodes and 3.8 times the p95 of 48 (a noisy measurement); on the other two corpora it is +0.001 or less. Budget 48 and the default stay as they are. Budget 64 stays accepted by the engine.
2. **Three or more edits is where the unreachable Birkbeck pairs are** (127 of the 132), and budgets 48 and 64 are the only tool measured that reaches them within the current model.
3. **Spelling-rule operations: do not add to the cost model.** The rules that helped were cheap vowel substitutions; the gain is +0.004 on a spelling corpus and not significant on the untouched pairs, negative elsewhere, and the price is 2 to 3.7 times the nodes.
4. **Phonetic candidates: promising, unproven.** No measurable difference from the preset on Birkbeck at about a quarter of the preset's nodes plus a scan of a key bucket, positive but small on the two corpora of mostly short edits, and the cross-corpus test on the 300 untouched pairs supports it. The follow-up below tests it where it should matter or fail.

## Caveats

- **Birkbeck is spelling-heavy** (4 or more unit edits in a quarter of the M0 pairs), and the Wikipedia list of misspellings and the GitHub corpus are mostly one or two edits. The budget gains and the phonetic gains are large only on the first; nothing here is evidence about finger slips (not run) or about other languages (the keys are English-only).
- **MRR versus R@10.** Words newly found by a larger budget land low in the top 10, so R@10 rises about 1.6 times as much as MRR@10 (+0.1033 R@10 against +0.0639 MRR for 48 on M0). Phonetic gains are the opposite: mostly at rank 1, on queries the default leaves empty or short. A phonetic slot can still push out a correct word that stood at rank 6 to 10 (R@10 -0.0023 on GitHub, not significant, for Birkbeck-trained phonetic), which MRR barely sees. Report both.
- **Small numbers of pairs decide the small differences.** The M0 test has 300 pairs, and the paired SEs are normal-approximation; differences below about 0.01 there are not resolvable (the phonetic-against-preset SE on M0 is 0.0107). The Birkbeck sample has 3000 pairs from the same population as the selection halves, so its two-fold result is an in-distribution held-out result; the Wikipedia-trained result on M0 is the out-of-distribution one. Splits are by pair, not by intended word: 2098 of the 3000 pooled test pairs of the Birkbeck sample have their right word among the training pairs' right words, and so do 238 of the 300 M0 pairs when trained on the sample and 128 of 300 when trained on Wikipedia. The Wikipedia-trained M0 result is the least affected, not free of it.
- **Selection sees the training half only, but the design was not pre-registered publicly** (no committed protocol before the run, unlike `tiebreak-preregistration.md`); the method and thresholds were fixed in the source before the first scoring run, and only reporting was added after it. The selected `m = 5` is at the top of the grid, so a larger `m` might do better; that was not explored, deliberately, to avoid tuning on test results.
- **Prototype approximations.** Rule variants keep 30 hits each and are merged by cost; the rules are applied one at a time (one rule per variant, at one position), never combined. The phonetic ranking uses unit OSA distance, not the engine's keyboard costs. Metaphone-like and Soundex were written from published descriptions and are not validated against a reference implementation. Soundex, Metaphone-like and the rule list are English-only; Portuguese and other languages are not covered.
- **Dictionaries.** Words with no frequency in the source text have weight 0, so ties in ranking are broken by term id often; results at smaller dictionaries for the non-Birkbeck corpora use a constructed dictionary (targets plus random fill), where the targets are a large share of the dictionary at 10 000 words. The phonetic experiments use the full dictionary only.
- **One machine, one build, busy while timed** (see Latency). The node counts and quality numbers are deterministic; latency is not.
- **Not run:** finger-slip corpus (not available), phonetic latency, phonetic at 10 000 and 100 000 words, combinations (rules with phonetic, phonetic with the preset), rule application depth above one, non-English data.

## Proposals (text only; nothing was implemented in the core)

**Follow-up A, phonetic fallback candidates, no core change needed.** The prototype uses only `Trie::len`, `term`, `weight` and `Searcher::search`. If pursued, keep it outside `keyhammer` (a companion module, a binding, or an `examples/` recipe): a key-to-term-ids map built from the same term list, and the injection policy of this report. Before adopting anything: (1) measure its latency (bucket scan plus merge) against budget 48 on a quiet machine; (2) evaluate on further spelling corpora (the Aspell and Holbrook sets are candidates) and on real finger slips with the licence question resolved; (3) test one more key (a longer Metaphone, a phonetic-skeleton key) and a wider `m`, chosen on training data only; (4) decide the interaction with `high_recall()`.

**Follow-up B, only if rules are ever wanted: a substitution operation in the cost model.** The prototype shows the gain is too small to justify this today, so this is a description, not a recommendation. A rule `(from, to, cost)` with `from` of length 1 to 3 and `to` of length 0 to 3 would need in `search.rs`: (a) a per-query table of the rules whose `from` occurs in the query, built once per search (hash of the first byte to a short list); (b) a dynamic-programming transition from row `i - |from|` to row `i` on the query axis, with the trie side consuming `to` byte by byte, so up to `max |to|` previous trie rows must be kept along the current path (the code already keeps one extra row for the transposition), and the band half-width must widen by `max(|from|, |to|) - 1` positions; (c) the subtree bound and the lower bound (`docs/design/lower-bound.md`) recomputed with the cheapest rule cost included in `c_indel_min`-style minima, otherwise pruning would become unsound; (d) `CostModel` builders for the rule list with costs in the same fixed-point unit, a cap on the number of rules so that the per-query table stays small, and oracle tests (`tests/oracle.rs`) extended with the rules. Because a rule can shrink a length difference the current band already covers, the lower bound of the length difference must also account for `|to| - |from|`. The measured payoff is limited to vowel-for-vowel substitutions, which a cheaper vowel-substitution cost in the existing `sub` operation would capture without any new operation; that alternative was not tested.

## Reproduce

```
node bench/fetch-typo-corpora.mjs --data bench/data --holdout   # after fetch-data.mjs and prepare-m0-data.mjs
cargo run --release -p keyhammer-bench --bin recall   -- bench/data              # budgets, reach (about 85 s)
cargo run --release -p keyhammer-bench --bin phonetic -- bench/data              # prototypes (about 1 min)
cargo run --release -p keyhammer-bench --bin recall   -- bench/data --latency    # run last, quiet machine (about 3 min)
```

`missp.dat`, `big.txt` and the word lists come from `fetch-data.mjs`; nothing under `bench/data` is committed.
