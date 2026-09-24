# Pre-registration: ranking tie-break variant and weight-0 ties (issue #43)

Written and committed BEFORE any ranking was evaluated on the test samples described here. The result report (`tiebreak-result.md`) is written afterwards and must be read against this file. Anything the result report does that is not described here is labelled as exploratory there.

## What is tested

Default ranking, `Ranking::Coarse` of `SearchConfig::default()` (budget 32, `tsb` on or off, results are identical): order the returned terms by

1. the weighted cost rounded up to whole units of 16 (`whole_units`), ascending;
2. higher weight;
3. lower term id.

Variant, fixed now and not to be changed after seeing data: order by

1. whole units, ascending;
2. higher weight;
3. lower EXACT weighted cost (`Hit::cost`);
4. lower term id.

The variant differs from the default only among terms that have the same whole units and the same weight: the default lets the term id decide there, the variant lets the exact cost decide first. Candidates (budget, band, subtree bound) are unchanged, so the set of terms within budget is identical; only the order among them differs. `Ranking::Exact` (exact cost, then weight) is a different variant and is not tested here.

Origin: in the ranking-variants experiment (issue #40) this variant was added after seeing the data and gave about +0.003 MRR (z about 1.6 to 1.75). That is why it is tested again on samples that were not used then.

## Data (all fixed now)

- Dictionaries: `words-10000.tsv`, `words-100000.tsv` and `words-full.tsv` (274 137 words) from `bench/prepare-m0-data.mjs`, weights as in those files. SHA-256: `73504b33fe89c59cdbb18f033db57331ecf5a244e8e32264e8ca4c2d36d23b76`, `d80689720c82b701efc994645ad6fa5462240ec99841a1683c329bb853827e38` and `fda37a01625cad137afff97428661e57bc9837f8b477b2fececa74c9de6181b5`.
- Typo corpora, both from `bench/fetch-typo-corpora.mjs` (pinned sources and SHA-256 of the downloads are in that script). The script's filters are unchanged: both words `^[a-z]+$`, correct word (at least 3 letters) in the full dictionary, typo (at least 2 letters) not in it, one differing token for the GitHub Typo Corpus, single correction for Wikipedia.
- Already used (by issue #40 and the competitive benchmark): `gtc.tsv` (1000 pairs, seed 20260924, sha256 `091dfd81ef0f13b901bfec40dc1617b74867832b3b3efec35a3cb831378c04e2`) and `wiki.tsv` (1000 pairs, same seed, sha256 `d3dc05e3a52b1a2c5c06bac4bbfcaafdbfffbe3a501bcd95d457819008b9c3f9`). The #40 experiment is not in the repository; the assumption is that its "1000-sample" per corpus is these two files (same script, same seed, same size). If it was not, disjointness from the #40 data is not guaranteed and the result report must say so.
- New samples, produced by `node bench/fetch-typo-corpora.mjs --holdout`:
  - `gtc-holdout.tsv`: 2000 pairs drawn from the 24 784 usable pairs left after removing every pair of `gtc.tsv` and every pair whose typo string equals a typo of `gtc.tsv` (the pool has 25 880 unique usable pairs). Seed 20260943 (different from 20260924), same shuffle as the original sample. sha256 `1508e677ef3d626a0bcd5317d8a7628a0950fe7f9a3b23f6bb213606078182c0`.
  - `wiki-holdout.tsv`: every usable pair not used before, 2754 pairs (3754 unique usable pairs minus the 1000 of `wiki.tsv`; no typo string of `wiki.tsv` recurred, so the typo-string exclusion removed none). The disjoint Wikipedia pool (2754) is larger than 2000, so all of it is used and no subsampling happens. sha256 `199b7eaabbf21570cd031c1294849fb31e6d00472efe968a2c2fb40549620701`.
- The sample files were generated (not evaluated) before this document was committed. No ranking of any kind was run on them.
- Birkbeck is not used (it was not part of the disjoint design and is small).

## Primary metric

MRR@10 (reciprocal rank of the correct word among the top 10, 0 if absent) of the variant minus the default, per query, paired on the same query, at 10 000, 100 000 and 274 137 words. Report the mean difference, its standard error (sample standard deviation of the per-query differences over the square root of n) and the 95% interval mean +/- 1.96 SE. The correct word may be absent from the 10 000 and 100 000 dictionaries; those queries score 0 for both and stay in the sample.

The primary sample is the two new corpora pooled (2000 + 2754 = 4754 pairs). Per-corpus results are reported as secondary.

## Decision rule

Adopt the variant as the default only if ALL of the following hold:

1. At the full dictionary (274 137 words), on the pooled primary sample, the paired 95% interval of the MRR@10 difference (variant minus default) lies entirely above 0 (lower end > 0).
2. No error category with n >= 50 (pooled sample, categories below) has a significantly negative difference at the full dictionary: significantly negative means mean difference < 0 and mean + 1.96 SE < 0.
3. Neither corpus on its own has a significantly negative overall difference at the full dictionary (same definition).

The issue text says "more than about 1.5 standard errors". This rule is stricter (1.96) on purpose. Verdicts: all three conditions hold, adopt. Otherwise: if the pooled interval at the full dictionary lies entirely below 0 or contains 0 with its upper end below +0.005, or if condition 2 or 3 fails, do not adopt (a trade-off, if any, is reported). If the interval contains 0 but its upper end is at least +0.005 (a gain of that size is not excluded), the verdict is inconclusive. The +0.005 is fixed now and is arbitrary; it is roughly twice the size of the #40 lead. Checks at 10 000 and 100 000 words are reported but do not enter the rule. Nodes expanded are not compared: the variant does not change the search, only the order of the results (a claim of fewer nodes in #40 is not tested here). No latency is measured. No multiplicity correction is applied to the category check (it is a loss check, so the lack of correction is conservative for adoption).

The default stays as it is whatever the outcome of this document; adopting is a separate change.

## Error categories

From the pair (typo, correct), by the unit optimal-string-alignment distance d (adjacent transposition counts 1):

- d = 1 and the typo is the correct word with two adjacent letters swapped: `transposition`.
- d = 1, the typo lacks one letter of the correct word: `missing doubled letter` if some valid deletion position removes a letter equal to an adjacent letter of the correct word, else `missing letter`.
- d = 1, the typo has one extra letter: `extra doubled letter` if the extra letter equals an adjacent letter in the typo (valid position), else `extra letter`.
- d = 1, one substitution: `neighbouring-key substitution` if `CostModel::qwerty().sub_cost(typo_letter, correct_letter, 1) < 16`, else `other substitution`.
- d >= 2: `two or more edits`.
- Overlay, not exclusive: `first letter differs` (typo[0] != correct[0]).

Categories with n < 50 are reported but do not enter condition 2.

## Tie analysis

Computed for the DEFAULT ranking on the pooled sample and per corpus, at each of the three dictionary sizes, over the top 10. A "key" of a hit is (whole units of its exact cost, weight). Two hits with the same key are ordered by term id only.

- T1: share of queries whose top 10 holds at least two hits with the same key (a tie decided only by term id).
- T2: share of queries in which the correct word is in the top 10 and shares its key with at least one other top-10 hit.
- T3: share of queries in which the correct word is NOT in the top 10 but has the same key as the 10th hit (it was cut off by the id tie-break at the boundary). The search is run with a larger k until every term with units <= the 10th hit's units is returned, so this is exact.
- Weight-0 breakdown: each of T1 and T2 split by whether the tied key has weight 0 or weight > 0 (a query counts under "weight 0" if some tied pair in the top 10, respectively the correct word's tie, has weight 0; a query can count under both for T1); the share of correct words of the sample with weight 0 in each dictionary; and the mean MRR difference of the variant restricted to queries counted in T2 and in T3.
- For the variant, the same T1 is reported with a key extended by the exact cost (a tie the variant still leaves to the term id).

These are descriptive, with counts and shares, no test; nothing in the decision rule depends on them.

## Implementation (fixed in advance)

`bench/src/bin/tiebreak.rs`. No change to `crates/keyhammer`, no new public API. The variant is computed in the bench from the public `Hit` (id, exact cost, weight): search with the default configuration, doubling k from 10 until the last hit has more units than the 10th hit or fewer than k hits are returned (then every term with units up to the 10th hit's is present), and reorder by the variant key. A self-check compares this against `Ranking::Exact` with a very large k, sorted by the variant key, on every query; any mismatch is reported and stops the run. Searches that hit `max_nodes` are counted and reported.

## Not covered

One machine (deterministic quantities only), English dictionary, two corpora, the QWERTY cost model; the Wikipedia pairs are a curated list of common misspellings; correlated pairs (same correct word several times) are treated as independent, which makes the standard errors somewhat optimistic; the effect of a genuine frequency prior is not tested (the weights in these dictionaries are what they are; the tie analysis only says how often weight fails to decide).
