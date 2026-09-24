# Match highlighting (issue #32)

A caller that shows search results wants to mark the letters of each result that the user
actually typed. This note fixes what those "match ranges" are, how they are computed, in which
units they are reported, and what is guaranteed about them, before the code. It reuses the
definitions of `docs/design/lower-bound.md` (weighted OSA, `D[i][j]`, the four moves) and of
`docs/design/prefix-mode.md` (`pcost`); read those first. The mapping utilities it builds on
(`text::SourceMap`, the UTF-8 and UTF-16 offsets) come from `docs/design/unicode.md`.

## 1. Decisions in one list

1. Highlighting is a separate call made **after** the search, for the hits the caller wants to
   show (normally the final `k`). It re-runs the alignment of the query with one term, on the
   full (unbanded) matrix, and walks it back (a traceback). The search itself is not touched:
   no field, no flag, no extra work, identical node counts.
2. A match range is a half-open span `[start, end)` over the **original term string the caller
   inserted**, not over the normalised term the trie stores. Ranges are reported in three units
   at once: code points, UTF-8 bytes and UTF-16 code units (for JavaScript).
3. A term symbol is highlighted when the alignment matches it with an equal query symbol, or
   when it is one of a transposed pair. Substituted term symbols and term symbols the user
   skipped are not highlighted. Extra query symbols have no place in the term.
4. Among alignments of equal cost one is chosen by a fixed rule (section 4), so the ranges are
   deterministic.
5. The cost of the alignment that produced the ranges equals the hit's exact cost
   (`Hit::cost`); highlighting refuses a hit whose cost it cannot reproduce (section 5).
6. Prefix mode is supported: only the aligned prefix of the term can be highlighted (section 6).
7. The DP of the traceback is computed by the very function the search uses for its rows, not a
   copy of it (section 7).
8. Bindings are out of scope (section 10).

## 2. What a match range is

### 2.1 Symbols first

The search compares symbols: the code points of the normalised query and of the normalised term
(`docs/design/unicode.md`, section 3). Write `q` for the query symbols (`m = |q|`) and `t` for
the term symbols (`n = |t|`). An optimal alignment is a path of moves from `(0, 0)` to `(m, n)`
in `D` (to `(m, j)` for some `j` in prefix mode). Each move says what happened to the term
symbols it consumes:

| move | term symbols consumed | status of each | highlighted |
| --- | --- | --- | --- |
| substitution `D[i-1][j-1]`, `q[i-1] == t[j-1]` (cost 0) | `t[j-1]` | matched | yes |
| substitution `D[i-1][j-1]`, `q[i-1] != t[j-1]` | `t[j-1]` | substituted | no |
| transposition `D[i-2][j-2]` | `t[j-2]`, `t[j-1]` | transposed | yes, both |
| insertion `D[i][j-1]` (the user skipped `t[j-1]`) | `t[j-1]` | skipped | no |
| deletion `D[i-1][j]` (an extra query symbol) | none | - | - |

Why each row:

- **Substitutions**, including the cheap neighbouring-key ones, are not highlighted: the user
  typed a different letter. The cost model decides that the term is still close; the highlight
  shows which letters agree.
- **Transpositions** are highlighted: the user typed both letters, in the wrong order. Marking
  them as unmatched would show `recieve` against `receive` as `rec··ve` when every letter was
  typed; marking them keeps the highlight about "which letters of the result did I type". A
  caller that wants to show the swap differently needs the alignment itself, which is not
  exposed (section 9).
- **Doubled letters** change only costs, not statuses. The cheap insertion of a repeated term
  letter (`ins_cost` with `prev == t`, cost 8) leaves that letter unhighlighted; the cheap
  deletion of a repeated query letter consumes no term symbol. Which of two equal letters is the
  skipped one is decided by the costs: for the query `helo` and the term `hello`, skipping the
  *second* `l` costs 8 (it repeats the first) and skipping the first costs 16 (it follows `e`),
  so the optimal alignment skips the second one and the ranges are `hel` and `o`: `[0, 3)` and
  `[4, 5)`. For `helllo` against `hello` the extra `l` is a query deletion and all of `hello`
  is highlighted.
- **The first-symbol factor** (x1.5 on query position 0) is part of every cost above and so of
  the choice of the optimal alignment; it has no status of its own.

### 2.2 From normalised symbols to the caller's string

