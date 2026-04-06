# keyhammer

Fuzzy string search that knows how humans make typos.

Most fuzzy search libs (FuseJS, RapidFuzz) compare the query against every term in the dataset — O(n) per query. That works for 1k terms. At 100k it's slow. At 1M it's unusable.

keyhammer builds an index (CGL tree) that prunes 99% of terms before comparing, then ranks candidates by **typo probability** — not just string distance.

## What makes it different

- **Sublinear queries** — uses a CGL tree ([arXiv:2604.01307](https://arxiv.org/abs/2604.01307)) to skip most terms. Query time grows logarithmically, not linearly.
- **Typo-aware scoring** — errors in the middle of a word are common (fat-finger), errors at the start are rare. Transpositions ("teh" → "the") score higher than random substitutions ("txe" → "the"). No other fuzzy lib does this.
- **O(n) space** — the tree is truncated and uses Fiat-Naor function inversion to recover data on demand, keeping memory linear.
- **Zero config** — no distance metric to choose, no threshold to tune. Just pass your terms and search.

## Usage

```rust
use keyhammer::FuzzyIndex;

let terms = vec!["javascript", "typescript", "python", "rust", "golang"];
let index = FuzzyIndex::build(&terms, 2).unwrap(); // k=2 max mismatches

let results = index.search("javasript", 5).unwrap();

for r in &results {
    println!("{} (score: {:.2})", r.term, r.score);
}
```

## How it works

Two-phase pipeline:

1. **Phase 1 — CGL tree** (fast, coarse): partitions terms into a recursive tree using pivot strings and LCP. Retrieves candidates within Hamming distance k in sublinear time. The tree is truncated to O(n/σ) leaves; a Fiat-Naor inverter recovers term-to-leaf mappings without storing them all.

2. **Phase 2 — Typo scorer** (precise, cheap): scores each candidate by how likely the query is a typo of that term. Three signals:
   - **Position weight** — errors at word start cost more than errors in the middle
   - **Transposition detection** — adjacent char swaps are the most common typo
   - **Bigram context** — mismatches in common letter pairs (th, er, in) are more likely accidental

## Running

```bash
cargo test
cargo run --example basic
cargo bench
```

## Based on

> Bibbens, Borevitz, McCauley. *Space-Efficient Text Indexing with Mismatches using Function Inversion*. arXiv:2604.01307, 2026.

The CGL tree and Fiat-Naor integration come from this paper. The typo probability scorer is our addition — no existing work combines probabilistic error modeling with a sublinear indexed search.

## License

MIT
