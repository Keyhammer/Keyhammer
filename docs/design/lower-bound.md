# The lower bound of the trie search is admissible

This note proves that the bound `lower_bound` in
`crates/keyhammer/src/search.rs` never exceeds the cost of a term that the
search must find, and that the best-first search built on it returns the exact
top-k. It also states which parts are checked by tests and which are only
argued here.

Every ingredient of the bound is already published (row minimum over a DP
column, per-subtree length ranges, character-presence signatures); see
`docs/papers.md` for the references. Nothing below is claimed to be new.

## 1. Definitions

### Costs

All costs are integers of type `Cost` (`u16`, fixed point, 16 = one ordinary
edit). `CostModel` (in `cost.rs`) defines four steps. Let `q` be the query,
`m = |q|`, `t` a term, `n = |t|`, and let `at_start(x, p) = x + x/2` when the
query position `p` is 0 and `x` otherwise (so `at_start(x, p) >= x`).

| step | cost | notes |
| --- | --- | --- |
| substitution of query byte `q[p]` by term byte `b` | `sub_cost(q[p], b, p)` | 0 if `q[p] == b`, else `at_start(16 or 8, p)` (8 for neighbouring keys) |
| insertion of term byte `b` (the user skipped it), `p` query bytes consumed | `ins_cost(b, prev, p)` | `at_start(8, p)` if `b` equals the previous term byte, else `at_start(16, p)` |
| deletion of query byte `q[p]` (the user typed an extra byte) | `del_cost(q, p)` | `at_start(8, p)` if `p > 0` and `q[p] == q[p-1]`, else `at_start(16, p)` |
| transposition of `q[p] q[p+1]` | `transpose_cost(p)` | `at_start(12, p)` |

Every step costs at least 0, and only a substitution of equal bytes costs 0.
The constants used by the bound are the smallest possible costs:

- `c_indel_min = min(indel, indel_double) = 8`: every insertion and every
  deletion costs at least this;
- `c_transpose_min = transpose = 12`: every transposition costs at least this;
- `c_min = min(sub, sub_adjacent, indel, indel_double, transpose) = 8`: every
  step that is not a zero-cost substitution costs at least this.

### Weighted OSA

`D[i][j]` is the smallest cost of an alignment of the query prefix `q[..i]`
with the term prefix `t[..j]`: `D[0][0] = 0` and, for `(i, j) != (0, 0)`,
`D[i][j]` is the minimum of

- `D[i-1][j-1] + sub_cost(q[i-1], t[j-1], i-1)` (if `i, j >= 1`),
- `D[i][j-1] + ins_cost(t[j-1], t[j-2] if j >= 2, i)` (if `j >= 1`),
- `D[i-1][j] + del_cost(q, i-1)` (if `i >= 1`),
- `D[i-2][j-2] + transpose_cost(i-2)` if `i, j >= 2`, `q[i-1] == t[j-2]`,
  `q[i-2] == t[j-1]` and `q[i-1] != q[i-2]`.

The cost of `t` is `cost(t) = D[m][n]`. This is the restricted
(optimal string alignment) form of the Damerau distance: a transposed pair is
not edited again. The independent oracles `oracle_cost` in
`tests/support/mod.rs` and `oracle` in the unit tests of `search.rs` compute
exactly this recurrence on the full matrix.

An alignment is a path of cells from `(0, 0)` to `(m, n)` where each step is
one of the four moves above, with the cost of that move. `D[i][j]` is the
minimum cost of a path to `(i, j)`. Because every step cost is non-negative,
every cell on a path of total cost `c` is reached with a prefix cost of at most
`c`.

Row `j` of the matrix depends only on `q` and on `t[..j]`: every step into row
`j` reads at most `t[j-1]` and `t[j-2]`. So all terms that share the prefix
`t[..j]`, i.e. all terms at or below the trie node `v` at depth `j` that spells
it, share rows `0..=j`. We write `D_v[i]` for row `j` of node `v`.

### The band

The budget is `B` (`SearchConfig::budget`, at most `8 * c_indel_min = 64`,
otherwise `search` returns `BudgetTooLarge`), and the band half-width is
`W = floor(B / c_indel_min)`, so `W <= 8`.