The trie stores the normalised term (`Trie::term`), but the caller shows its own spelling, for
example `Coração` where the trie has `coracao`. The caller passes that original string (for a
trie from `Trie::build_normalized`, the entry `items[trie.input_index(hit.id)]`; for a trie from
`Trie::build`, the term itself). It is normalised again with the trie's normaliser into a
`SourceMap`, which records for each normalised code point the source code point that produced
it, and must give back exactly `Trie::term(hit.id)`; otherwise highlighting returns an error
rather than ranges over the wrong text.

Normalisation is not one-to-one, so the status of a source character is derived from the status
of the symbols it produced:

- **One to one** (`Ç` -> `c`, `É` -> `e`, `A` -> `a`): the source character has the status of
  its symbol.
- **One to many** (`ß` -> `ss`, `æ` -> `ae`, `ﬁ` -> `fi`, `İ` -> `i` + U+0307 in case-only mode):
  the source character is highlighted when **at least one** of its symbols is. A character is
  the smallest unit a range can cover, so a partial match must round one way; rounding up keeps
  a typed letter visible (the query `strase` against `Straße` types one of the two `s` of `ß`,
  and `ß` is shown as typed). The alternative, "all symbols", would hide `ß` there although the
  user typed half of it. This is the policy for partially matched source characters.
- **One to none** (a combining mark U+0300 to U+036F dropped by diacritic folding): the mark has
  no symbol, so it takes the status of the source character before it. This is what
  `SourceMap::to_source` already does at the end of a range (a mark belongs to the letter it
  follows), so `cafe` + U+0301 highlights the mark with the `e`. A mark at the very start of the
  string has no letter before it and is never highlighted.

The highlighted source characters are then merged into maximal runs. The result is a list of
spans over the source string that is

- **sorted and disjoint**, with a gap of at least one character between consecutive spans (runs
  are maximal, so two spans never touch);
- **non-empty spans only**, each within `0..=len` in every unit;
- **on character boundaries**: every UTF-8 offset is a `char` boundary of the source and every
  UTF-16 offset lies between two code points (never inside a surrogate pair).

