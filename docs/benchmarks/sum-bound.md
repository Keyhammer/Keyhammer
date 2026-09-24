# A summed subtree-signature bound (issue #44), 2026-09-24

Verdict: **do not adopt.** Under the current `CostModel::qwerty()` the sum `T_comp + T_let` is admissible (argued in `docs/design/lower-bound.md` section 3a, and no counterexample in about 13.2 million tested cases), but it expands only 0.14% to 0.33% fewer trie nodes than `max` on the M0 data, a node reduction far too small to justify a cost-table precondition (issue #44 acceptance: "an unmeasurable gain"). The core is unchanged; `max` stays.

## Questions

1. Is `T_comp + T_let` admissible under the current costs (first-byte factor, doubled-letter discounts, transpositions)?
2. If so, how many fewer nodes does it expand, and is that worth changing the bound?

## 1. Admissibility

**Method.** `crates/keyhammer/tests/sum_bound.rs` re-implements the node bound of `lower_bound` with full DP matrices (no band storage, no trie), independent of `search.rs`, and compares, for every prefix node of a dictionary, the bound with the oracle cost of the cheapest within-budget term below it. It also checks the cell-level claim the proof uses: `T_comp(k) + T_let(k)` never exceeds the true cost of the rest of the path from that cell, for paths whose first step out of the cell is not a deletion (the last cell of a path in a row). The ignored `sweep` test ran, in release mode:

| Set | Cases | `max` violations | sum violations, node level | sum violations, cell level (tails not starting with a deletion) |
|---|---|---|---|---|
| alphabet `as` (adjacent keys): every dictionary of 1 to 3 terms of length 1 to 4, every query of length 0 to 5, 7 budgets | 1,995,525 | 0 | 0 | 0 |
| alphabet `ax` (non-adjacent keys): same | 1,995,525 | 0 | 0 | 0 |
| alphabet `asx`: terms of length 1 to 3, queries up to 4, up to 3 terms | 8,401,393 | 0 | 0 | 0 |
| alphabet `asd`: terms up to 3, queries up to 4, up to 2 terms | 660,660 | 0 | 0 | 0 |
| random (alphabets `aqw`, `asdfqwer`, `ax`, `aab`; 1 to 6 terms of length 1 to 8; queries of length 0 to 9 or an entry with 0 to 3 edits; seed 44) | 160,000 | 0 | 0 | 0 |

Budgets: 7, 15, 16, 24, 32, 48, 64 (`W = 0..8`). Cell level was checked for all cases of the four exhaustive sets and for one random round in eight. For tails that may start with a deletion, the cell-level claim does fail (24,486 random and millions of exhaustive cell violations), e.g. dictionary `{"a"}`, query `"aa"`, cell `(1, 1)`: `T_comp = T_let = 8` but the doubled-letter deletion costs 8. The node-level check is weak below the root: at depth >= 1 Case B caps the bound at min(row j-1) + 12, hiding most tail overestimates; the cell-level (last-in-row) check is what actually tests the proof. That is exactly the overlap the issue worried about; it does not reach the node bound, because a path's last cell in a row never starts with a deletion. `tests/sum_bound.rs` pins the example and runs a smaller exhaustive sweep in the normal test run.

With the sum patched into `lower_bound` (one line, not committed), `cargo test -p keyhammer --release` passes, including the 43,200-case lower-bound test per mode, `tests/oracle.rs` and `tests/fuzz_like.rs`.

**Proof.** Argued, not machine-checked, in `docs/design/lower-bound.md` section 3a. In short: with `sp`/`dp` the picked missing-class bytes consumed by substitutions/deletions, every step costs at least 8, an undiscounted deletion costs at least 16 (`indel >= indel_double + c_min`, equality in the current table), and the only discounted picked deletion can be at the first tail byte, where the tail must then start with an insertion that pays for it.

**Not valid for every cost table.** The proof needs `indel >= indel_double + c_min`. With `indel = 12`, `indel_double = 8` the sum would overestimate (dictionary {"b"}, query "bxy", root cell (0,0): T_comp = 16, T_let = 16, sum 32, but the path b=b, delete x, delete y would cost 12 + 12 = 24; with the real table it costs 32, so the sum is tight). `max` needs only the three minimum costs. `CostModel` has private fields; its constructors (`qwerty()`, `for_layout(..)`) all use the same scalar costs (only the neighbour table differs), so the sum is admissible for every layout, but not for arbitrary tables. A sum would need a checked constant. This is a second reason, besides the gain, not to change the default.

Limits: the exhaustive sets use 2 to 3 letters and dictionaries of up to 3 terms; the random set is 160,000 cases of larger alphabets. Nothing above covers queries over 10 bytes, large dictionaries, or bytes that collide modulo 64 (the proof handles collisions in argument only).

## 2. Nodes

**Method.** `bench/src/bin/sumbound.rs` (release) on the 300 Birkbeck typo pairs and the three M0 dictionaries (10,000, 100,000, 274,137 words; data not committed), `SearchConfig::default()` (`tsb = true`, `Ranking::Coarse`, k = 10) at budget 32 and 48, built against the unchanged core (`max`) and against a core with `extra = comp.saturating_add(missing)` in `lower_bound` (sum). It prints nodes and queue pushes per query and an FNV-1a fingerprint of all (id, cost) hits of all queries. Reproduce: `cargo run --release -p keyhammer-bench --bin sumbound -- bench/data`, once per variant.

The hit fingerprints are equal for `max` and the sum in all six rows (10,000 / 32: `425926cb9aca3e0d`, 10,000 / 48: `18cf10f92fb5368d`, 100,000 / 32: `69f1d24b151aec3f`, 100,000 / 48: `93aac51d45a05eb6`, 274,137 / 32: `7e3fffb35a1c6b30`, 274,137 / 48: `768edc9e9d55b974`), and no query was truncated.

| Words | Budget | nodes/query max | nodes/query sum | change | pushed/query max | pushed/query sum |
|---|---|---|---|---|---|---|
| 10,000 | 32 | 490.6 | 489.0 | -0.33% | 493.5 | 491.9 |
| 10,000 | 48 | 2551.4 | 2544.6 | -0.27% | 2851.2 | 2843.5 |
| 100,000 | 32 | 910.9 | 908.5 | -0.26% | 998.1 | 995.4 |
| 100,000 | 48 | 4609.8 | 4601.9 | -0.17% | 6308.2 | 6298.9 |
| 274,137 | 32 | 995.4 | 993.6 | -0.18% | 1181.6 | 1179.4 |
| 274,137 | 48 | 4590.2 | 4583.6 | -0.14% | 7118.1 | 7110.2 |

Node counts are deterministic, so machine load does not affect them.

**Latency was not measured.** A 0.14% to 0.33% node reduction cannot produce a p95 difference that this shared machine (other agents building and testing concurrently) could resolve, and the task made the latency run conditional on a clear node benefit. Any latency statement would be a hypothesis, and the expected sign is "no measurable change".

**Why so small (hypothesis, not tested).** The two terms are rarely both positive at the cells that decide a node's bound: the length gap is 0 in most of the tree, and a missing letter class is usually the only reason a subtree is pruned, so `max` and the sum coincide.

## Verdict

- Admissible under the current costs: yes (tested, and argued); not valid for every cost table.
- Gain: 0.14% to 0.33% fewer nodes, too small to justify a cost-table precondition; results identical.
- Adopt: **no.** `max` stays the bound; the sum adds a cost-table precondition for no measurable gain. Reopen only if a cost model with much larger insertion/deletion costs, where both terms are often positive, becomes a target.
