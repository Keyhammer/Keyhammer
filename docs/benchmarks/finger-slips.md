# A finger-slip corpus and the keyboard costs on it, 2026-09-24

Issue #38 asked for a machine-readable corpus of finger slips (typing errors, not spelling errors), a script that downloads it and verifies a SHA-256, its source, licence and size, and counts per operation. Issues #21 and #40 left one question open: do the keyboard-aware costs help on real slips? Those experiments used the GitHub Typo Corpus, which still holds spelling errors, and a synthetic set. This report builds a real corpus from keystroke data and measures that question on it, once. Nothing in the core crate changed.

## Summary

- A real corpus was available and downloadable, so no synthetic data is used here. Source: the 136M Keystrokes dataset (Dhakal et al., CHI 2018). It is free for non-commercial research or projects with attribution, which is not an open-source licence, so the archive and everything derived from it stay out of the repository. `bench/fetch-finger-slips.mjs` rebuilds them locally. The corpus and its derivatives cannot be redistributed. Using them for evaluation only may still conflict with "non-commercial" if the maintainer offers Keyhammer commercially (for example dual licensing); the maintainer must decide this before relying on these results.
- 57 542 unique (typo, intended word) pairs, from 159 124 occurrences typed by 136 959 participants. Counts per category are below. The corpus still contains classic spelling errors (`recieved`, `seperate`); it is a corpus of uncorrected typing errors, not a clean set of finger slips.
- On a fixed-seed sample of 3000 unique pairs (274 137-word dictionary), the shipped engine (keyboard costs, x1.5 first-letter factor, cost budget, `Coarse` ranking) ranked below a plain OSA<=2 baseline: MRR@10 0.841 (default, budget 32) and 0.842 (budget 48) against 0.871, paired difference -0.031 (SE 0.004) and -0.029 (SE 0.004).
- The whole loss is on first-letter pairs (n = 312: -0.308, SE 0.023); where the first letter is untouched (n = 2688) the difference is +0.002 (SE 0.003). The adjacency costs showed no measurable benefit on adjacent-key substitutions, the case they were designed for (n = 484: -0.002, SE 0.006).
- So on this corpus the keyboard-aware costs did not improve ranking, in agreement with #21 and #40. This measurement cannot say which part of the model is responsible (see Limits): the baseline differs from the engine in more than the costs.

## Source, licence, size