Each span carries three ranges: code points (`chars`), UTF-8 bytes (`utf8`, what Rust and C
index with) and UTF-16 code units (`utf16`, what JavaScript's `String.prototype.slice` takes).
An astral character such as `😀` is 1 code point, 4 bytes and 2 UTF-16 units. The three are
computed in one pass over the source; the tests check them against `text::utf8_range` and
`text::utf16_range`.

Besides the ranges the result reports `aligned`, the span of the source that the query was
aligned with: the whole term in whole-term mode, the matched prefix in prefix mode (section 6).
It is derived like the ranges: a source character is aligned when at least one of its symbols
is, and a mark that produced nothing follows the character before it. So a prefix cut inside
an expansion (the first `s` of `ß`) aligns the whole `ß` and the marks after it, every range lies
within `aligned`, and a mark at the start of the string is outside it. When nothing is aligned
(the empty prefix), `aligned` is empty and sits before the first character that produced a
symbol.

## 3. The traceback

`D` is computed on the full matrix `(m + 1) x (jmax + 1)` of the query against the term (term
prefixes up to `jmax`; see section 8 for the bound), by the recurrence of lower-bound.md section
1, then walked back from the end cell. At each cell the first move, in the order of section 4,
whose predecessor value plus its step cost equals the cell's value is taken, and the term
symbols it consumes get the status of the table in section 2.1. The walk ends at `(0, 0)`.

The matrix is not banded and not capped by the budget: every cell holds its exact value. The
banded search computes the same values for every cell within the budget (that is what the oracle
tests of the search check), so the end cell holds `Hit::cost`.

## 4. Tie-breaking

Several alignments can have the same optimal cost; for example the query `abab` against the term
`ab` can drop either of several pairs. The rule, applied at every cell of the walk back from the
end:

1. the diagonal move (a match, or a substitution);
2. the transposition;
3. the insertion (a skipped term symbol);
4. the deletion (an extra query symbol).

The first of these that attains the cell's value is taken. Because the walk goes from the end to
the start, preferring the diagonal move keeps the later symbols aligned with each other and
pushes insertions and deletions towards the start of the strings: among equal-cost alignments,
edits are placed leftmost and matches rightmost. A match (cost 0) is always a diagonal move, so a
match is never given up for an equal-cost insertion and deletion at the same cell.

In prefix mode the aligned prefix length `j` is the **shortest** one that attains `pcost` (the
smallest `j` with `D[m][j] = min over j' of D[m][j']`); among equal costs the shortest prefix
claims the least of the term.

The rule is a function of the query symbols, the term symbols and the cost model only, so the
ranges are deterministic and the same on every platform.

## 5. Consistency guarantee

The alignment walked back is an optimal path of `D`, so the sum of its step costs is
`D[m][n]` (whole-term mode) or `min over j of D[m][j]` (prefix mode). That is the exact cost the
search reports in `Hit::cost` for a hit of that query and term under the same cost model.
Highlighting checks it: if the recomputed cost differs from `hit.cost` (the hit came from
another query, another cost model, the other mode, or a query normalised differently) it returns
`HighlightError::CostMismatch` instead of ranges that would not explain the hit. So whenever
ranges are returned, the alignment that produced them costs exactly `Hit::cost`.

Tested (section 11): for random dictionaries and queries, ASCII and non-ASCII, the traceback
cost equals `Hit::cost` and the independent oracle cost of `tests/support`, and the sum of the
step costs of the walked path, recomputed from the cost model in the test, equals that cost too.

## 6. Prefix mode

For a hit of `Searcher::search_prefix` (or `search_prefix_text`) the caller asks for prefix
highlighting. `pcost = min over j of D[m][j]` (prefix-mode.md section 1), and the traceback
starts at `(m, j*)` with `j*` the shortest prefix length attaining it (section 4). Consequences:

- ranges cover only symbols of `t[..j*]`; the rest of the term, `t[j*..]`, is never
  highlighted: the user has not typed it yet;
- `aligned` is the source span of `t[..j*]` (for `jav` against `javascript`, `[0, 3)`);
- a transposition is inside the prefix (as in `pcost`: both swapped symbols in `t[..j*]`);
- the empty query aligns with the empty prefix at cost 0: no ranges, `aligned` empty.

A whole-term hit highlighted in prefix mode, or the reverse, normally fails the cost check of
section 5 (it does not when the two costs happen to be equal, and then the prefix alignment is a
correct explanation of that cost).

## 7. One cost function for the search and the traceback

A traceback that recomputed the four moves with its own code could drift from the search (a
changed doubled-letter rule, a factor applied at the wrong position), and the ranges would then
explain a cost the search never computed. So the move costs are factored out of the search into
one function, `search::each_move`, that computes the value of each available move into a cell
(substitution, transposition, insertion, deletion) from its predecessors and hands it to a
closure. The banded rows of the search (`root_row`, `child_row`) fold the minimum (`cell_min`);
the traceback collects the four values (`moves`, `INF` for a move that is not available), fills
its matrix with their minimum, and the walk back compares each value with the cell's. (A first
version returned the four values in an array to the search too; it made the search measurably
slower, `docs/benchmarks/highlighting.md`, and was replaced by the closure.) The cost model's functions (`sub_cost`, `ins_cost`,
`del_cost`, `transpose_cost`, with their layouts, first-symbol factor, doubled letters and
transposition condition) are called only there.

The refactor must not change the search. It computes the same four candidates and the same
minimum in each cell, so rows, bounds, the queue and every counter are the same; this is checked
by the existing oracle, regression (`tests/ascii_regression.rs`, pinned `Stats`) and fuzz tests,
and measured on the M0 data (node counts must be identical, section 11).

## 8. API and cost

Additive, in a new module `keyhammer::highlight` plus two methods on `Searcher`:

```rust
pub enum HighlightMode { Whole, Prefix }

pub struct MatchRange { pub chars: Range<usize>, pub utf8: Range<usize>, pub utf16: Range<usize> }

#[non_exhaustive]
pub struct Highlight {
    pub ranges: Vec<MatchRange>, // sorted, disjoint, non-empty
    pub aligned: MatchRange,     // the part of the source aligned with the query
    pub cost: Cost,              // equals hit.cost
    pub cells: usize,            // DP cells computed (the deterministic work counter)
}

impl Searcher {
    // The query as given to `search` / `search_prefix` (bytes, not normalised).
    pub fn highlight(&mut self, trie: &Trie, cm: &CostModel, q: &[u8], hit: &Hit,
                     source: &str, mode: HighlightMode) -> Result<Highlight, HighlightError>;
    // The query as given to `search_text` / `search_prefix_text` (normalised like the trie).
    pub fn highlight_text(&mut self, trie: &Trie, cm: &CostModel, q: &str, hit: &Hit,
                          source: &str, mode: HighlightMode) -> Result<Highlight, HighlightError>;
}
```

