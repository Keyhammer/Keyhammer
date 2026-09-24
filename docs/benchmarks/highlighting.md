# Match highlighting: cost per hit, and the search unchanged (issue #32), 2026-09-24

Questions: (1) does factoring the DP cell out of the search (so that the highlighting
traceback uses the same cost code, `docs/design/highlighting.md` section 7) change the search?
(2) What does highlighting cost per hit? Design: `docs/design/highlighting.md`.

## Summary

- **Search results and work: identical.** On the M0 data (300 Birkbeck typo pairs; 10 000,
  100 000 and 274 137 words), `main` (commit 60c04e3) and this branch give, for eight
  configurations (default, `tsb: false`, `Ranking::Exact`, `high_recall()`, each in exact and
  prefix mode), the same trie node count and the same totals of `nodes_expanded`,
  `nodes_pushed` and `rows_computed` and the same digest of every hit (id, cost, weight). At
  10 000 words, default exact search: 147 181 nodes expanded, 148 044 pushed, 588 032 rows on
  both sides. This is deterministic.
- **Highlighting work** (deterministic): a whole-term hit of the default search costs 52 to 65
  DP cells on average (at most 240), a prefix hit 16 to 46. Highlighting all hits of a query
  (at most 10) costs 105 to 455 cells, against 859 to 47 141 cells of the search that found
  them: from 0.6% (whole-term, 10 000 words) to 19% (prefix of 3 characters, 274 137 words) of
  the search's cell work.
- **Highlighting time** (one machine, shared, best of five passes, median of three runs): 0.22 to
  0.56 us per hit, 0.9 to 4.4 us per query, against 8 to 246 us for the search of that query.
- **Search time, main against branch** (one machine, shared, see method): medians of five runs
  differ by -0.5% to +2.0% at 10 000 and 100 000 words, and runs at 274 137 words were too noisy
  to separate (single runs of one binary spread by up to 31%). A first version of the refactor,
  which returned the four candidates in an array, was measured 6% to 9% slower on every row and
  was replaced by a visitor function before this change; see "The refactor" below.

## Method

- Machine: one Windows 11 PC, shared with other agents. Processor load sampled before the timed
  runs: 12% to 17% (other agents' processes idle or light; no other cargo or benchmark process).
  Not a controlled benchmark; the deterministic counters are the result, the times are
  indications.
- Search equivalence: a throwaway program (not committed, as in `unicode.md`) built once against
  `git archive origin/main` and once against the branch, reading the M0 data, printing per
  dictionary and configuration the summed work counters and an FNV digest of the hits. The two
  outputs were compared with `diff`: no difference. The same program timed the default
  configuration (best of seven passes over the 300 queries per run), alternating the two
  binaries, five runs each.
- Highlighting: `bench/src/bin/highlight.rs` (committed):
  `cargo run --release -p keyhammer-bench --bin highlight -- [--time] bench/data`. For each
  dictionary it searches the 300 misspellings with `SearchConfig::default()` in whole-term mode,
  and their first 3 and 5 characters in prefix mode, then highlights every hit (at most 10 per
  query) with `Searcher::highlight`, checking that each succeeds with its hit's cost (all did:
  exit status 0). "cells/query (search)" is `rows_computed` times the row width 9
  (`2 * 32 / 8 + 1`). With `--time`: best of five passes of all searches, then of all
  highlightings; three runs, medians below.

## Results

Deterministic, per dictionary and mode (`SearchConfig::default()`, QWERTY):

| words | mode | queries | hits | cells/hit mean | p95 | max | cells/query (highlight) | cells/query (search) | ranges/hit |
|---|---|---|---|---|---|---|---|---|---|
| 10000 | whole | 300 | 601 | 52.2 | 132 | 225 | 105 | 17641 | 1.76 |
| 10000 | prefix 3 | 300 | 3000 | 18.3 | 24 | 24 | 183 | 2112 | 1.08 |
| 10000 | prefix 5 | 300 | 3000 | 45.5 | 60 | 60 | 455 | 9772 | 1.47 |
| 100000 | whole | 300 | 1394 | 58.1 | 121 | 225 | 270 | 40239 | 1.82 |
| 100000 | prefix 3 | 300 | 3000 | 16.5 | 24 | 24 | 165 | 1069 | 1.02 |
| 100000 | prefix 5 | 300 | 3000 | 41.4 | 48 | 60 | 414 | 6762 | 1.38 |
| 274137 | whole | 300 | 1791 | 64.6 | 132 | 240 | 386 | 47141 | 1.85 |
| 274137 | prefix 3 | 300 | 3000 | 16.3 | 16 | 24 | 163 | 859 | 1.01 |
| 274137 | prefix 5 | 300 | 3000 | 40.0 | 48 | 60 | 400 | 5694 | 1.34 |