| | |
|---|---|
| Dataset | Observations on Typing from 136 Million Keystrokes (Dhakal, Feit, Kristensson, Oulasvirta, CHI 2018, doi 10.1145/3173574.3174220) |
| URL | https://userinterfaces.aalto.fi/136Mkeystrokes/data/Keystrokes.zip |
| Size | 1 572 785 433 bytes (18 854 241 824 bytes unzipped; 168 594 participants in the metadata, 15 sentences each) |
| SHA-256 | `5fb217e0e1273017a6789c5c7ebcc00eed624f2f6426c284da2077a88e270420` (checked by the script; the server's `Last-Modified` is 2018-03-29) |
| Licence | `readme.txt` in the archive: "free to use this data for non-commercial use in your own research or projects with attribution to the authors". Not an open-source licence; not redistributed here; cannot be redistributed by users either, and evaluation-only use may still conflict with "non-commercial" if Keyhammer is offered commercially (the maintainer must decide) |

Each participant transcribed 15 sentences in an online typing test; the archive holds one row per keystroke with the sentence shown (`SENTENCE`) and the text submitted (`USER_INPUT`). The sentences come from a fixed set, so the intended words are drawn from a limited vocabulary: 2258 distinct intended words in the full corpus, 1472 in the sample.

## How the corpus is built

`bench/fetch-finger-slips.mjs` (Node, no dependencies; needs `words-full.tsv` from `prepare-m0-data.mjs`):

1. Download the archive (streamed), verify size and SHA-256, read the ZIP central directory (ZIP64) itself.
2. Keep participants with `LAYOUT = qwerty`, `KEYBOARD_TYPE` full or laptop and `NATIVE_LANGUAGE = en`: 136 960 of 168 594 in the metadata (136 959 files present).
3. For each sentence whose shown and typed texts have the same number of whitespace-separated tokens, compare tokens one by one after lower-casing and stripping punctuation at both ends. Same number of tokens is required so that the pairing is not a guess; sentences with a missing or extra space are skipped.
4. Keep a pair when both words are `^[a-z]+$`, the OSA (Damerau) distance is at most 2 (the engine's budget cannot reach more), the intended word has at least 3 letters and is in `words-full.tsv`, and the typo has at least 2 letters and is not in `words-full.tsv`.
5. Classify with the QWERTY adjacency of `crates/keyhammer/src/cost.rs` (same rows and the same neighbours, re-implemented in the script; it is not called from Rust).

Funnel over occurrences (sentences typed, 2 054 449; 2 013 973 with equal token counts):

| step | occurrences |
|---|---|
| tokens differing only in case or punctuation (dropped) | 251 715 |
| tokens differing in letters | 313 671 |
| dropped: not a-z on either side (apostrophes, digits) | 28 546 |
| dropped: OSA distance above 2 | 11 351 |
| dropped: intended word under 3 letters or typo under 2 | 20 908 |
| dropped: intended word not in the dictionary | 23 514 |
| dropped: the typo is itself a dictionary word (real-word errors) | 70 228 |
| kept | 159 124 |

Dropping typos that are dictionary words follows the other bench corpora. It removes about a fifth of the differing tokens and means that slips producing a real word are absent (adjacent-key substitutions such as `bit` for `but` are affected more than others); the corpus therefore under-represents them. Whether it changes the result was not measured.

Categories (`category` column of `slips.tsv`). Operations are given from the typist's side: an extra letter was typed, a letter was skipped. "Doubled" means the extra or missing letter belongs to a run of equal letters (the case `del_cost` and `ins_cost` price at 8). A transposition is OSA distance 1 by swapping two neighbours. `two_edits` is every pair at distance 2. The `first` column is 1 when the edit touches the first letter of the word.

| category | unique pairs | share of pairs | occurrences | sample (3000) |
|---|---|---|---|---|
| sub_adjacent (substitution by a neighbouring key) | 9 125 | 15.9% | 24 287 | 484 |
| sub_other (substitution by another key) | 5 034 | 8.7% | 12 974 | 288 |
| transposition | 4 692 | 8.2% | 25 942 | 214 |
| extra_letter | 16 289 | 28.3% | 30 913 | 842 |
| extra_doubled | 3 939 | 6.8% | 9 044 | 207 |
| missing_letter | 7 750 | 13.5% | 38 767 | 399 |
| missing_doubled | 322 | 0.6% | 3 694 | 18 |
| two_edits | 10 391 | 18.1% | 13 503 | 548 |
| total | 57 542 | 100% | 159 124 | 3 000 |

The first letter is involved in 6 258 unique pairs (10.9%); 312 of the 3000 in the sample. `first` is 1 for an extra or missing letter only when no non-initial deletion gives the same word (a doubled first letter, `tthe`, is deleted at position 1, which the engine prices without the first-letter factor), and for `two_edits` it means typo[0] differs from correct[0]. The corpus has 4 692 transpositions and 322 missing-doubled pairs, so those two categories are thin only in the sample (214 and 18), not in the corpus.

Outputs: `bench/data/slips.tsv` (SHA-256 `7a88ed8a89b7b93a5c765a229df005b7a552d1f8616fc2c6031ad987814cefb7`, 2 031 177 bytes) and `bench/data/slips-sample.tsv` (`b94ca31eadd9cda44d36a062f32b387e6c349474c3018c39c0332d61ee9b2ba1`, 3000 unique pairs shuffled with seed 20260924; 106 037 bytes). Both depend on `words-full.tsv`; the script checks both hashes (with the default `--sample`) and fails on a mismatch. Columns: `typo, correct, category, first, count, users`.

## What this corpus is not

- It holds uncorrected errors: text the participant submitted. Slips noticed and fixed with backspace are not in it. Errors that survive are the ones the typist did not notice, which may differ from the ones made (doubled letters and transpositions are probably noticed more often; that is a hypothesis, not measured).
- It cannot separate slips from spelling errors. The most frequent pairs are systematic (`recieved` by 672 participants, `seperate` 473, `tommorow` 362). Pairs typed by at most two participants (2414 of the 3000 sampled pairs) are more likely idiosyncratic slips; this is a proxy, not a label. The measurement below is repeated on that subset.
- The stimuli are a fixed set of sentences of short business-style English, so it is one domain and a small vocabulary.
- Words truncated at an early submit (`ther` for `there`, `th` for `the`) may appear as missing-letter pairs.
- Not every mistake produces a pair: token-count mismatches, real-word slips, and edits at distance above 2 are excluded (funnel above).

A keystroke-level extraction (replaying the key log to recover the corrected slips) would address the first point; it was not done here and is left as a follow-up.

## Measurement

Harness: `bench/src/bin/slips.rs`, a small variant of the M0 harness (same trie, `CostModel::qwerty()`, same weights from `big.txt`, the same baseline). Quality only; latency was not measured.

- Systems: the engine with `SearchConfig::default()` plus `tsb: true` (budget 32, `Coarse` ranking; `tsb` does not change results), the engine with `SearchConfig::high_recall()` (budget 48), and the unit-cost baseline: OSA distance at most 2, then higher weight, then lower id, over a full scan.
- Data: the 3000 pairs of `slips-sample.tsv`, dictionary `words-full.tsv` (274 137 words), `k = 10`.
- Metrics: MRR@10, R@1, R@10; paired difference of per-query reciprocal ranks with SE = sample standard deviation of the differences over sqrt(n). One sample, one machine, one run (the quantities are deterministic; the run took 2 min 42 s, dominated by the baseline scan, on a shared machine whose load was not checked, so no time is reported as a result).

All pairs (n = 3000):

| system | MRR@10 | R@1 | R@10 |
|---|---|---|---|
| default (budget 32) | 0.841 | 0.773 | 0.955 |
| high_recall (budget 48) | 0.842 | 0.774 | 0.959 |
| unit-cost baseline | 0.871 | 0.808 | 0.974 |

| paired MRR difference | diff | SE | 95% interval |
|---|---|---|---|
| default - baseline | -0.0305 | 0.0038 | [-0.0380, -0.0230] |
| high_recall - baseline | -0.0289 | 0.0037 | [-0.0363, -0.0216] |
| high_recall - default | +0.0016 | 0.0006 | [+0.0005, +0.0027] |

Pairs typed by at most 2 participants (n = 2414): MRR default 0.831, budget 48 0.833, baseline 0.866; default - baseline -0.0352 (SE 0.0044), budget 48 - baseline -0.0332 (SE 0.0043), budget 48 - default +0.0020 (SE 0.0007).

By category (all 3000 pairs; difference is engine minus baseline, MRR@10, SE in brackets):

| category | n | default | budget 48 | baseline | default - baseline | budget 48 - baseline |
|---|---|---|---|---|---|---|
| sub_adjacent | 484 | 0.896 | 0.896 | 0.898 | -0.002 (0.006) | -0.002 (0.006) |
| sub_other | 288 | 0.855 | 0.855 | 0.879 | -0.024 (0.013) | -0.024 (0.013) |
| transposition | 214 | 0.878 | 0.878 | 0.955 | -0.077 (0.017) | -0.077 (0.017) |
| extra_letter | 842 | 0.927 | 0.927 | 0.966 | -0.039 (0.007) | -0.039 (0.007) |
| extra_doubled | 207 | 0.938 | 0.938 | 0.982 | -0.044 (0.011) | -0.044 (0.011) |
| missing_letter | 399 | 0.820 | 0.820 | 0.858 | -0.037 (0.011) | -0.037 (0.011) |
| missing_doubled | 18 | 0.894 | 0.894 | 0.873 | +0.021 (0.015) | +0.021 (0.015) |
| two_edits | 548 | 0.613 | 0.622 | 0.633 | -0.020 (0.011) | -0.011 (0.010) |
| first letter involved | 312 | 0.488 | 0.503 | 0.795 | -0.308 (0.023) | -0.292 (0.022) |
| first letter untouched | 2688 | 0.882 | 0.882 | 0.880 | +0.002 (0.003) | +0.002 (0.003) |

The last two rows overlap the categories above (each pair is in one category and in one of them). About ten comparisons per system are shown; the cuts (categories, first letter, at most 2 participants) were fixed before the run, the first-letter cut because of #21 and #40, but with that many tests some |z| above 2 are expected by chance: read the small ones (`sub_other`, `two_edits`, `missing_doubled`) as no evidence either way.

Category by first letter (engine default minus baseline; groups with fewer than 2 pairs omitted; the pairs are the same 3000):

| group | n | MRR default | MRR baseline | default - baseline (SE) |
|---|---|---|---|---|
| sub_adjacent, first letter untouched | 418 | 0.893 | 0.907 | -0.015 (0.006) |
| sub_adjacent, first letter involved | 66 | 0.919 | 0.838 | +0.081 (0.024) |
| sub_other, first letter untouched | 258 | 0.907 | 0.884 | +0.023 (0.007) |
| sub_other, first letter involved | 30 | 0.409 | 0.839 | -0.430 (0.072) |
| transposition, first letter untouched | 188 | 0.944 | 0.955 | -0.010 (0.008) |
| transposition, first letter involved | 26 | 0.395 | 0.955 | -0.560 (0.073) |
| extra_letter, first letter untouched | 754 | 0.970 | 0.966 | +0.004 (0.003) |
| extra_letter, first letter involved | 88 | 0.561 | 0.970 | -0.409 (0.039) |
| extra_doubled, first letter untouched | 207 | 0.938 | 0.982 | -0.044 (0.011) |
| missing_letter, first letter untouched | 363 | 0.872 | 0.861 | +0.010 (0.007) |
| missing_letter, first letter involved | 36 | 0.302 | 0.822 | -0.519 (0.062) |
| missing_doubled, first letter untouched | 18 | 0.894 | 0.873 | +0.021 (0.015) |
| two_edits, first letter untouched | 482 | 0.679 | 0.662 | +0.017 (0.010) |
| two_edits, first letter involved | 66 | 0.133 | 0.424 | -0.290 (0.050) |

Note the two exceptions to the pattern: first-letter adjacent-key substitutions favour the engine (+0.081, n = 66; the 8-cost neighbour edit with the x1.5 factor is 12, one unit), and `extra_doubled` (-0.044, n = 207, first letter untouched) and untouched adjacent-key substitutions (-0.015, SE 0.006) are slightly below the baseline.

## Reading

Measured:

- Overall the keyboard-aware engine is worse than the unit-cost baseline on this corpus by about 0.03 MRR@10 (about 3.5% relative; about 8 standard errors), with budget 32 and with budget 48. R@10 is 0.955 and 0.959 against 0.974.
- On adjacent-key substitutions the engine ties the baseline (-0.002, SE 0.006, n = 484). The costs do not show a ranking benefit even on the category they target.
- The differences sit in the first-letter pairs (-0.308, SE 0.023). Excluding them, the overall difference is +0.002 (SE 0.003). Transpositions lose -0.077 overall, but almost all of it is first-letter transpositions (n = 26, -0.560, SE 0.073); mid-word transpositions (n = 188) are -0.010 (SE 0.008). The same pattern holds for extra_letter (+0.004 against -0.409), missing_letter (+0.010 against -0.519) and sub_other (+0.023 against -0.430).
- Budget 48 over budget 32: +0.0016 (SE 0.0006), almost all of it in `two_edits` (+0.009 MRR on 548 pairs); the corpus has few pairs the higher budget can reach. Its cost in speed is in `recall-preset.md`.

Hypotheses, not tested here:

- The first-letter loss is the x1.5 factor at position 0, as #21 and #40 found on the GitHub corpus; the baseline has no such factor. Removing it would need a core change and a re-run.
- The transposition loss is almost entirely first-letter transpositions (n = 26, -0.56), which the x1.5 factor prices at 18, two whole units. Mid-word transpositions (12, one unit under `Coarse`) are -0.010. Cost 12 is not implicated.
- 64% of substitution pairs (9 125 of 14 159) are adjacent keys, well above chance (about 16%), yet ranking did not benefit.

Suggested follow-up (not adopted here): re-run with the first-byte factor at 1.0 (and with it applied after rounding), then check whether `extra_doubled` (-0.044, n = 207, first letter untouched) comes from two cheap edits (8 + 8) being merged into one whole unit.

## Limits

- The baseline is not a clean ablation. It differs from the engine in its edit costs, its candidate set (distance at most 2 versus a cost budget), its ordering (unit distance then weight versus the merged cost with `Coarse` ranking) and the absence of the first-letter factor. The `CostModel` fields are private and the task allowed no core change, so an engine with unit costs could not be run. The result says how the shipped engine compares with a plain edit-distance ranking on real slips, not how much each part of the cost table contributes.
- One corpus, one sample of 3000 pairs (of 57 542), one dictionary; weights are corpus frequencies from `big.txt`, most dictionary words have weight 0 and ties are broken by id. The right word is often a very common word, which favours a ranking that uses weight strongly.
- Unique pairs are counted once. Weighting by occurrences would give the systematic misspellings more influence; this was not run.
- Costs are provisional (see `cost.rs`); this report does not calibrate them.

## How to reproduce

From the repository root (the archive is 1.5 GB and the script needs about 35 s to read it after the download; nothing is committed):

```sh
cd bench
npm ci
node fetch-data.mjs && node prepare-m0-data.mjs    # words-full.tsv
node fetch-finger-slips.mjs                        # slips.tsv, slips-sample.tsv, category counts
cd ..
cargo run --release -p keyhammer-bench --bin slips -- bench/data
```

The table above is the harness's output. `slips -- bench/data slips.tsv` runs the whole corpus, with one full dictionary scan per pair for the baseline (57 542 scans; not run).