`HighlightError` (non-exhaustive): `QueryTooLong` (as for the search), `UnknownTerm` (the hit's
id is not a term of this trie), `SourceMismatch` (the source does not normalise to the term),
`CostMismatch` (section 5). The methods never panic, for any input.

Rejected alternatives:

- **A `Hit::ranges` field filled on request.** `Hit` is `Copy` and built with struct literals by
  the bindings and the benchmarks; a `Vec` field would break both, and the ranges need the
  caller's original spelling, which the search does not have.
- **A flag on `SearchConfig`.** `SearchConfig` is not `non_exhaustive` and is built with full
  literals by the bindings; a new field breaks them (the reason `search_prefix` is a method,
  prefix-mode.md section 1). A flag would also tempt highlighting every popped terminal instead
  of the final ones.

Cost per hit. Only term prefixes up to `jmax = m + hit.cost / c_indel_min` can be on an
alignment of cost `hit.cost` (a longer prefix needs more than that many insertions, each at least
`c_indel_min`), and `hit.cost` is at most the largest budget the search accepts (64, so
`jmax <= m + 8`). A hit above that budget cannot come from a search and is refused before any
work, and so is a whole-term hit whose term is longer than `jmax`. The matrix therefore has at
most `(m + 1) x (m + 9)` cells (18 369 at `MAX_QUERY_LEN = 128`); `Highlight::cells` reports the
exact number. Add one normalisation of the source (linear) and one pass over it for the offsets.
The matrix buffer lives in the `Searcher` and is reused. Measured per hit on the M0 data in
`docs/benchmarks/highlighting.md`.

## 9. Not exposed

- The alignment itself (which symbols were substituted, skipped or transposed, and which query
  symbols were extra). The ranges answer "which letters did I type"; a richer view (showing a
  substitution in another colour) would need a public alignment type, a possible follow-up.
- Ranges over the query (which query letters were used).

## 10. Bindings (follow-up)

Out of scope here. Each binding needs the call and a way to pass the original term string:
wasm and Node (UTF-16 ranges, since JavaScript indexes strings in UTF-16 units), Python (code
point ranges, since Python indexes `str` by code point), C (UTF-8 byte ranges, an ABI addition
under `docs/design/c-abi.md` rules). The WebAssembly size gate has little headroom, so the wasm
follow-up must report the size delta.

## 11. Tests

- **Property** (unit tests of the traceback and `tests/highlight.rs`): random small dictionaries
  and queries over ASCII and non-ASCII alphabets (`ç`, `ß`, `é`, other scripts, an emoji), all
  layouts, whole-term and prefix mode, plain and normalised tries: for every hit, highlighting
  succeeds, its cost equals `Hit::cost` and the oracle cost, the step costs of the path
  (recomputed in the test from the cost model) sum to it, and the ranges are sorted, disjoint,
  non-empty, within bounds and on character boundaries in all three units, and agree with
  `text::utf8_range` / `text::utf16_range`.
- **Golden examples**: `coracao` against `coração` (plain, with a typo, and with a case-only
  normaliser where `ç` and `ã` are substitutions), `javasript` against `javascript`, the
  transposition `recieve` against `receive`, `helo` against `hello` (doubled letter), prefix
  mode `jav` against `javascript`, and `strase` against `Straße` (partial `ß`).
- **UTF-16**: astral characters (emoji) before, inside and after highlighted spans.
- **Errors**: unknown id, wrong source, wrong cost, wrong mode, query too long.
- **Mutation tests**: deliberate bugs in the new code (tie order, transposed symbols unmatched,
  the prefix length rule, the any/all policy, marks not inheriting, UTF-16 counted as bytes, a
  cost function copied with a changed doubled-letter rule) must each make a test fail; the list
  and the outcome are recorded in `docs/benchmarks/highlighting.md`.
- **Fuzz**: a `highlight` property in `tests/fuzz_props` (arbitrary text, never panics; for
  every hit the cost equals `Hit::cost` and the ranges are well formed), as a cargo-fuzz target
  and in the deterministic `tests/fuzz_like.rs`.
- **Search unchanged**: node counts and hits on the M0 data identical to `main`.
