# Prefix (autocomplete) mode (issue #30)

`Searcher::search_prefix` returns the best terms that *start with* something close to the
query. This note fixes the semantics, adapts the lower-bound proof of
`docs/design/lower-bound.md` to it, and says what is tested and what is only argued. Every
definition of that note (weighted OSA, the band, the row lemma, `LB`) is reused; read it
first. Nothing here is claimed to be new: the idea is the usual one for autocomplete over a
trie, with the proof redone for the weighted, banded, subtree-bounded search.

## 1. Decisions

- **Cost.** `pcost(t) = min over j in 0..=|t| of D_t[m][j]`: the smallest weighted OSA cost
  between the whole query `q` (`m = |q|`) and any prefix `t[..j]` of the term, the empty
  prefix included. `D` is the recurrence of lower-bound.md section 1, so a transposition
  must lie inside the prefix (both swapped bytes in `t[..j]`, both in `q`); the cost of an
  alignment never depends on the term bytes after the cut.
- **Ranking.** Identical to `search`: the key is `(rank(pcost), 65535 - weight, id)`, with
  `rank` = whole units (`Ranking::Coarse`, the default) or the exact cost
  (`Ranking::Exact`), then higher weight, then lower term id. `Hit::cost` is the exact `pcost`.
  So among terms with the same rounded cost the heavier comes first, which is what an
  autocomplete list wants.
- **What a hit is.** The full term (its `id`); the matched prefix length is *not* reported.
  Reasons: it is not needed for ranking; the best prefix can be a tie between several lengths
  and a rule would have to be chosen; and it would add a field to `Hit`, which every consumer
  (including the bindings, which this change does not touch) would see. A caller that needs the
  cut can recompute the alignment. Possible follow-up: report the shortest prefix that attains
  `pcost`.
- **Budget, `k`, empty query.** `budget` bounds `pcost` and has the same limits (at most 64,
  otherwise `BudgetTooLarge`); queries above `MAX_QUERY_LEN` are `QueryTooLong`; `k = 0`
  returns nothing. The empty query has `pcost = 0` for every term (the empty prefix), so the
  result is the `k` heaviest terms (ties by id), the usual answer for an empty box. A query
  longer than the term is fine: its extra bytes are deletions against the whole term.
- **API.** A new method, `Searcher::search_prefix(trie, cm, q, cfg)`, returning the same
  `Output`, with the same `SearchConfig` (`k`, `budget`, `tsb`, `max_nodes`, `ranking`). A
  `mode` field on `SearchConfig` was rejected: `SearchConfig` is not `non_exhaustive` and the
  WebAssembly binding and the benchmarks build it with a full struct literal, so a new field
  would break them. The method is purely additive. `Stats` gained `rows_computed` (banded DP rows
  computed) so that the work of the two modes can be compared; `nodes_expanded` keeps its meaning
  (a node popped from the queue and expanded).
- **`max_nodes`, truncation.** As in `search`: the search stops after `max_nodes` expansions and
  sets `Stats::truncated`; the hits already returned are still the first hits of the full order.
  Expansions of settled nodes (section 4) count too, although they compute no row, so one
  `max_nodes` is not the same amount of work in the two modes.
- **`tsb`.** Kept and adapted (section 3); results are identical with it on or off.

## 2. Search

For a node `v` at depth `j` write

- `acc(v) = min over ancestors-or-self u of v of R_u[m]`, where `R_u[m]` is the banded cell of
  the whole query (`INF` if there is none or it is not finite): the best cost of a prefix that
  ends at or above `v`;
- `lbd(v)`: a lower bound on `D_t[m][j']` for every term `t` at or below `v` and every
  `j' >= j` (the prefix of length `j` is included; `acc` covers it too, so the overlap is harmless),
  whenever that cost is at most the budget.

