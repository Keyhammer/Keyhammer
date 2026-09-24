# keyhammer

> **Status:** under active rewrite (Keyhammer 2). The numbers below describe the legacy engine.

Fuzzy string search that knows how humans make typos. 100x faster than FuseJS.

## The problem

Every app with a search bar needs to handle typos. The standard solution (FuseJS) scans every term on every query. At 10k terms it takes 36ms per query — on mobile, the UI stutters.

keyhammer finds matches in microseconds, not milliseconds. And it ranks results by how likely the query is a real typo.

## Benchmarks (vs FuseJS, same process, same data)

10,000 terms, single query:

| Typo type | FuseJS | keyhammer | Speedup | Both find? |
|-----------|--------|-----------|---------|------------|
| Substitution | 36.7ms | 345µs | **106x** | ✓✓ |
| Transposition | 35.8ms | 350µs | **102x** | ✓✓ |
| Deletion | 29.7ms | 285µs | **104x** | ✓✓ |
| Insertion | 38.1ms | 369µs | **103x** | ✓✓ |
| Adjacent key | 36.6ms | 348µs | **105x** | ✓✓ |
| Mixed case | 36.0ms | 347µs | **104x** | ✓✓ |
| Double error | 29.8ms | 296µs | **101x** | ✓✓ |
| Miss (no match) | 27.1ms | 187µs | **145x** | — |

Throughput: **3,988 queries/sec** vs FuseJS 39 q/s (**102x**).

## Features

Everything FuseJS has, plus more:

- **Fuzzy search** with substitution, transposition, deletion, insertion handling
- **Case insensitive** by default
- **Typo-aware scoring** — adjacent key errors score higher than random substitutions
- **Match highlighting** — character ranges for UI highlighting
- **Multi-field search** with weighted keys (`DocumentIndex`)
- **Extended search** — `=exact`, `^prefix`, `suffix$`, `!exclude` operators
- **Logical queries** — AND (space) and OR (`|`) operators
- **Dynamic add/remove** — update index without rebuild
- **Threshold filtering** — minimum score cutoff
- **Diacritics handling** — café → cafe, naïve → naive
- **Keyboard layouts** — QWERTY (default), AZERTY, QWERTZ
- **Export/import** — serialize and reconstruct the index
- **Custom sort** — user-provided comparator
- **Field-length normalization** — shorter terms score slightly higher

Unique to keyhammer (no other lib has these):

- **Confusion-aware tree pruning** — skips tree branches where the mismatch is an implausible typo
- **Character frequency fingerprints** — detects deletions/insertions without generating variants
- **Typo-optimal encoding** — character codes where XOR bit distance = typo probability
- **Columnar term storage** — cache-friendly layout for brute force scans
- **Lazy CGL tree** — demand-driven construction, only builds what queries need

## Usage (Rust)

```rust
use keyhammer::FuzzyIndex;

let terms = vec!["JavaScript", "TypeScript", "Python", "Rust"];
let index = FuzzyIndex::build(&terms, 2).unwrap();

let results = index.search("javasript", 5).unwrap();
assert_eq!(results[0].term, "JavaScript");
// results[0].match_ranges → [(0,4), (5,10)] for highlighting
```

### Multi-field search

```rust
use keyhammer::DocumentIndex;

let docs = vec![
    vec![("name", "React"), ("category", "framework")],
    vec![("name", "Vue"), ("category", "framework")],
    vec![("name", "Rust"), ("category", "language")],
];

let index = DocumentIndex::builder()
    .key("name", 2.0)       // name weighs 2x
    .key("category", 1.0)
    .k(2)
    .build(&docs)
    .unwrap();

let results = index.search("react", 5).unwrap();
```

### Dynamic updates

```rust
let mut index = FuzzyIndex::build(&["hello", "world"], 2).unwrap();
index.add("rust");
index.remove(1); // remove "world"
```

## Usage (Node.js via napi-rs)

```javascript
const { KeyhammerIndex } = require("keyhammer");

const index = KeyhammerIndex.build(["JavaScript", "TypeScript", "Python"], 2);
const results = index.search("javasript", 5);
// results[0] = { term: "JavaScript", score: 0.48, matchRanges: [[0,4],[5,10]] }

// with layout
const fr = KeyhammerIndex.buildWithLayout(terms, 2, "azerty");

// dynamic
index.add("Rust");
index.remove(1);

// threshold
const strict = index.searchWithThreshold("javasript", 5, 0.8);

// export/import
const terms = index.export();
const restored = KeyhammerIndex.import(terms, 2);
```

## Running

```bash
cargo test                          # 115 Rust tests
cargo bench                         # criterion benchmarks
cd crates/node && node test.mjs     # 33 Node.js tests
cd bench-vs-fuse && node bench.mjs  # FuseJS comparison
```

## Architecture

Two-phase search:

1. **Candidate retrieval** — columnar Hamming scan + fingerprint matching for deletions/insertions. For large datasets, a lazy CGL tree ([arXiv:2604.01307](https://arxiv.org/abs/2604.01307)) supplements with sublinear Hamming search.

2. **Ranking** — typo probability scorer combining positional error weight (Wobbrock & Myers 2006), QWERTY confusion matrix (Grudin 1983), transposition detection (Damerau 1964), and bit-level encoding distance.

## Author

Robson Trasel ([@RobsonTrasel](https://github.com/RobsonTrasel))

## License

AGPL-3.0-or-later. See [LICENSE](LICENSE). Earlier commits, published before the
relicense commit ("chore: relicense to AGPL-3.0-or-later"), were released under MIT.