The prefix cell counts stay within the designed bound `(m + 1) * (min(n, m + cost / 8) + 1)`:
with the default budget of 32 (so `cost / 8 <= 4`) at most `4 * 8 = 32` cells for a 3-character
query (24 observed) and `6 * 10 = 60` for 5 characters (60 observed).

Time, median of three runs (microseconds):

| words | mode | search us/query | highlight us/hit | highlight us/query |
|---|---|---|---|---|
| 10000 | whole | 93.1 | 0.46 | 0.9 |
| 10000 | prefix 3 | 14.1 | 0.24 | 2.4 |
| 10000 | prefix 5 | 51.0 | 0.44 | 4.4 |
| 100000 | whole | 208.8 | 0.51 | 2.4 |
| 100000 | prefix 3 | 9.6 | 0.22 | 2.2 |
| 100000 | prefix 5 | 39.4 | 0.40 | 4.0 |
| 274137 | whole | 246.0 | 0.56 | 3.4 |
| 274137 | prefix 3 | 8.1 | 0.22 | 2.2 |
| 274137 | prefix 5 | 33.6 | 0.39 | 3.9 |

A highlight includes normalising the source string (here the identity normaliser of a
`Trie::build` trie, which still builds a source map) and one allocation for the ranges.

Search time, default configuration, `main` against the branch, medians of five alternated runs
(microseconds per query):

| words | mode | main | branch | difference |
|---|---|---|---|---|
| 10000 | exact | 94.3 | 93.8 | -0.5% |
| 10000 | prefix | 90.7 | 91.3 | +0.7% |
| 100000 | exact | 207.1 | 211.2 | +2.0% |
| 100000 | prefix | 180.3 | 182.5 | +1.2% |
| 274137 | exact | 253.5 | 262.0 | noisy (runs 244.0 to 319.5 on main) |
| 274137 | prefix | 257.9 | 250.5 | noisy (runs 205.9 to 273.4 on main) |

A difference of 1% to 2% at 100 000 words may be real (in exact mode the three fastest runs of
each side differ by 4.0 to 4.1 us) or may be code layout; it was not investigated further.

## The refactor

The first version of the shared cell function returned the four candidate values in an array,
and the search took their minimum. On the same program it was 7% to 8% slower than `main` in
every row (6% to 9%; for example 99.1 against 91.7 us at 10 000 words, exact, in the first
alternated run), with identical counters.
Restoring the old hand-written minimum in `child_row` alone brought the time back to `main`
(92.4 to 92.9 us), so the cost was the array, not the rest of the change. The shipped version,
`search::each_move`, calls a closure for each available move in the old order; the search folds
the minimum (`cell_min`) and the traceback collects the four values (`moves`). Both still go
through the one function that calls the cost model.

## Mutation tests

Each deliberate bug below was applied to the code, `cargo test -p keyhammer --release` run, and
the change reverted. All but one make at least one test fail:

| mutation | caught by |
| --- | --- |
| tie order: deletion first | `ties_put_edits_leftmost`, `traceback_cost_equals_oracle_and_hit_cost` |
| tie order: insertion before the diagonal move | `traceback_cost_equals_oracle_and_hit_cost` (tie rule checked on an independent matrix) |
| transposed symbols not highlighted | `transposed_letters_are_highlighted` |
| substituted symbols highlighted | eight golden and property tests |
| prefix mode: longest instead of shortest prefix of minimal cost | `traceback_cost_equals_oracle_and_hit_cost` |
| partial expansion: all symbols instead of any | `a_partially_typed_expansion_counts_as_typed`, a doctest |
| dropped marks do not take their letter's state | `a_dropped_combining_mark_follows_its_letter` |
| UTF-16 counted in bytes | golden UTF-16 tests, the property test, the fuzz twin, a doctest |
| `aligned` not extended over marks | the property test and the fuzz twin |
| no cost check | `impossible_costs_are_refused_before_any_work`, `errors_are_typed` |
| traceback matrix one term symbol short | seven tests |
| traceback reads the wrong previous term symbol (doubled letters) | the property tests and the fuzz twin |
| shared cell function: no x1.5 on a first-symbol deletion | 29 tests (search and highlighting) |
| shared cell function: doubled-letter insertion ignored | 30 tests (search and highlighting) |
| shared cell function: transposition of two equal symbols allowed | **not caught: equivalent** |

The last mutation is equivalent: with the other two conditions, two equal query symbols can only
be "transposed" with two equal term symbols, which two zero-cost matches align more cheaply, so
the move is never optimal and no cost or range changes.

## Not measured, and caveats

- Non-ASCII dictionaries (no benchmark corpus here); `Trie::build_normalized` sources, where the
  normalisation and the source map do real work. The property tests cover their correctness only.
- Queries near `MAX_QUERY_LEN`: the bound is at most `(m + 1) * (m + 9)` cells (18 369 at 128
  symbols), not measured.
- One machine, shared; the times are indications, the cell counts are the result.