Every term below `v` has `pcost >= min(acc(v), lbd(v))`; that is the queue bound of `v` (its
rank, then the subtree's largest weight, then 0). `acc` is carried down: a child's `acc` is the
parent's `acc` minimum the child's own cell (`Expander::prefix_cell`). A term that ends at `v`
has all its prefixes at or above `v`, so its exact cost is `acc(v)`; it is pushed as a terminal
when `acc(v)` is within the budget. Otherwise `search_prefix` mirrors `search` (root, pop order,
pushes).

The row lemma (lower-bound.md section 1) holds unchanged. The cell of the whole query at depth
`j` is `k = m + W - j` (in the band iff `|m - j| <= W`).

**Lemma A.** For a term `t` at or below `v` (depth `j`), `min over j' <= j of D_t[m][j'] = acc(v)`
when that minimum is at most `B`, and `acc(v) = INF` otherwise. *Proof.* Each `D_t[m][j']` with
`j' <= j` is the cell `R_u[m]` of the ancestor `u` of `v` at depth `j'`, equal to the true value
when that is at most `B` and `INF` otherwise (row lemma; a cell outside the band costs more than
`B` by the band lemma). Take the minimum. ∎

## 3. The adapted bound

`lbd(v)` is `lower_bound` of lower-bound.md section 2 with one change, only in prefix mode, and the
queue bound is `min(acc, lbd)`: the length term keeps only the upper end: `gap = r - hi` if `r > hi`,
else 0 (`lo` is dropped), where `r = m - i` and `hi = len_max(v) - j`.

**Theorem (prefix admissibility).** Let `t` be at or below `v` and `j' >= j` with
`D_t[m][j'] <= B`. Then `lbd(v) <= D_t[m][j']`, with `tsb` on or off. Hence every term below `v`
with `pcost <= B` satisfies `min(acc(v), lbd(v)) <= pcost(t)`.

*Proof.* Take an optimal alignment of `q` with `t[..j']`, of cost `c = D_t[m][j'] <= B`. It runs
from `(0, 0)` to `(m, j')` with `j' >= j`, so as in the original theorem it either visits row `j`
(case A) or jumps over it with a transposition (case B).

*Case B* is unchanged: the parent's cell plus `c_transpose_min` is at most `c`; nothing after the
jump is used, so the changed length term does not matter.

*Case A.* Let `(i, j)` be its last cell in row `j`. The prefix cost `p` gives `D_v[i] <= p <= c <= B`,
so the cell is finite and `R[k] = D_v[i]`. The rest of the path, of cost `S`, goes from `(i, j)`
to `(m, j')` and `c = p + S >= D_v[i] + S`. It is enough to show `extra(k) <= S`:

- *Length.* The rest consumes `r = m - i` query bytes and `s = j' - j` term bytes, and
  `j' <= len_max(v)` gives `s <= hi`. By the argument of the band lemma it contains at least
  `|r - s|` insertions or deletions, each costing at least `c_indel_min`. If `r > hi` then
  `|r - s| >= r - hi`, so `S >= (r - hi) * c_indel_min`. That is `T_comp` without its `lo` part. The
  lower end `len_min(v) - j` cannot be used: the prefix can be shorter than every term below `v`
  (`s` ranges over `0..=hi`), so using it would be inadmissible, and a test with it back fails
  (section 6).
- *Missing letters.* `T_let(k) <= S`: word for word the argument of lower-bound.md section 3. It
  needs only that the term bytes consumed by the rest of the path, here `t[j..j']`, are labels of
  edges strictly below `v`, hence in `below_mask(v)`; a prefix of `t` has a subset of the classes
  of `t`.

`extra` is the maximum of valid lower bounds on `S`, hence valid. The last sentence of the theorem
follows from Lemma A: the part `j' <= j` of `pcost` is `acc(v)`, and the part `j' >= j` is at least
`lbd(v)` whenever it is at most `B`; if every alternative exceeds `B` there is nothing to prove. ∎

The key lemma of lower-bound.md section 5 carries over: `min(acc, lbd)` bounds every `pcost` below
`v`, rank is monotone and `max_weight(v) >= weight(t)`, so `key(v) <= key(t)`.

## 4. Settled subtrees

**Definition.** `v` is *settled* when `acc(v) < INF` and `lbd(v) >= acc(v)`.

**Lemma B.** If `v` is settled, every term at or below `v` has `pcost = acc(v)` exactly.
*Proof.* By Lemma A the prefixes `j' <= j` give `acc(v)`, which is finite, so at most `B`. If
some prefix `j' > j` had `D_t[m][j'] < acc(v) <= B`, the theorem would give
`lbd(v) <= D_t[m][j'] < acc(v)`, a contradiction. So `pcost(t) = acc(v)`. ∎

Below a settled node all costs are equal, so only the weight (then the id) orders the terms and no
row is needed. Expanding a settled node pushes its terminal (key
`(rank(acc), 65535 - weight, id)`) and each child as a settled entry with key
`(rank(acc), 65535 - max_weight(child), 0)`. That is the best-first order of the key
`(rank(acc), weight, id)` over the subtree; the node key is at most every key below it because
`max_weight` is at least every weight below, so by the argument of lower-bound.md section 5 the
terminals leave the queue in exactly the oracle order. This is the "all terms in a subtree become
candidates once the query is consumed within budget" of the issue, without computing rows for the
whole subtree.

The rest of the best-first argument (invariant, finality of the first terminal popped, termination,
truncation) is lower-bound.md section 5 with these keys.

## 5. What `tsb` does here

Nothing is disabled. `tsb` adds the length and missing-letter terms of section 3 to the cells of
`lbd`. Results are identical with `tsb` on
or off (tested); only the work differs.

## 6. What is tested and what is only argued

Tested (`cargo test -p keyhammer`):

- `src/search.rs`, `prefix_bound_never_exceeds_the_oracle_{with,without}_tsb`: 43,200 cases per
  mode, built like the exact bound test (600 random dictionaries over four alphabets, eight queries
  each: a quarter random words of length 0 to 10, the rest a truncated dictionary entry with 0 to 3
  edits; nine budgets, `W = 0` to `8`). Every trie node is walked as `search_prefix` walks it and
  checked: `lbd` is at most `D[m][j']` for every term below and every `j' > depth` whose cost is within
  the budget (against a full-matrix oracle); `min(acc, lbd)` and its queue key are at most the prefix
  cost and key of every within-budget term below, under both rankings; every term below a settled node
  has exactly the cost `acc`; at every terminal `acc` equals the oracle prefix cost when that is within
  the budget and is above the budget otherwise. Per mode: 1,397,664 nodes, 392,544 terminals (204,825
  within budget); 575,439 nodes are settled with `tsb` on and 572,231 with `tsb` off.
- `tests/oracle.rs`, `prefix_matches_the_oracle_on_*`: equality of `search_prefix` with a brute-force
  top-k (`oracle_topk_prefix` in `tests/support`: for each term the minimum over all prefixes of the
  weighted OSA cost) for `tsb` on and off, both rankings, alphabets of 3, 8 and 26 letters (120, 300 and
  400 terms), budgets 7 to 64, truncated dictionary words with edits, unrelated words and the empty query;
  node-limited runs must be a prefix of the oracle list. Also `prefix_cost_never_exceeds_the_exact_cost`.
- `tests/edge_cases.rs`, `prefix_*`: empty query, exact prefix, term equal to the query, a query longer
  than every term, a transposition inside the prefix at the boundary (`"ab"` against `"bax"` costs 18,
  which is 12 with the x1.5 first-byte factor), `k = 0`, budget 0, the error cases, truncation.
- `tests/fuzz_like.rs::prefix_matches_the_oracle` and the `prefix_oracle_equality` fuzz target
  (`tests/fuzz_props`): the same equality on generated bytes; `never_panics` also calls
  `search_prefix`. The fuzz target itself (a separate workspace needing nightly and libFuzzer) was not
  built or run here; only its property runs, through `fuzz_like.rs`.
- Mutations that make these tests fail: keeping the `lo` term in
  prefix mode; settling too eagerly (`lbd + 8 >= acc`); not taking the minimum with the parent's `acc`;
  ordering settled children with weight 0 instead of `max_weight`; dropping `acc` from the root bound;
  adding 8 to the missing-letter term.

Only argued:

- The theorem and Lemma B for all inputs (long queries, arbitrary bytes, larger dictionaries); the tests
  sample small cases.
- Termination, finality of the pop order and truncation, as in lower-bound.md section 5; exercised
  through the oracle tests, not checked step by step.
- Correctness for cost numbers other than `CostModel::qwerty()`: the proof uses only `c_indel_min`,
  `c_transpose_min`, `c_min` and non-negative step costs.

## 7. Cost of the mode and limits

- Work: deterministic counters against the exact search on the 10k, 100k and 274k dictionaries are in
  `docs/benchmarks/prefix-mode.md`. Latency was not measured.
- Memory: a queue entry has one extra `bool`; no new per-node data.
- Not done: reporting the matched prefix length; a different tolerance inside and after the typed part
  (all edits count against the same budget).
