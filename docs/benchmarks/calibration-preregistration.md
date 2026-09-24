# Pre-registration: calibration of the cost model (issue #21)

Written and committed BEFORE any cost model other than the shipped one was evaluated on any query, and before the calibration harness (`bench/src/bin/calib.rs`) and the experiment patch (`bench/experiments/cost-knobs.patch`) were committed. The result report (`calibration.md`) is written afterwards and must be read against this file. Anything the result report does that is not described here is labelled as exploratory there.

## What is tested

The cost values are provisional (`cost.rs`: ordinary edit 16, neighbouring-key substitution 8, doubled-letter insertion or deletion 8, transposition 12, all x1.5 on the first query byte, ranking by the cost rounded up to whole units of 16). Issue #21 asks whether they can be fitted on labelled corpora, and says the default changes only if the improvement is larger than about 1.5 standard errors on data the fit did not see. This study fits a small grid on training data, chooses on validation data, and tests once on data nobody looked at. The rule below is stricter than the issue (1.96 SE, several guards). Nothing in `crates/keyhammer` changes in the pull request that carries this study; the default stays as it is whatever the outcome. Adoption is a separate change, proposed as a follow-up issue text in the result report only if the rule says so.

Prior knowledge that shaped the design, disclosed: in the finger-slip report (#60) the shipped engine lost to a unit-cost baseline almost entirely on first-letter pairs (n = 312, -0.308 MRR@10, SE 0.023; elsewhere +0.002), and untouched-first-letter adjacent-key substitutions were -0.015 (SE 0.006). Both were measured on `slips-sample.tsv`, which is therefore NOT used in any role below (it is reported afterwards as a seen reference only). Issue #40 saw the same first-letter effect on the GitHub Typo Corpus; `gtc.tsv` and `wiki.tsv` were used there (ranking variants, not cost calibration).

## The parameters (the grid)

Every model is the shipped model with some of these changed. The ordinary substitution cost stays 16 (the unit); the search budget stays 32 and `max_nodes` 100 000 (defaults) for every model, so a lower cost means a wider search at the same budget; that interaction is part of what is tested and the result report says so. All costs are multiples of 4 so that the first-byte factor is exact in integer arithmetic.

| parameter | values | shipped |
|---|---|---|
| first-byte factor `f` | 1.0, 1.25, 1.5 | 1.5 |
| where `f` acts | before rounding (as shipped), after rounding | before |
| transposition | 8, 12, 16 | 12 |
| neighbouring-key substitution | 4, 8, 12, 16 | 8 |
| (ordinary indel, doubled-letter indel) | (16, 8), (16, 12), (16, 16), (12, 8), (12, 12) | (16, 8) |

`f = 1.0` makes "before" and "after" identical, so that combination appears once. The first-byte factor multiplies every edit at query position 0 (as `cost.rs` does). Total: 5 factor/rounding combinations x 3 x 4 x 5 = 300 models; the shipped model is one of them.

"Before rounding" is the shipped behaviour: the exact cost includes the factor and `Ranking::Coarse` orders by `whole_units(cost)`, so a neighbouring-key edit on the first byte (8 x 1.5 = 12) is one unit and an ordinary one (16 x 1.5 = 24) is two.

"After rounding" is defined here, without changing core: with `c` the exact cost under the model (factor included) and `c1` the exact cost of the same pair under the same costs with factor 1.0, the ranking key is `whole_units(c1) * 16 + (c - c1)`, then higher weight, then lower term id. That is, the base cost is rounded to whole units and the surcharge caused by the first-byte factor is added afterwards without rounding (the surcharge is `c - c1 >= 0`). Candidates and the budget are those of the search with the factor (the cost `c`), as shipped. The key is computed in the harness from the `Exact` list of the search, which is extended (k = 64, doubling up to 4096) until the last hit's `c` exceeds the tenth key, so every term that can reach the top 10 is present (incomplete lists are counted and reported). `c1` comes from an independent dynamic programme in the harness that is checked against `Hit::cost` (self-check below).

Not in the grid (hypotheses only): the ordinary substitution and indel costs relative to the budget, the budget itself (see `recall-preset.md`), the keyboard layout, per-letter costs, and any change to ranking beyond the definition above.

## Implementation (fixed in advance)

`CostModel` fields are private and the public core API must not change, so the experiment uses `bench/experiments/cost-knobs.patch`: a patch to `crates/keyhammer/src/cost.rs` that adds `CostParams`, `CostModel::qwerty_with` and the first-byte factor as a field, with `CostParams::SHIPPED` giving the shipped values (identical results to the shipped model, checked by reproducing the shipped-model figures of `finger-slips.md`). The harness `bench/src/bin/calib.rs` is compiled only with `--features calibration` after `git apply`; the pull request does not apply the patch to the tree. A self-check (`--selfcheck`, 300 training queries, pass or fail only) verifies that (a) `Hit::cost` equals the independent DP for the models with `f` = 1.0, 1.25 and 1.5, (b) `Coarse` equals "whole units of the exact cost, then weight, then id" over the `Exact` list at `f` = 1.0, and (c) the "after" lists are complete. Before this pre-registration was committed, the harness had been compiled, its split sizes printed (`--counts`, below) and the self-check run once (all three passed after fixing the size of `k` in check b, which was too small). No model was scored on any query for quality.

Threads only parallelise over models; every quantity is deterministic.

## Data and splits (all fixed now)

Dictionary: `words-full.tsv` (274 137 words, weights as in `prepare-m0-data.mjs`, SHA-256 `fda37a01625cad137afff97428661e57bc9837f8b477b2fececa74c9de6181b5`). Quality only, at this dictionary size; smaller dictionaries are not used.

Corpora (four, each a different kind of error):

- Birkbeck (`missp.dat`, SHA-256 `ed7d8c91961a1201632351943571e77af2011cdf45d24304c4d7cad6cf77ea15`): all unique (typo, word) pairs after the filter of `prepare-m0-data.mjs` and `compare.mjs` (typo at least 3 letters, a-z only; word at least 3 letters, in the dictionary; typo not in the dictionary; typo different from the word). Mostly spelling errors, and many pairs are farther than two edits.
- GitHub Typo Corpus: `gtc.tsv` (sha256 `091dfd81ef0f13b901bfec40dc1617b74867832b3b3efec35a3cb831378c04e2`) and `gtc-holdout.tsv` (`1508e677ef3d626a0bcd5317d8a7628a0950fe7f9a3b23f6bb213606078182c0`), from `fetch-typo-corpora.mjs --holdout`.
- Wikipedia list of common misspellings: `wiki.tsv` (`d3dc05e3a52b1a2c5c06bac4bbfcaafdbfffbe3a501bcd95d457819008b9c3f9`) and `wiki-holdout.tsv` (`199b7eaabbf21570cd031c1294849fb31e6d00472efe968a2c2fb40549620701`).
- Finger slips: `slips.tsv` (57 542 unique pairs, sha256 `7a88ed8a89b7b93a5c765a229df005b7a552d1f8616fc2c6031ad987814cefb7`) from `fetch-finger-slips.mjs`, minus every pair of `slips-sample.tsv` (`b94ca31eadd9cda44d36a062f32b387e6c349474c3018c39c0332d61ee9b2ba1`, the sample of #60, excluded because it was already seen). Licence caveats of that corpus are in `finger-slips.md` and apply here.

Splitting is by intended word, not by pair, so that no word (and none of its misspellings) is in two roles inside a corpus. The bucket of a word is `splitmix64(FNV-1a-64("keyhammer-calibration-v1:" + word)) mod 100` (harness functions `hash` and `bucket`). Within a role the pairs are ordered by the same hash of `typo TAB word` and the first `cap` are kept.

| corpus | train | validation | test |
|---|---|---|---|
| Birkbeck | buckets 0-49, cap 2000 | 50-69, cap 1500 | 70-99, cap 2000 |
| finger slips | 0-49, cap 3000 | 50-69, cap 2000 | 70-99, cap 3000 |
| GitHub Typo Corpus | `gtc.tsv` buckets 0-59 | `gtc.tsv` buckets 60-99 | all of `gtc-holdout.tsv` |
| Wikipedia | `wiki.tsv` buckets 0-59 | `wiki.tsv` buckets 60-99 | all of `wiki-holdout.tsv` |

Resulting sizes (printed by `calib --counts`, which runs no model): train 2000 + 3000 + 597 + 617 = 6214 pairs; validation 1500 + 2000 + 403 + 383 = 4286; test 2000 + 3000 + 2000 + 2754 = 9754. The two typo-corpus holdouts are disjoint from `gtc.tsv` and `wiki.tsv` by pair and by typo string but not by intended word, so a word of the test may also occur in training; the same word-level leakage does not exist inside Birkbeck and finger slips (word buckets). With six parameters and 6214 training pairs, word-level leakage from the typo-corpus holdouts is unlikely to matter; it is a listed limit. The holdout files were used before only by the tie-break study (default cost model, ranking order only, no cost was varied).

Every query is scored on all corpora alike: the query is the typo, the target is the intended word, `k = 10`, reciprocal rank 0 when the word is not in the top 10.

## Metric and statistics

Per query, reciprocal rank at 10 (MRR@10). For a candidate model and the shipped model, per corpus: mean paired difference candidate minus shipped and its standard error (sample standard deviation of the per-query differences over the square root of n). Across the four corpora: the MACRO average of the four differences (each corpus counts equally, so the size of a corpus does not decide) with SE = sqrt(sum of the four squared SEs) / 4; the 95% interval is mean +/- 1.96 SE. Pairs inside a corpus are treated as independent, which makes the SEs somewhat optimistic (several typos of one word; the same participant).

## Procedure and decision rule

1. Train. Score all 300 models on the training split. Rank them by macro difference to the shipped model (ties: fewer parameters changed from shipped, then grid order). The training numbers are for the shortlist only and carry no claim.
2. Shortlist: the top 5 of the training ranking, plus two named reference models if not already there: F1 (shipped costs with `f` = 1.0, the lead from #60) and FA (shipped costs, `f` = 1.5 applied after rounding).
3. Validation. Score the shortlist and the shipped model on the validation split. A shortlisted model QUALIFIES if (i) the lower end of its macro 95% interval is above 0 and (ii) no corpus has a significantly negative difference (difference + 1.96 SE < 0). The selected model is the qualifying one with the largest macro difference (ties: fewer parameters changed). If none qualifies, the shipped model stays, no candidate is selected, and the outcome is "no change".
4. Test, once. Only after the selection is printed, the test split is scored: the shipped model and the selected model. If none was selected, the shortlisted model with the largest validation macro difference is scored for information only and marked exploratory; it cannot change the verdict. The test split is never scored for any other model, and never before the selection.
5. Verdict, on the test split only, for the selected model. PROPOSE it as a follow-up if ALL hold: (i) the lower end of the macro 95% interval is above 0; (ii) no test corpus has a significantly negative difference; (iii) no error category with n >= 50 (pooled test pairs, categories below) has a significantly negative difference. INCONCLUSIVE if the macro interval contains 0, its upper end is at least +0.005 and (ii) and (iii) hold. Otherwise DO NOT PROPOSE (the trade-off, if any, is reported). The +0.005 is arbitrary and copied from the tie-break study. Adopting anything in core is a separate change after a new issue, whatever the verdict.

Guards against overfitting: the choice is made on data separate from the fit (validation) and the claim on data separate from both (test); the shortlist is small (up to 7); the validation criterion is stricter than the issue's 1.5 SE; splits are by word; no rule uses the seen sample or the unit-cost baseline; the test is scored once. The validation winner is the maximum of up to 7 noisy estimates, so its validation difference is biased upward; that is why the claim rests on the test split. No multiplicity correction is applied to the test (a single model is tested; the category check is a loss check, so the lack of correction is conservative for a proposal).

## Error categories (test, pooled)

From (typo, word), by unit optimal-string-alignment distance d, exactly as in `tiebreak-preregistration.md`: `transposition`, `missing letter`, `missing doubled letter`, `extra letter`, `extra doubled letter`, `neighbouring-key substitution` (`sub_cost(typo letter, word letter, 1) < 16` of the shipped model), `other substitution`, `two or more edits` (d >= 2), plus the overlay `first letter differs`. Categories with n < 50 are reported and do not enter (iii).

## Also reported (not part of the rule)

- Where the chosen model loses: per corpus, per category, and the first-letter overlay, on the test split, as a table with differences and SEs.
- Nodes expanded (deterministic, `tsb` on, budget 32, `Coarse`, `k` = 10) for the first-byte factor 1.0, 1.25 and 1.5 with the other costs shipped, per corpus and split, and for the selected model on the test split (a model using "after" ranking searches with `Exact` and a larger k, so its node count is not comparable and is labelled). No latency is measured.
- The unit-cost baseline of `finger-slips.md` (OSA distance at most 2, then weight, then id) on the test split, as a reference for the shipped and selected model.
- The shipped model on `slips-sample.tsv` (seen) as a reproduction check against `finger-slips.md` (0.841), and the selected model there.
- Counts of searches truncated by `max_nodes` and of incomplete "after" lists.

## Not covered

One dictionary size, English, QWERTY; four corpora that mix slips and spelling errors (the finger-slip corpus contains classic spelling errors); the search budget fixed at 32; unit-weighted pairs (occurrence counts are not used); correlated pairs treated as independent; the grid is coarse (multiples of 4) and small, so a better model outside it is not excluded; the "after rounding" key is one definition among several. Quality only, one machine for node counts (deterministic anyway).
