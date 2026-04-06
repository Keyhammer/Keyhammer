# keyhammer

Fuzzy string search that knows how humans make typos. 95-400x faster than FuseJS.

## The problem

Every app with a search bar needs to handle typos. User types "javasript", you need to find "javascript". The standard solution (FuseJS) scans every term on every query — O(n) per keystroke. At 5k terms it takes 12ms per query. At 10k it takes 26ms. On mobile, the UI stutters.

keyhammer builds an index that finds matches in microseconds, not milliseconds. And it scores results by how likely the query is a real typo — not just string distance.

## Benchmarks (vs FuseJS, same process, same data)

```
Terms    FuseJS/query    keyhammer/query    Speedup
──────────────────────────────────────────────────────
1,000    2.3ms           31µs               73x
5,000    12.3ms          129µs              95x
10,000   26.4ms          247µs              107x
```

Miss queries (no results found):
```
5,000    11.5ms          91µs               126x
10,000   23.0ms          57µs               406x
```

## What's under the hood

**CGL tree** — recursive partitioning structure from [arXiv:2604.01307](https://arxiv.org/abs/2604.01307) (Bibbens, Borevitz, McCauley, 2026). Sublinear query time for Hamming distance search.

**Confusion-aware pruning** — before descending into a tree branch, checks a confusion matrix (based on Grudin 1983 QWERTY data) to see if the mismatch is a plausible typo. Implausible branches get skipped entirely. No other fuzzy search structure does this.

**Character frequency fingerprints** — instead of pre-generating all deletion variants of every term (expensive), each term gets a 26-byte fingerprint. At query time, fingerprint comparison detects possible deletions/insertions in one cache line. Replaces the SymSpell-style variant generation approach.

**Columnar term storage** — terms stored column-wise (all first chars together, all second chars together) for cache-friendly brute force scans. The CPU compares the query against all terms at each position in one contiguous memory pass.

**Typo probability scorer** — ranks candidates by how likely the query is a typo of each term. Three signals: positional error weight (Wobbrock & Myers 2006), QWERTY confusion matrix (Grudin 1983), and transposition detection (Damerau 1964).

**Damerau-Levenshtein coverage** — handles all four edit operations (substitution, transposition, deletion, insertion) without changing the Hamming-based core. Substitution and transposition via Hamming scan, deletion and insertion via fingerprint matching + term map lookups.

## Usage

```rust
use keyhammer::FuzzyIndex;

let terms = vec!["javascript", "typescript", "python", "rust", "golang"];
let index = FuzzyIndex::build(&terms, 2).unwrap();

let results = index.search("javasript", 5).unwrap();
for r in &results {
    println!("{} (score: {:.2})", r.term, r.score);
}
```

## Node.js (via napi-rs)

```javascript
const { KeyhammerIndex } = require("keyhammer");

const index = KeyhammerIndex.build(["javascript", "typescript", "python"], 2);
const results = index.search("javasript", 5);
```

## Running

```bash
cargo test           # 45 tests
cargo run --example basic
cargo bench          # criterion benchmarks

# FuseJS comparison
cd bench-vs-fuse && npm install && node bench.mjs
```

## Based on

> Bibbens, Borevitz, McCauley. *Space-Efficient Text Indexing with Mismatches using Function Inversion*. arXiv:2604.01307, 2026.

## License

MIT