**Band lemma.** Every path from `(0, 0)` to `(i, j)` costs at least
`|i - j| * c_indel_min`. So `D[i][j] <= B` implies `|i - j| <= W`.

*Proof.* Substitutions and transpositions change `i` and `j` by the same
amount; only insertions and deletions change `i - j`, each by one. So a path to
`(i, j)` has at least `|i - j|` insertions or deletions, each costing at least
`c_indel_min`. If `|i - j| >= W + 1`, the cost is at least
`(W + 1) * c_indel_min > B` by the definition of `W`. ∎

### Banded rows

The search stores, for node `v` at depth `j`, the array `R_v[0..=2W]` where
cell `k` stands for the query prefix length `i = j + k - W` (so
`k = i - j + W`). Cells with `i < 0` or `i > m` are never written and stay
`INF`. `root_row` fills row 0 with the cumulated deletion costs.

`child_row` is called while expanding `v` (at depth `j`, the `depth` argument)
and fills the row of a child `c` of `v`, which is row `j + 1`. Its inputs are
`v`'s row `cur` (row `j`) and the row `prev` of `v`'s parent (row `j - 1`,
used only when `j >= 1`). On the path to `c`, the term bytes are
`t[j - 1] = label(v)` (when `j >= 1`) and `t[j] = label(c)`. In the output
row, cell `k` stands for the query length `i = (j + 1) + k - W`. The index
arithmetic then matches the recurrence at cell `(i, j + 1)`:

- substitution reads `cur[k]`: row `j`, query length `j + k - W = i - 1`,
  cost `sub_cost(q[i-1], t[j], i-1)`;
- insertion reads `cur[k + 1]`: row `j`, query length `i`, cost
  `ins_cost(t[j], t[j-1], i)`, with `t[j-1] = label(v)` passed as the previous
  term byte when `j >= 1` and no previous byte when `j = 0`;
- deletion reads `out[k - 1]`: row `j + 1` itself, query length `i - 1`, cost
  `del_cost(q, i-1)`;
- transposition reads `prev[k]`: row `j - 1`, query length
  `(j - 1) + k - W = i - 2`, when `j >= 1`, `i >= 2`, `q[i-1] == t[j-1]`
  (`label(v)`), `q[i-2] == t[j]` (`label(c)`) and `q[i-1] != q[i-2]`, cost
  `transpose_cost(i-2)`. These are the conditions of the recurrence for row
  `j + 1`, whose two last term bytes are `t[j-1]` and `t[j]`.

Every computed value `x` is replaced by `cap(x) = x if x <= B, else INF`, with
`INF = 30000 > B`, and a predecessor cell equal to `INF` is ignored.

**Row lemma.** For every node `v` at depth `j` and every in-band `i`
(`0 <= i <= m`, `|i - j| <= W`, cell `k = i - j + W`):
`R_v[k] = D_v[i]` if `D_v[i] <= B`, and `R_v[k] = INF` otherwise.

*Proof.* By induction in the order the cells are computed (row by row, and by
increasing `k` inside a row).

1. A finite `R_v[k]` is the cost of an actual path to `(i, j)`: it is the sum
   of a finite predecessor cell (by induction, the cost of a path) and the cost
   of one legal step. So `R_v[k] >= D_v[i]` whenever `R_v[k]` is finite.
2. If `D_v[i] > B`, a finite `R_v[k]` would satisfy `R_v[k] >= D_v[i] > B`
   and would have been capped; so `R_v[k] = INF`.
3. If `D_v[i] <= B`, take an optimal path to `(i, j)` and its last step from a
   cell `(i', j')`. The prefix cost `D[i'][j'] <= D_v[i] <= B`, so `(i', j')`
   is in the band (band lemma) and, by induction, its stored cell equals
   `D[i'][j']`, which is finite. The recurrence therefore considers the
   candidate `D[i'][j'] + step = D_v[i] <= B`, so `R_v[k] <= D_v[i]`, and with
   (1) `R_v[k] = D_v[i]`, not capped. ∎

