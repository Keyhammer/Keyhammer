# Result: pre-registered test of the ranking tie-break variant (issue #43)

Verdict, against the rule fixed in `tiebreak-preregistration.md` before any ranking was run on these samples: **do not adopt**. The variant gains +0.0030 MRR@10 overall at 274 137 words (paired 95% interval [+0.0013, +0.0047]), so condition 1 holds, but it loses significantly on two error categories with n >= 50 (`missing letter`, `extra letter`), so condition 2 fails. The default stays `Ranking::Coarse`; nothing in `crates/keyhammer` changed.

Deterministic quantities only (MRR, tie counts), so one run is the result; no latency was measured, and node counts were not compared (the variant does not change the search). One machine, English dictionaries, QWERTY costs.

## What was run

- Harness: `bench/src/bin/tiebreak.rs`, run as `cargo run --release -p keyhammer-bench --bin tiebreak -- bench/data`, after `node bench/fetch-typo-corpora.mjs --holdout` (data is not committed). It takes about 10 s.
- Samples as pre-registered: `gtc-holdout.tsv` (2000 pairs, seed 20260943, disjoint from the 1000-pair `gtc.tsv`) and `wiki-holdout.tsv` (2754 pairs, all usable pairs not in `wiki.tsv`), 4754 pooled. The script reproduced the recorded SHA-256 of the two old samples and of the two new files. The assumption that the samples of issue #40 are `gtc.tsv` and `wiki.tsv` could not be checked (that experiment is not in the repository). Disjointness is per corpus, as pre-registered: 46 holdout pairs also occur in the other corpus's old sample (so in the #40 data) and 68 pairs occur in both holdouts (counted twice in the pooled sample). Excluding these 114 (exploratory, not pre-registered) gives +0.0031 [+0.0013, +0.0048] and the same two category losses (missing letter -0.0035, extra letter -0.0030); the verdict is unchanged. The holdout-generation code was committed with the harness, after the pre-registration; the pre-registered SHA-256 pins its output.
- Variant: whole units, then higher weight, then exact cost, then term id, computed in the bench from the default search (k doubled until every term with at most the 10th hit's units is returned, then reordered). Self-check against `Ranking::Exact` with all terms within budget, sorted by the variant key: 0 mismatches over 4754 queries at each of the three sizes. No search reached `max_nodes` (0 truncated at k = 10 and in the expansion).
- The definitions of categories and ties are those of the pre-registration; nothing was added after the run except this prose.

## Primary: MRR@10, variant minus default (paired)

At 274 137 words (the size the rule uses; the correct word is in this dictionary for every query):

| sample | n | MRR default | MRR variant | diff | SE | 95% interval | queries that differ |
|---|---|---|---|---|---|---|---|
| pooled | 4754 | 0.8698 | 0.8728 | +0.0030 | 0.0009 | [+0.0013, +0.0047] | 120 |
| gtc-holdout | 2000 | 0.8151 | 0.8195 | +0.0044 | 0.0015 | [+0.0016, +0.0073] | 77 |
| wiki-holdout | 2754 | 0.9095 | 0.9115 | +0.0020 | 0.0010 | [-0.0001, +0.0040] | 43 |

At the smaller dictionaries (not part of the rule). 100 000 words (the correct word is absent from 2817 of 4754 queries, which score 0 for both):

| sample | n | MRR default | MRR variant | diff | SE | 95% interval | queries that differ |
|---|---|---|---|---|---|---|---|
| pooled | 4754 | 0.3751 | 0.3753 | +0.0002 | 0.0004 | [-0.0005, +0.0009] | 20 |
| gtc-holdout | 2000 | 0.3531 | 0.3536 | +0.0005 | 0.0007 | [-0.0009, +0.0019] | 14 |
| wiki-holdout | 2754 | 0.3911 | 0.3911 | -0.0000 | 0.0003 | [-0.0006, +0.0006] | 6 |

10 000 words (absent from 4374 of 4754 queries):

| sample | n | MRR default | MRR variant | diff | SE | 95% interval | queries that differ |
|---|---|---|---|---|---|---|---|
| pooled | 4754 | 0.0764 | 0.0763 | -0.0001 | 0.0001 | [-0.0003, +0.0001] | 1 |
| gtc-holdout | 2000 | 0.0695 | 0.0693 | -0.0003 | 0.0003 | [-0.0007, +0.0002] | 1 |
| wiki-holdout | 2754 | 0.0813 | 0.0813 | +0.0000 | 0.0000 | [+0.0000, +0.0000] | 0 |

The difference is only visible at the full dictionary, where the tie groups are large. At 10 000 and 100 000 words the interval contains 0.

## Categories at 274 137 words (pooled), difference variant minus default

| category | n | diff | SE | 95% interval | significant loss | in rule (n >= 50) |
|---|---|---|---|---|---|---|
| transposition | 653 | +0.0130 | 0.0031 | [+0.0069, +0.0190] | no | yes |
| missing letter | 1253 | -0.0034 | 0.0012 | [-0.0057, -0.0011] | yes | yes |
| missing doubled letter | 318 | +0.0175 | 0.0049 | [+0.0078, +0.0272] | no | yes |
| extra letter | 669 | -0.0030 | 0.0014 | [-0.0057, -0.0003] | yes | yes |
| extra doubled letter | 314 | +0.0140 | 0.0045 | [+0.0052, +0.0229] | no | yes |
| neighbouring-key substitution | 207 | +0.0170 | 0.0058 | [+0.0056, +0.0285] | no | yes |
| other substitution | 634 | -0.0013 | 0.0011 | [-0.0035, +0.0010] | no | yes |
| two or more edits | 706 | -0.0009 | 0.0024 | [-0.0057, +0.0038] | no | yes |
| first letter differs | 160 | +0.0099 | 0.0083 | [-0.0063, +0.0261] | no | yes |

Two of the categories with n >= 50 have an interval entirely below 0: `missing letter` (n = 1253, -0.0034, SE 0.0012) and `extra letter` (n = 669, -0.0030, SE 0.0014). The gains are in `transposition`, `missing doubled letter`, `extra doubled letter` and `neighbouring-key substitution` (+0.013 to +0.018), the categories where cheap edits make the exact cost differ between candidates. The losses are small (about 0.003) but they are what the rule tests. No multiplicity correction was applied to this loss check, as pre-registered: with 9 rows one borderline loss (`extra letter`, upper end -0.0003) could arise by chance, `missing letter` (z about -2.8) less likely. The rule does not allow deciding on that after the fact.

## Decision rule at 274 137 words

| condition | result |
|---|---|
| 1. pooled interval lower end > 0 | PASS: +0.0030, SE 0.0009, [+0.0013, +0.0047] |
| 2. no category with n >= 50 significantly negative | FAIL: `missing letter`, `extra letter` |
| 3. neither corpus significantly negative | PASS: upper ends +0.0073 (GitHub), +0.0040 (Wikipedia) |
| verdict | do not adopt |

Also visible: on the Wikipedia sample alone the interval includes 0 ([-0.0001, +0.0040]); the gain is carried by the GitHub Typo Corpus sample (+0.0044, SE 0.0015). The +0.003 of issue #40 is reproduced on new data (+0.0030). It is a real gain at the scale of a few thousandths of MRR, and it comes with a small loss on the two most frequent kinds of error. That is a trade-off, which the rule was written to refuse.

## Ties (descriptive; nothing in the rule depends on them)

Default ranking, top 10; key = (whole units, weight), hits with equal keys are ordered by term id only. T1: queries whose top 10 holds such a tie. T2: the correct word is in the top 10 and shares its key with another hit. T3: the correct word is not in the top 10 but has the same key as the 10th hit (cut off by the id tie-break; exact, from the expanded search). "Weight-0" columns: the tie involves a weight-0 word. The last column is the share of queries where the variant still leaves a tie to the term id (equal units, weight and exact cost).

274 137 words:

| sample | n | T1 any id-only tie | T1 with a weight-0 tie | T1 with a weight>0 tie | T2 correct in a tie | T2 correct in a weight-0 tie | T3 correct cut off by the tie at rank 10 | correct word has weight 0 | T1 left by the variant (exact cost also equal) |
|---|---|---|---|---|---|---|---|---|---|
| pooled | 4754 | 69.9% (3323) | 64.6% (3069) | 13.7% (650) | 6.8% (324) | 6.1% (291) | 0.4% (19) | 23.5% (1117) | 62.1% (2950) |
| gtc-holdout | 2000 | 75.3% (1506) | 69.1% (1382) | 17.1% (341) | 8.7% (174) | 7.8% (155) | 0.8% (16) | 27.6% (552) | 66.9% (1338) |
| wiki-holdout | 2754 | 66.0% (1817) | 61.3% (1687) | 11.2% (309) | 5.4% (150) | 4.9% (136) | 0.1% (3) | 20.5% (565) | 58.5% (1612) |

MRR difference of the variant on the T2 queries (n = 324): +0.0421 (SE 0.0124, [+0.0178, +0.0664]); on the T3 queries (n = 19): +0.0321 (SE 0.0152, [+0.0024, +0.0618]). These subsets are chosen by a property of the default ranking, so the intervals are descriptive.

100 000 words:

| sample | n | T1 any id-only tie | T1 with a weight-0 tie | T1 with a weight>0 tie | T2 correct in a tie | T2 correct in a weight-0 tie | T3 correct cut off by the tie at rank 10 | correct word has weight 0 | T1 left by the variant (exact cost also equal) |
|---|---|---|---|---|---|---|---|---|---|
| pooled | 4754 | 49.8% (2369) | 46.4% (2205) | 8.6% (408) | 1.6% (76) | 1.5% (71) | 0.0% (2) | 8.8% (417) | 43.8% (2084) |
| gtc-holdout | 2000 | 56.1% (1122) | 51.5% (1029) | 10.7% (214) | 2.4% (47) | 2.2% (44) | 0.1% (1) | 9.9% (198) | 49.1% (983) |
| wiki-holdout | 2754 | 45.3% (1247) | 42.7% (1176) | 7.0% (194) | 1.1% (29) | 1.0% (27) | 0.0% (1) | 8.0% (219) | 40.0% (1101) |

10 000 words (the correct word is mostly absent from this dictionary, so T2 and T3 are almost empty):

| sample | n | T1 any id-only tie | T1 with a weight-0 tie | T1 with a weight>0 tie | T2 correct in a tie | T2 correct in a weight-0 tie | T3 correct cut off by the tie at rank 10 | correct word has weight 0 | T1 left by the variant (exact cost also equal) |
|---|---|---|---|---|---|---|---|---|---|
| pooled | 4754 | 15.1% (719) | 14.9% (707) | 1.0% (49) | 0.0% (1) | 0.0% (1) | 0.0% (0) | 1.1% (53) | 12.7% (603) |
| gtc-holdout | 2000 | 18.8% (375) | 18.4% (367) | 1.4% (28) | 0.1% (1) | 0.1% (1) | 0.0% (0) | 1.4% (27) | 15.6% (312) |
| wiki-holdout | 2754 | 12.5% (344) | 12.3% (340) | 0.8% (21) | 0.0% (0) | 0.0% (0) | 0.0% (0) | 0.9% (26) | 10.6% (291) |

Readings, at 274 137 words, pooled:

- In 69.9% of queries (3323 of 4754) the default's top 10 contains at least two hits whose order is decided only by the term id; in 64.6% one of those ties involves weight-0 words, against 13.7% for ties among words with weight above 0.
- The correct word sits in such a tie in 6.8% of queries (324), in a weight-0 tie in 6.1% (291); in 0.4% (19) the correct word misses the top 10 because of an id-only tie at rank 10.
- 23.5% of the correct words of the samples have weight 0 in the full dictionary (1117 of 4754), so for about a quarter of queries the dictionary gives the correct word no frequency information. Whether a real frequency prior would help was not tested; this only shows how often the weight does not decide. Any reading that a better prior would raise MRR is a hypothesis.
- The exact-cost tie-break resolves only some of the id-only ties: 62.1% of queries still have one after it, against 69.9%.

## Caveats

- One dictionary family (the M0 English list, weights as in that file), one cost model, two corpora, one run. The Wikipedia list is curated common misspellings.
- Pairs are treated as independent (4754 pairs, 3345 distinct correct words); pairs sharing a correct word are correlated (max 21 per word), so the standard errors are optimistic.
- The gain, +0.0030, is small; the pre-registered rule asks for no significant loss anywhere, and that is what failed.
- The self-check shows the bench-side expansion returns every candidate the variant needs (the key itself is the same code on both routes). The variant is a bench-side reordering, not an engine change; adopting it would need a native tie key in `search.rs`, and a ranking option with its oracle test.

## Reproduce

```
node bench/fetch-typo-corpora.mjs --holdout      # writes gtc-holdout.tsv and wiki-holdout.tsv (checks the old samples' SHA-256)
cargo run --release -p keyhammer-bench --bin tiebreak -- bench/data
```
