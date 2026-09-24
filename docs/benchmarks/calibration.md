# Calibration of the cost model, 2026-09-24

Result of the study pre-registered in `calibration-preregistration.md` (issue #21). The protocol was committed first, in its own commit, before any candidate model was scored; the harness (`bench/src/bin/calib.rs`) and the experiment-only patch (`bench/experiments/cost-knobs.patch`) came in a second commit, the run after that. Everything below is the harness output of one run, read against the rule of that file. Nothing in `crates/keyhammer` changed; the default costs are untouched by this pull request.

## Summary

- Verdict of the pre-registered rule: PROPOSE a follow-up (read together with the exploratory node-matched controls below: on Birkbeck the reach effect dominates). One model was selected on validation and confirmed on the untouched test split: first-byte factor 1.25 (before rounding, i.e. applied to the cost as shipped), neighbouring-key substitution 12 (shipped 8), ordinary indel 12 (shipped 16), doubled-letter indel 8 and transposition 12 (both as shipped); shortened `f1.25B t12 a12 i12/8`.
- Test split (9754 pairs, four corpora): macro MRR@10 difference to the shipped model +0.0134 (SE 0.0015), 95% interval [+0.0105, +0.0162]. Every one of the four corpora is above zero (Birkbeck +0.0050, SE 0.0031, not significant on its own; finger slips +0.0279; GitHub Typo Corpus +0.0117; Wikipedia +0.0089). No category with n >= 50 loses significantly.
- The gain has a price in work: at the same budget of 32 the selected model expands about 1.67 times the trie nodes of the shipped one on the test split (1634.6 against 979.4 per query, deterministic counts). Latency was not measured.
- The first-byte factor by itself is not the main lever. The reference model F1 (shipped costs, factor 1.0) scored +0.0012 (SE 0.0028) on validation and lost significantly on one corpus (Birkbeck -0.0061, SE 0.0023), so it did not qualify; the reference FA (factor 1.5 applied after rounding) scored -0.0014 (SE 0.0014). The gain is concentrated on first-letter pairs (81.7% of the pooled gain; +0.164 on those against +0.0029 macro on the rest), as #60 found, but removing the factor alone (F1) did not help; the train marginals (exploratory) favour a shallower neighbour discount, and the two appear to interact. This refines the hypothesis in `finger-slips.md` that the first-letter loss is the x1.5 factor (see the dated note there).
- The selected model is still below the unit-cost baseline on finger slips (-0.0029, SE 0.0018, not significant) and above it overall (macro +0.0037, SE 0.0012, reference only, not part of the rule).
- Not adopted here. The proposed follow-up issue text is at the end.

## What was run

One run of `calib` (300 models on the training split, 7 on validation, 2 on the test split), 2026-09-24, Windows, 6 threads, on a shared machine whose load was not checked; the run took 7 min 21 s of wall time, which is not a result. All reported quantities are deterministic. Splits (by intended word within a corpus; whole holdout files for the typo corpora) and sizes, as pre-registered:

| split | Birkbeck | finger slips | GitHub Typo Corpus | Wikipedia | total |
|---|---|---|---|---|---|
| train | 2000 | 3000 | 597 | 617 | 6214 |
| validation | 1500 | 2000 | 403 | 383 | 4286 |
| test | 2000 | 3000 | 2000 | 2754 | 9754 |

Dictionary 274 137 words, budget 32, `k` = 10, `max_nodes` 100 000 (no search was truncated; no "after rounding" list was incomplete). Differences are candidate minus shipped model, MRR@10, per corpus with the standard error in brackets; the macro column is the equal-weight mean of the four with SE = sqrt(sum of squared SEs)/4.

## Phase 1: training (300 models)

The shipped model scored macro MRR@10 0.7378 on the training split. 162 of the 300 models were above it, 137 below and 1 equal (the model itself). The full list is `calibration-train-grid.tsv` (macro difference and SE per model). Top 10 (the first five are the shortlist):

| model | Birkbeck | finger slips | GitHub Typo Corpus | Wikipedia | macro | 95% interval |
|---|---|---|---|---|---|---|
| f1.25B t12 a12 i12/8 | +0.0083 (0.0027) | +0.0336 (0.0033) | +0.0190 (0.0056) | +0.0072 (0.0036) | +0.0170 (0.0020) | [+0.0131, +0.0209] |
| f1.25B t8 a12 i12/8 | +0.0120 (0.0031) | +0.0325 (0.0034) | +0.0161 (0.0058) | +0.0062 (0.0037) | +0.0167 (0.0021) | [+0.0126, +0.0208] |
| f1.0B t8 a16 i16/8 | +0.0076 (0.0030) | +0.0339 (0.0037) | +0.0184 (0.0065) | +0.0042 (0.0039) | +0.0160 (0.0022) | [+0.0116, +0.0204] |
| f1.0B t12 a16 i12/8 | +0.0078 (0.0031) | +0.0319 (0.0037) | +0.0202 (0.0063) | +0.0042 (0.0039) | +0.0160 (0.0022) | [+0.0116, +0.0204] |
| f1.0B t16 a16 i16/8 | +0.0038 (0.0028) | +0.0349 (0.0037) | +0.0209 (0.0063) | +0.0042 (0.0039) | +0.0159 (0.0022) | [+0.0117, +0.0202] |
| f1.0B t12 a16 i16/8 | +0.0038 (0.0028) | +0.0349 (0.0037) | +0.0209 (0.0063) | +0.0042 (0.0039) | +0.0159 (0.0022) | [+0.0117, +0.0202] |
| f1.0B t8 a12 i16/8 | +0.0074 (0.0030) | +0.0335 (0.0037) | +0.0182 (0.0065) | +0.0042 (0.0039) | +0.0158 (0.0022) | [+0.0114, +0.0202] |
| f1.0B t16 a12 i16/8 | +0.0037 (0.0028) | +0.0346 (0.0037) | +0.0207 (0.0063) | +0.0042 (0.0039) | +0.0158 (0.0022) | [+0.0115, +0.0201] |
| f1.0B t12 a12 i16/8 | +0.0037 (0.0028) | +0.0344 (0.0036) | +0.0207 (0.0063) | +0.0042 (0.0039) | +0.0158 (0.0022) | [+0.0115, +0.0200] |
| f1.0B t16 a16 i12/8 | +0.0060 (0.0030) | +0.0322 (0.0037) | +0.0202 (0.0063) | +0.0042 (0.0039) | +0.0156 (0.0022) | [+0.0113, +0.0200] |

Model names: `f` first-byte factor (`B` applied before rounding as shipped, `A` after), `t` transposition, `a` neighbouring-key substitution, `i` ordinary/doubled indel. The rows `t12 a16 i16/8` and `t16 a16 i16/8` are not identical: they differ only in the fifth decimal (+0.01593 for t12 against +0.01594 for t16, `calibration-train-grid.tsv`), and that 1e-5 gap decided shortlist rank 5 (t16) over rank 6 (t12). Under `Coarse` a transposition of 12 or 16 is one unit either way, so mid-word the two costs cannot differ except through sums and the first-byte factor.

Training macro differences are the maximum of a search over 300 models, so they are biased upward and carry no claim.

## Phase 2: validation

Shipped model on validation: macro MRR@10 0.7446.

| model | Birkbeck | finger slips | GitHub Typo Corpus | Wikipedia | macro | 95% interval |
|---|---|---|---|---|---|---|
| f1.25B t12 a12 i12/8 (train rank 1) | +0.0074 (0.0031) | +0.0293 (0.0042) | +0.0336 (0.0078) | +0.0178 (0.0061) | +0.0220 (0.0028) | [+0.0165, +0.0275] |
| f1.25B t8 a12 i12/8 (train rank 2) | +0.0103 (0.0035) | +0.0284 (0.0043) | +0.0332 (0.0080) | +0.0157 (0.0059) | +0.0219 (0.0028) | [+0.0163, +0.0275] |
| f1.0B t8 a16 i16/8 (train rank 3) | +0.0052 (0.0034) | +0.0322 (0.0046) | +0.0303 (0.0087) | +0.0128 (0.0063) | +0.0201 (0.0030) | [+0.0142, +0.0261] |
| f1.0B t12 a16 i12/8 (train rank 4) | +0.0061 (0.0036) | +0.0297 (0.0046) | +0.0285 (0.0086) | +0.0147 (0.0064) | +0.0197 (0.0030) | [+0.0138, +0.0257] |
| f1.0B t16 a16 i16/8 (train rank 5) | +0.0037 (0.0031) | +0.0331 (0.0045) | +0.0295 (0.0086) | +0.0120 (0.0058) | +0.0196 (0.0029) | [+0.0138, +0.0254] |
| f1.0B t12 a8 i16/8 (reference F1: factor 1.0) | -0.0061 (0.0023) | +0.0120 (0.0040) | -0.0007 (0.0086) | -0.0002 (0.0052) | +0.0012 (0.0028) | [-0.0042, +0.0066] |
| f1.5A t12 a8 i16/8 (reference FA: 1.5 after rounding) | -0.0005 (0.0011) | -0.0048 (0.0025) | -0.0002 (0.0036) | -0.0003 (0.0030) | -0.0014 (0.0014) | [-0.0041, +0.0012] |

Qualification (lower end above 0 and no corpus with a significant loss): the five shortlisted models qualify (lower ends +0.0165, +0.0163, +0.0142, +0.0138, +0.0138; no corpus veto); F1 does not (lower end -0.0042; vetoed by Birkbeck); FA does not (lower end -0.0041). Selected, the qualifying model with the largest validation macro difference: `f1.25B t12 a12 i12/8`. The validation difference of the selected model (+0.0220) is larger than its test difference (+0.0134), which is the expected optimism of choosing the maximum of seven estimates on validation.

## Phase 3: test (scored once, after the selection)

Macro MRR@10 on the test split: shipped 0.7420, selected 0.7554.

| model | Birkbeck | finger slips | GitHub Typo Corpus | Wikipedia | macro | 95% interval |
|---|---|---|---|---|---|---|
| f1.25B t12 a12 i12/8 - shipped | +0.0050 (0.0031) | +0.0279 (0.0035) | +0.0117 (0.0028) | +0.0089 (0.0022) | +0.0134 (0.0015) | [+0.0105, +0.0162] |

Per corpus MRR@10, shipped / selected: Birkbeck 0.4018 / 0.4069; finger slips 0.8416 / 0.8695; GitHub Typo Corpus 0.8151 / 0.8268; Wikipedia 0.9095 / 0.9184. Birkbeck is low because many of its misspellings are more than two edits away from the word.

Where the selected model does and does not win (pooled test pairs, MRR@10; the categories are pre-registered, and the last row overlays the others):

| category | n | shipped | selected | difference (SE) | significantly negative |
|---|---|---|---|---|---|
| extra doubled letter | 553 | 0.9483 | 0.9800 | +0.0317 (0.0059) | no |
| extra letter | 1667 | 0.9395 | 0.9675 | +0.0279 (0.0036) | no |
| missing doubled letter | 366 | 0.8983 | 0.9028 | +0.0045 (0.0078) | no |
| missing letter | 1763 | 0.8903 | 0.9065 | +0.0162 (0.0030) | no |
| neighbouring-key substitution | 689 | 0.8931 | 0.9146 | +0.0215 (0.0047) | no |
| other substitution | 1116 | 0.8944 | 0.8949 | +0.0005 (0.0024) | no |
| transposition | 928 | 0.9012 | 0.9395 | +0.0383 (0.0055) | no |
| two or more edits | 2672 | 0.3835 | 0.3821 | -0.0014 (0.0035) | no |
| first letter differs (overlay) | 665 | 0.3503 | 0.5241 | +0.1738 (0.0134) | no |

The only negative point estimate is `two or more edits` (-0.0014, SE 0.0035, indistinguishable from zero); no model in the grid changes the budget, so pairs that need more than about two edits are not reached differently. Missing doubled letters and other substitutions are unchanged within error. The selected model does not lose significantly anywhere at this sample size; that says nothing about categories or corpora not present here (one dictionary, English, QWERTY).

## Verdict against the pre-registered rule

Test macro lower bound +0.0105 (above 0: yes); test corpus with a significant loss: none; category (n >= 50) with a significant loss: none. Rule outcome: PROPOSE the candidate as a follow-up. The rule does not say adopt: adoption is a separate change with its own decision, and it must weigh the cost in nodes below.

## Nodes expanded by the first-byte factor (deterministic)

Other costs shipped, `Coarse`, `k` = 10, budget 32, `tsb` on, no search truncated. Test split, by corpus (nodes expanded; per query in brackets), and in total (9754 queries):

| first-byte factor | Birkbeck | finger slips | GitHub Typo Corpus | Wikipedia | total | per query |
|---|---|---|---|---|---|---|
| x1.0 | 3 886 408 (1943.2) | 4 720 611 (1573.5) | 3 742 324 (1871.2) | 5 748 393 (2087.3) | 18 097 736 | 1855.4 |
| x1.25 | 3 330 433 (1665.2) | 4 162 822 (1387.6) | 3 208 854 (1604.4) | 4 938 907 (1793.4) | 15 641 016 | 1603.5 |
| x1.5 (shipped) | 2 052 246 (1026.1) | 2 561 993 (854.0) | 1 951 666 (975.8) | 2 987 521 (1084.8) | 9 553 426 | 979.4 |

So lowering the first-byte factor from 1.5 to 1.25 costs 1.64 times the nodes, and to 1.0 costs 1.89 times: at position 0 an ordinary edit costs 24, 20 or 16, and a lower cost puts more first-byte edits inside the budget of 32. The same counts for the training and validation splits are in the harness output (same ratios, 1.6 to 1.9 times). The selected model (factor 1.25, indel 12) expands 15 943 852 nodes on the test split, 1634.6 per query, 1.67 times the shipped 9 553 426 (979.4); it uses the shipped `Coarse` search, so the counts are comparable. For scale, `recall-preset.md` measured 4.6 to 5.2 times the nodes for budget 48. Latency was not measured here; node counts are a proxy, not a latency claim.

## References (not part of the rule)

- Unit-cost baseline of `finger-slips.md` on the test split, difference to the baseline (MRR@10, SE): shipped model Birkbeck -0.0037 (0.0035), finger slips -0.0308 (0.0038), GitHub Typo Corpus -0.0089 (0.0036), Wikipedia +0.0046 (0.0030), macro -0.0097 (0.0017); selected model +0.0013 (0.0029), -0.0029 (0.0018), +0.0028 (0.0025), +0.0135 (0.0022), macro +0.0037 (0.0012). Baseline macro MRR@10 0.7517. The shipped model loses to plain edit distance on finger slips again on new pairs (-0.0308, the sample of #60 gave -0.0305); the selected model closes about nine tenths of that gap.
- The finger-slip sample of #60 (seen before, in no role): shipped 0.8407 (the report has 0.841; it reproduces the shipped model through the patch), selected 0.8735, difference +0.0328 (SE 0.0034).

## Exploratory (train split only; not part of the rule)

Marginal means of the training macro difference over the whole grid (computed from `calibration-train-grid.tsv`, a descriptive summary of 300 correlated models, not a test): by neighbouring-key substitution cost 4: -0.0437, 8 (shipped): -0.0024, 12: +0.0107, 16: +0.0102; by first-byte setting 1.0 before: -0.0073, 1.25 before: -0.0050, 1.25 after: -0.0055, 1.5 before: -0.0079, 1.5 after: -0.0059; by transposition 8, 12, 16: -0.0065, -0.0061, -0.0063; by indel pair (16, 8): -0.0018, (16, 12): -0.0033, (16, 16): -0.0030, (12, 8): -0.0107, (12, 12): -0.0127. Taken together with the near-equal `t12` and `t16` rows above, this reads as: the neighbouring-key discount of 8 is too deep; 12 or 16 are equally good on average; the transposition cost hardly matters; a cheaper ordinary indel (12) is worse on average even though the selected model contains it. Hypotheses, not tested: (1) under `Coarse` two neighbouring-key edits at 8 + 8 = 16 count as one unit, so a word with two slips ties with a word with one; a cost of 12 or 16 keeps them apart; (2) the ordinary-indel value 12 in the selected model may be there because it was the best of a noisy maximum, not because it helps (on average it does not); a model that keeps the indel at 16, for instance `f1.25B t12 a12 i16/8`, was not scored on validation or test and is not covered by this verdict; (3) part of the gain of cheaper costs may be a larger effective search (more candidates within budget 32), the work shows in the node counts. The test split has been scored once and is spent; testing (2) or any variant needs a new pre-registration and new data (or a new word-bucket split with the test buckets not reused).

## Exploratory controls after review (test split, already spent; cannot change the verdict)

Added after an independent review, with `calib --exploratory` (committed; same test split, so these numbers are for review only and are not part of the rule). Difference to the shipped model at budget 32 (979.4 nodes per query), MRR@10 with SE, node counts per query:

| arm | Birkbeck | finger slips | GitHub Typo Corpus | Wikipedia | macro | nodes per query |
|---|---|---|---|---|---|---|
| shipped, budget 36 | +0.0015 (0.0008) | +0.0010 (0.0005) | +0.0000 (0.0000) | +0.0001 (0.0001) | +0.0006 (0.0002) | 1794.7 |
| shipped, budget 40 | +0.0228 (0.0029) | +0.0015 (0.0006) | +0.0009 (0.0006) | +0.0013 (0.0006) | +0.0066 (0.0008) | 2342.7 |
| shipped, budget 48 | +0.0513 (0.0043) | +0.0014 (0.0006) | +0.0040 (0.0013) | +0.0029 (0.0008) | +0.0149 (0.0012) | 4106.4 |
| selected, budget 32 | +0.0050 (0.0031) | +0.0279 (0.0035) | +0.0117 (0.0028) | +0.0089 (0.0022) | +0.0134 (0.0015) | 1634.6 |
| selected, budget 28 | -0.0200 (0.0043) | +0.0306 (0.0036) | +0.0095 (0.0032) | +0.0037 (0.0026) | +0.0060 (0.0017) | 1200.9 |
| f1.25B t12 a12 i16/8, budget 32 | +0.0050 (0.0024) | +0.0184 (0.0026) | +0.0141 (0.0021) | +0.0142 (0.0018) | +0.0129 (0.0011) | 1469.1 |
| f1.25B t12 a12 i16/8, budget 28 | -0.0885 (0.0067) | -0.0159 (0.0043) | -0.0219 (0.0046) | -0.0248 (0.0040) | -0.0378 (0.0025) | 744.4 |
| F1 (f1.0B t12 a8 i16/8), budget 32 | -0.0073 (0.0025) | +0.0117 (0.0034) | -0.0081 (0.0029) | -0.0087 (0.0019) | -0.0031 (0.0014) | 1855.4 |

Reading (exploratory): on finger slips, GTC and Wikipedia the gain is ranking, not reach: giving the shipped costs 1.8 to 2.4 times the nodes (budgets 36 and 40; 4.2 times at 48) buys at most +0.0040 there, while the selected model gains +0.0279, +0.0117 and +0.0089 at 1.67 times, and still gains +0.0306, +0.0095 and +0.0037 at 1.23 times (budget 28). On Birkbeck the reach effect dominates: the shipped model at budget 48 gains +0.0513 there, and the selected model at budget 28, at 1.23 times the shipped nodes, loses -0.0200 (significant). The arm `f1.25 a12 i16/8` (factor 1.25, neighbour 12, ordinary indel back to 16) keeps +0.0129 (SE 0.0011) for 1469 nodes per query, 1.50 times the shipped nodes, and is the natural first arm of the follow-up study; it was not scored on validation. F1 on the test split is -0.0031 (SE 0.0014): removing the factor alone does not help here either.

First-letter concentration (same run): 665 of the 9754 test pairs have a first letter that differs; they carry 81.7% of the pooled gain (115.58 of 141.53 summed per-pair differences). Per pair, +0.1738 on those against +0.0029 on the rest; macro over corpora +0.1636 (SE 0.0181) against +0.0029 (SE 0.0012).

## Limits

- Grid and budget: 300 models of multiples of 4 around the shipped values, the budget fixed at 32. A better model outside the grid is not excluded. Lower costs at a fixed budget can change how far the search reaches, so the effect of a cost and the effect of a wider search are entangled. The exploratory node-matched controls below separate them only partly: the neighbour cost went up (8 to 12) and the band `budget / c_indel_min` is unchanged (`c_indel_min` is 8 in both models), so the wider reach of the selected model comes from factor 1.25 and indel 12.
- QWERTY only: the fit uses `CostModel::qwerty` costs and QWERTY adjacency on English corpora. Other layouts now ship (`Layout`, #62) and were not fitted or evaluated; nothing here says the values transfer to them.
- Corpora: four, all English, mixing slips and spelling errors; the finger-slip corpus has licence caveats (see `finger-slips.md`).
- Word overlap across corpora also exists (19-45% of test pairs have their intended word in some train/validation split: Birkbeck 22.8%, finger slips 19.2%, GTC 44.6%, Wikipedia 42.3%; up to 126 identical (typo, word) pairs, 215 in total, 126 of them in Wikipedia); restricted to test pairs with no overlap the difference is +0.0138 (SE 0.0018), on the overlapping subset +0.0109 (SE 0.0027) (`calib --exploratory`). The pre-registered claim that no word is in two roles holds only within a corpus (Birkbeck and finger slips by word bucket); the two typo-corpus holdouts were disjoint by pair and typo string only, and no word is shared between roles inside one corpus for Birkbeck and finger slips. Across corpora, overlap is not controlled.
- Pairs are unique and treated as independent, so the standard errors are somewhat optimistic: clustering the test differences by intended word (within a corpus) gives a macro SE of 0.0016 against 0.0015 (per corpus 0.0032, 0.0040, 0.0027, 0.0029; `calib --exploratory`), which does not change any conclusion. Weights are corpus frequencies from `big.txt`; most dictionary words have weight 0.
- The "after rounding" ranking is one definition among several (pre-registered); the grid has 120 models with it, none of them in the top 10 of training.
- One dictionary size (274 137 words), quality only, one machine for a deterministic quantity; no latency.

## Proposed follow-up (text for a new issue; not adopted here)

Title: "core: try the calibrated cost model (factor 1.25, neighbouring-key substitution 12, indel 12) with a latency budget"

Body: The pre-registered calibration (`docs/benchmarks/calibration.md`, part of #21) selected the model `first-byte factor 1.25, neighbouring-key substitution 12, ordinary indel 12, doubled-letter indel 8, transposition 12` (shipped: 1.5, 8, 16, 8, 12). On an untouched test split of 9754 pairs from four corpora it improved MRR@10 by +0.0134 (SE 0.0015, 95% interval [+0.0105, +0.0162]) over the shipped costs, with no significant loss in any corpus or in any error category with n >= 50, and by +0.0279 on finger slips; it also expands about 1.67 times the trie nodes at the same budget of 32. Before changing any default: (1) pre-register and run a second study on fresh data that isolates the parameters (at least `a12` alone with the other costs shipped, `a12` with factor 1.25, and the indel at 16), because the training marginals suggest the neighbour discount is the driver and the indel value may not matter; (2) measure latency (p50 and p95, dictionary sizes 10 000, 100 000 and full, `tsb` on) for the chosen model against the shipped one, since a 1.6 to 1.7 times node count may cost more than the ranking gain is worth; (2b) compare against the shipped costs at a budget with equal nodes per query (about 34-36 at 274k words) and the new costs at a budget matching the shipped node count, with the arm `f1.25 a12 i16/8` as a required arm (exploratory here: +0.0129, SE 0.0011, 1469 nodes per query at budget 32; see the node-matched controls in the report); (3) the costs are private fields today: adding the tunable model to the public API is an API decision for the maintainers (the experiment patch in `bench/experiments/cost-knobs.patch` shows the minimal change and is not meant to be merged as is); (4) update the tables in `cost.rs`, `README.md` and the rank documentation only if the follow-up study confirms. Acceptance: the same rule as here (macro 95% interval above 0 on data not used to choose, no significant loss), plus latency and node counts reported next to it.

## How to reproduce

From the repository root, after `cd bench && npm ci && node fetch-data.mjs && node prepare-m0-data.mjs && node fetch-typo-corpora.mjs --holdout && node fetch-finger-slips.mjs` (the finger-slip archive is 1.5 GB; see `finger-slips.md`; nothing is committed):

```sh
git apply bench/experiments/cost-knobs.patch
cargo run --release -p keyhammer-bench --features calibration --bin calib -- bench/data --threads 6
git checkout crates/keyhammer/src/cost.rs
```

The patch was regenerated on 2026-09-24 against the layout-aware `cost.rs` of #62 (`CostModel::qwerty_with` builds on `CostModel::qwerty`); with it the run reproduces this report byte for byte (same tables, and the shipped model scores 0.8407 on the slips sample of #60). `--exploratory` prints the review analyses above (budget controls, word overlap, clustered SE, first-letter concentration). Building `calib` without the patch stops in `bench/build.rs` with a message. `--counts` prints only the split sizes; `--selfcheck` runs three consistency checks on 300 training queries (`Hit::cost` against an independent DP, `Coarse` against units-weight-id over the `Exact` list, completeness of "after" lists) and prints pass or fail. The run writes `bench/data/calib-train-grid.tsv`, copied here as `calibration-train-grid.tsv`.