Consequences:

- Cells outside the band (never stored) and capped cells never lie on a path of
  cost at most `B`, so dropping them loses nothing for within-budget terms.
- **Terminal exactness.** For a term `t` ending at `v` (`n = j`), the cell
  `k = m + W - j` holds `cost(t)` if `cost(t) <= B`. If `cost(t) > B`, either
  the cell is out of the band or it is `INF`. `Expander::terminal` (and hence
  `search`) reports the term exactly when `0 <= k <= 2W` and `R_v[k] <= B`,
  with cost `R_v[k]`. So a term is reported if and only if it is within the
  budget, and always with its exact cost.

## 2. The bound

Fix a node `v` at depth `j`, with row `R = R_v` and, if `j >= 1`, the parent's
row `P` (row `j - 1`). `lower_bound` returns

```
LB(v) = min( min over finite R[k] of  R[k] + extra(k),
             min over finite P[k] of  P[k] + c_transpose_min   (only if j >= 1) )
```

with `extra(k) = 0` when `tsb` is off, and
`extra(k) = max(T_comp(k), T_let(k))` when it is on (section 3). If no cell is
finite, `LB(v) = INF > B`.

**Theorem (admissibility).** Let `t` be a term at or below `v` with
`cost(t) <= B`. Then `LB(v) <= cost(t)`, with `tsb` on or off.

*Proof.* Take an optimal alignment of `t`, of cost `c = cost(t) <= B`. The row
index `j'` of its cells goes from 0 to `n >= j` and grows by 0 (deletion), 1
(substitution, insertion) or 2 (transposition) per step. So exactly one of the
following holds.

**Case A: the path crosses row `j`.** It visits some cell `(i, j)`. Choose the
last such cell. The prefix cost to `(i, j)`, call it `p`, satisfies
`D_v[i] <= p <= c <= B`, so the cell is in the band and `R[k] = D_v[i]`
(row lemma), finite. Write `S` for the cost of the rest of the path, from
`(i, j)` to `(m, n)`; then `c = p + S >= D_v[i] + S`. If `extra(k) <= S` then
`LB(v) <= R[k] + extra(k) <= D_v[i] + S <= c`. With `tsb` off, `extra = 0 <= S`
trivially. With `tsb` on, `extra(k) <= S` is shown in section 3.

**Case B: the path skips row `j`.** It never visits row `j`, so it jumps from
row `j - 1` to row `j + 1` by a transposition from some cell `(i - 2, j - 1)`
to `(i, j + 1)`. This needs `j >= 1` (row 0 always contains the start cell),
which is why `lower_bound` only uses `P` when `depth >= 1`. The prefix cost to
`(i - 2, j - 1)` is at most `c <= B`, so by the row lemma the parent's cell for
query length `i - 2` is finite and at most that prefix cost. The transposition
costs at least `c_transpose_min`. So
`LB(v) <= P[k'] + c_transpose_min <= c`. No signature term is added in this
case, so it holds with `tsb` on or off. ∎

The skip term is deliberately simple: it takes the smallest finite cell of `P`
plus the cheapest transposition and adds no signature term, because the
skipping transposition also consumes `t[j-1] = label(v)`, which
`below_mask(v)` does not cover, so the argument of section 3 would have to be
adapted to the cell `(i, j + 1)`. Omitting a non-negative term
can only lower the bound, so this is conservative, not wrong.

Only terms within the budget are covered. For a term with `cost(t) > B` the
theorem claims nothing, and none is needed: such a term is never reported.

**Saturation.** `extra` is computed with saturating arithmetic
(`gap.min(INF)`, `saturating_mul`, `saturating_add`). A saturated value is at
most the exact one, so saturation can only lower `LB(v)`, never raise it.

## 3. The subtree-signature terms

**Symbols (issue #19).** The search alphabet is now Unicode scalar values: in this note
"byte" reads "symbol" (a code point of the term or the query, or `0x110000 + b` for a byte
`b` of the query that is not valid UTF-8), and `class` is `cost::symbol_class`, which is
`1 << (s & 63)` for ASCII, as below, and one of the 38 classes that `a`-`z` never use for
every other symbol. Nothing below depends on more than "equal symbols have equal classes"
and on depths and query positions counting the same units, so every argument carries over
word for word; see `docs/design/unicode.md`, section 4.

`trie.rs` stores for each node `v`:

- `len_min(v)` and `len_max(v)`: the shortest and longest term length at or
  below `v`;
- `below_mask(v)`: the OR of `class(b)` over the labels `b` of all edges
  strictly below `v` (the edges into its children and deeper), where
  `class(b) = 1 << (b & 63)`.

The query mask is `qmask[i]` = OR of `class(q[p])` for `i <= p < m`
(`qmask[m] = 0`).

For a finite cell `k` of `R`, with `i = j + k - W` (so `0 <= i <= m`, because
unused cells stay `INF`) and `r = m - i` query bytes left:

- `lo = len_min(v) - j`, `hi = len_max(v) - j`,
  `gap = lo - r` if `r < lo`, `r - hi` if `r > hi`, else 0, and
  `T_comp(k) = gap * c_indel_min`;
- `T_let(k) = popcount(qmask[i] & !below_mask(v)) * c_min`.

Continue Case A: the rest of the path aligns `q[i..m]` (`r` bytes) with
`t[j..n]` (`s = n - j` bytes), starting at `(i, j)`, with cost `S`.

**`T_comp <= S`.** `t` is at or below `v`, so `len_min(v) <= n <= len_max(v)`
and `lo <= s <= hi`, hence `|r - s| >= gap`. By the argument of the band lemma
applied to the rest of the path, it contains at least `|r - s|` insertions or
deletions, each costing at least `c_indel_min`. So
`S >= |r - s| * c_indel_min >= T_comp(k)`.

**`T_let <= S`.** The bytes `t[j..n]` are the labels of the edges from `v` down
to the node of `t`, all strictly below `v`, so their classes are in
`below_mask(v)`. Let `C` be the set of classes in `qmask[i] & !below_mask(v)`.
For each class in `C` pick one query position `p >= i` whose byte has that
class; distinct classes give distinct positions. The rest of the path consumes
each query byte `q[p]`, `p >= i`, in exactly one step: a substitution, a
deletion, or a transposition (which consumes two query bytes). For a picked
position, `class(q[p])` is not the class of any byte of `t[j..n]`, and equal
bytes have equal classes, so `q[p]` equals no byte of `t[j..n]`. Hence:

- it is not consumed by a transposition (both query bytes of a transposition
  equal term bytes of `t[j..n]`: the step starts at a cell `(i', j')` with
  `i' >= i` and `j' >= j`);
- if it is consumed by a substitution, the substitution is between different
  bytes and costs at least `c_min`;
- if it is consumed by a deletion, that costs at least `c_min`.

Each substitution or deletion consumes one query byte, so the picked positions
use `|C|` distinct steps, each costing at least `c_min`:
`S >= |C| * c_min = T_let(k)`.

**Why `max` is valid.** `T_comp(k) <= S` and `T_let(k) <= S` are two separate
lower bounds on the same quantity, so `max(T_comp(k), T_let(k)) <= S`, which is
what Case A needs.

**Why `max` and not a sum.** `max` is used because the argument above makes
it valid for any cost model with the stated minimum costs, and it needs no
reasoning about whether one edit can be charged by both terms (a deletion of a
query byte whose class is missing, for example, shortens the length gap and
removes a missing class at once). Section 3a shows that under the current
`CostModel::qwerty()` a sum `T_comp + T_let` is also admissible, that this
needs one more property of the cost table, and that it prunes almost nothing
more (`docs/benchmarks/sum-bound.md`); `max` stays.

### 3a. A sum of the two terms (issue #44)

Result: under the current `CostModel::qwerty()`, `T_comp(k) + T_let(k) <= S`
also holds, so the sum is admissible. It is not adopted
(`docs/benchmarks/sum-bound.md`: 0.14% to 0.33% fewer nodes). What is argued
and what is tested is stated separately below.

**Extra property of the cost table.** Besides the three minimum costs of
section 1, the proof needs that a deletion that is not discounted costs at
least `c_indel_min + c_min`, i.e. `indel >= indel_double + c_min`. For the
current table `16 >= 8 + 8` holds with equality, so the sum is tight there. A
table with, say, `indel = 12` and `indel_double = 8` would break it
(dictionary `{"b"}`, query `"bxy"`, root cell `(0, 0)`: `T_comp = 16`,
`T_let = 16` (x, y), sum 32, but the path b=b, delete x, delete y would cost
12 + 12 = 24), while `max` would stay valid. With the current table that path
costs 16 + 16 = 32, equal to the sum (tight). So a sum is not valid for every
cost table and would have to be tied to a checked constant. `CostModel` has
private fields; its constructors (`qwerty()`, `for_layout(..)`) all use the same
scalar costs (only the neighbour table differs), so the sum is admissible for
every layout, but not for arbitrary tables.

**Claim.** Let `(i, j)` be the last cell that the optimal path of `t` visits in
row `j` (Case A). Let `S` be the cost of the rest of the path, with `a`
substitutions, `d` deletions, `e` insertions and `tau` transpositions, so that
`r = a + d + 2 tau` and `s = a + e + 2 tau`, hence `d - e = r - s`. Let `G` be
the gap and `C` the set of missing classes of section 3. Then
`S >= c_indel_min * G + c_min * |C|`.

*Proof (argued, not machine-checked).* Because `(i, j)` is the last cell of the
path in row `j`, the first step out of it is not a deletion (a deletion stays
in row `j`).

*Picks.* For each class of `C` pick the lowest position `p >= i` whose query
byte has that class. As in section 3, each picked byte is consumed by a
substitution (of different bytes, cost `>= c_min`) or by a deletion, never by a
transposition. Let `sp` and `dp` be the numbers of picked substitutions and
deletions; `sp + dp = |C|`.

*Discounts.* A deletion at `p` costs `>= 2 c_min = 16` unless `q[p] = q[p-1]`.
If a picked `p` has `q[p] = q[p-1]`, then `p - 1` has the same class, so by the
choice of the lowest position `p - 1 < i`, i.e. `p = i`. So at most one picked
deletion (the one at `p = i`) is discounted. If it is, `q[i]` is the first
query byte the tail consumes, yet the first step is not a deletion, so the
first step is an insertion and `e >= 1`.

*Gap.* If `G > 0` because `r < len_min - j`, then `G <= s - r = e - d <= e`;
the `e` insertions cost `>= c_indel_min` each and the picks are other steps, so
`S >= c_indel_min e + c_min |C| >= c_indel_min G + c_min |C|`. If `G > 0`
because `r > len_max - j`, then `G <= r - s = d - e`. `S` is at least the sum
of: the `sp` picked substitutions (`>= 8` each), the `d - dp` other deletions
(`>= 8` each), the `e` insertions (`>= 8` each) and the `dp` picked deletions
(`>= 16` each, or `8` for the one discounted deletion at `p = i`). Then
`S - 8 (|C| + d - e) >= 16 e - 8 [discounted]`, which is `>= 0` because a
discount implies `e >= 1`; with `G <= d - e` this is the claim. If `G = 0` the
claim is `T_let <= S` of section 3. The first-byte factor `x1.5` only raises
the costs used above, so it does not matter. ∎

Case B (transposition skip) adds no signature term, so nothing changes there.
Hence `LB_sum(v) <= cost(t)` for every within-budget `t` below `v`.

**The last-cell condition is needed.** For a tail that starts with a deletion
the claim is false: dictionary `{"a"}`, query `"aa"`, cell `(i, j) = (1, 1)`
has `T_comp = T_let = 8`, but the doubled deletion costs 8. The node bound is
not violated because the last cell of the path in row 1 is `(2, 1)`, where
nothing is missing. `tests/sum_bound.rs` pins this. The node-level check is weak
below the root: at depth >= 1 Case B caps the bound at `min(row j-1) + 12`,
hiding most tail overestimates; the cell-level (last-in-row) check is what
actually tests the proof.

**Tested.** `tests/sum_bound.rs` re-implements the bound with full matrices
(independent of the banded storage) and checks, for both `max` and the sum,
that the bound of every prefix node never exceeds the oracle cost of the
cheapest within-budget term below it, and, per cell, that `T_comp + T_let`
never exceeds the true cost of a tail that does not start with a deletion.
The default test runs a small exhaustive sweep. The ignored `sweep` test ran
13,053,103 exhaustive cases (all dictionaries of up to 3 terms over the
alphabets `as`, `ax` and `asx`, up to 2 over `asd`; all queries up to 4 to 5
bytes; budgets 7, 15, 16, 24, 32, 48 and 64) and 160,000 random ones: no
violation of either bound at node level, and none at cell level for tails that
do not start with a deletion (for unrestricted tails there are violations, as
above). With the sum patched into `lower_bound` (not committed), the unit test
of 43,200 cases per mode, `tests/oracle.rs`, `tests/fuzz_like.rs` and the rest
of `cargo test -p keyhammer` pass.

**Class collisions only loosen the bound.** Classes are `b & 63`, so distinct
bytes can share a class (for example `!` (33) and `a` (97)). The lowercase
letters `a..=z` map to 33..=58 and never collide with each other. The proof of
`T_let <= S` uses only "equal bytes have equal classes", which holds for any
class function. A collision can only put a class into `below_mask(v)` that the
real suffix bytes do not have, or make two query bytes count as one class; both
shrink `C`, and so lower `T_let`. The unit tests include an alphabet with
colliding bytes (`a`, `b`, `!`, `"`).

## 4. Capping to `INF`

`lower_bound` skips cells equal to `INF`. By the row lemma, a cell is `INF` or
missing only if every path to it costs more than `B`. In both cases of the
theorem, the cell used (`(i, j)` in Case A, `(i - 2, j - 1)` in Case B) has a
prefix cost of at most `cost(t) <= B`, so it is finite. Capping and skipping
therefore never remove the cell that the proof needs for a within-budget term.

## 5. The queue key and the best-first order

**Key.** `pack(rank, weight, id)` builds the `u64`
`rank << 48 | (65535 - weight) << 32 | id`. The three fields occupy disjoint
bits (`pack` takes `rank` and `weight` as `u16` and `id` as `u32`; moreover a
node or terminal is only pushed when its bound or cost is at most `B`, so the
ranks in the queue are small), so the integer
order is the lexicographic order on `(rank, 65535 - weight, id)`. A terminal
for term `t` gets `key(t) = pack(rank(cost(t)), weight(t), t)`. A node `v` gets
`pack(rank(LB(v)), max_weight(v), 0)`. `Entry` reverses the order, so the
`BinaryHeap` pops the smallest `(key, seq)` first.

**Rank functions are monotone.** `Ranking::Exact` uses `rank(c) = c`;
`Ranking::Coarse` uses `rank(c) = whole_units(c) = ceil(c / 16)`. Both are
non-decreasing.

**Key lemma.** If `t` is at or below `v` with `cost(t) <= B`, then
`key(v) <= key(t)`.

*Proof.* `LB(v) <= cost(t)` (theorem), so `rank(LB(v)) <= rank(cost(t))` by
monotonicity. If the ranks are equal, `max_weight(v) >= weight(t)` gives
`65535 - max_weight(v) <= 65535 - weight(t)`; if those are equal too,
`0 <= t`. ∎

Note that the key lemma needs only monotonicity, not strict monotonicity:
`Coarse` maps many costs to one rank, and the tie is then broken by weight and
id exactly as in the terminal keys.

**Pushes.** The root is pushed if `LB(root) <= B`. Expanding a node `v`
pushes the terminal of the term ending at `v` if it is within budget (terminal
exactness), and each child `c` with `LB(c) <= B`. By the theorem, a child that
has a within-budget term at or below it satisfies `LB(c) <= cost <= B` and is
pushed; the same holds for the root. Every node has one parent and is pushed
only when its parent is expanded, and the root only once, so each node and
each terminal is pushed at most once. The trie is finite, so the loop ends.

**Invariant.** Before each pop, every within-budget term not yet reported has
an entry in the queue whose key is at most `key(t)`: its terminal entry, or the
entry of a node on its path. Initially the root entry (key lemma). When a node
`v` is popped and expanded, the terms at or below it are covered by the pushed
terminal (with key exactly `key(t)`) or by the pushed child on their path (key
lemma). Popping a terminal removes only that term's representative.

**The first terminal popped is final.** When the terminal of `t` is popped,
every other within-budget term `t'` not yet reported has a representative with
key at most `key(t')`, and that representative is still in the queue, so
`key(t) <= key(t')`. Keys of different terms differ (the id field), so
`key(t) < key(t')`. Hence terms are reported in strictly increasing key order,
each exactly once, and the first `k` reported are the `k` smallest keys among
the within-budget terms: the oracle's order. Ties between a node and a terminal
with equal keys are harmless, since the argument only uses `<=`; the sequence
number `seq` only makes the pop order deterministic. If the node limit stops
the search early (`Stats::truncated`), the hits already reported are still the
first hits of the exact order, but the list may be shorter than `k`.

## 6. What is tested and what is only argued

Tested (`crates/keyhammer/src/search.rs`, module `tests`, run by
`cargo test -p keyhammer`):

- For `tsb` off and on: 600 random dictionaries (150 per alphabet, 1 to 24
  entries of length 1 to 9, with duplicates, prefix chains and extensions) over
  the alphabets `aqw`,
  `asdfqwer`, `a..=z` and the colliding `ab!"`; 8 queries each (a quarter fully
  random of length 0 to 10, the rest a dictionary entry with 0 to 3 random
  edits); budgets 7, 15, 16, 24, 31, 32, 40, 48 and 64 (`W = 0, 1, 2, 3, 3, 4,
  5, 6, 8`, including budgets that are not multiples of 16). That is 43,200
  cases per mode.
- In each case, every trie node, not only those the search reaches, gets its
  row and bound through `Expander`, the same code path that `search` uses. For
  every node with a within-budget term at or below it, the test asserts
  `LB(v) <= min cost` (against an independent full-matrix oracle) and
  `key(v) <= key(t)` for both rankings. At every terminal it asserts terminal
  exactness: the reported cost equals the oracle cost when within budget, and
  nothing is reported otherwise. Per mode this checks 1,397,664 nodes
  (258,928 of them with a within-budget term below) and 392,544 terminals
  (75,015 within budget).
- These mutations make the test fail: adding 1 to the signature term;
  dropping the transposition-skip term; building `below_mask` from the node's
  own label instead of its children's labels (an inadmissible variant: the
  children's classes can go missing); removing a class from `below_mask`.
- Adding the node's own label to `below_mask` on top of the correct mask does
  not make the property test fail, and by construction it cannot: a larger
  class set shrinks the set of missing classes, so `T_let` drops and `LB(v)`
  drops, and an admissibility test only fails when a bound rises. That variant
  is a loosening, not an error, and it is caught by the structural tests
  `a_terminal_node_counts_its_own_length` and
  `aggregates_cover_the_whole_subtree` in `tests/trie.rs`, which assert exact
  values of `below_mask`.
- End to end, `tests/oracle.rs` compares the hits of `search` with a
  brute-force top-k for both rankings and both `tsb` modes.

Only argued here:

- The theorem for all inputs: queries up to 128 bytes, any bytes, larger
  dictionaries, and the whole band range. The tests sample small cases only.
- The best-first argument of section 5 (invariant, finality of the first
  terminal popped, termination, truncation). It is exercised indirectly by
  `tests/oracle.rs`, not checked step by step.
- The proof in section 3a that a sum of the two signature terms is admissible
  under the current costs (the sweeps there are exhaustive only for tiny
  alphabets and dictionaries); the implementation uses `max`.
- That the theorem holds for other cost numbers: the proof uses only the
  minimum step costs `c_indel_min`, `c_transpose_min` and `c_min`, but only the
  current `CostModel::qwerty()` is tested.
