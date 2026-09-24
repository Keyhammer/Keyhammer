<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/logo/keyhammer-icon-dark.svg">
    <img src="assets/logo/keyhammer-icon.svg" alt="Keyhammer logo" width="120">
  </picture>
</p>

# Keyhammer

Typo-tolerant top-k search over a compact trie, with keyboard-aware edit costs.

[![CI](https://github.com/Keyhammer/Keyhammer/actions/workflows/ci.yml/badge.svg)](https://github.com/Keyhammer/Keyhammer/actions/workflows/ci.yml)
License: [AGPL-3.0-or-later](LICENSE)

## Status

Early prototype. Nothing is published to crates.io or npm, and the API is
unstable. The new engine is much faster than the previous one, but its ranking
quality is currently below a simple "edit distance plus frequency" baseline.
See [`docs/benchmarks/m0.md`](docs/benchmarks/m0.md).

## What it is

Given a dictionary of terms with frequency weights, Keyhammer returns the top-k
terms closest to a mistyped query. Edit costs depend on the keyboard: hitting a
neighbouring key is cheaper than an arbitrary substitution. The core crate is
`no_std` (it needs `alloc`), uses `forbid(unsafe_code)` and has zero
dependencies. Queries and terms are lowercased bytes for now.

## How it works

- Terms are stored in a compact trie built once from the dictionary.
- Each trie node holds one weighted edit-distance row, computed in a narrow
  band around the diagonal.
- The search is exact best-first top-k: nodes are expanded in order of their
  lower bound, and an oracle test checks the results against brute force.
- An optional subtree signature bound (each node keeps the length range and a
  letter mask of the terms below it) prunes subtrees that cannot improve the
  result. On the benchmark data it expands about a third fewer nodes with the
  same results. Its ingredients are already published; see
  [`docs/papers.md`](docs/papers.md).

## Quick start

```rust
use keyhammer::cost::CostModel;
use keyhammer::search::{SearchConfig, Searcher};
use keyhammer::trie::Trie;

let trie = Trie::build(&[
    ("javascript", 10),
    ("typescript", 10),
    ("python", 10),
    ("rust", 10),
    ("java", 10),
])
.unwrap();
let costs = CostModel::qwerty();
let mut searcher = Searcher::new();

// "javasript" skips the 'c' of "javascript".
let out = searcher
    .search(&trie, &costs, b"javasript", &SearchConfig::default())
    .unwrap();
let best = &out.hits[0];
assert_eq!(trie.term(best.id), "javascript");
assert_eq!(best.cost, 16);
```

The same example runs as a doc test in `crates/keyhammer/src/lib.rs`.

## Results so far

Rust-only, one machine, one run, 300 typo pairs (Birkbeck corpus), one English
dictionary, provisional costs. Full dictionary of 274137 words; the baseline is
"unit edit distance <= 2, then higher weight". Full report and caveats:
[`docs/benchmarks/m0.md`](docs/benchmarks/m0.md).

| Engine | MRR | p95 latency (us) |
|---|---|---|
| legacy (previous engine) | 0.255 | 44691.7 |
| baseline | 0.433 | 79726.6 |
| new+tsb | 0.389 | 363.2 |

The p95 of the previous engine is 123.0x that of the new one. The new engine's
MRR is lower than the baseline's at every dictionary size tested (10000,
100000 and 274137 words).

## Repository layout

- `crates/keyhammer`: the core crate.
- `legacy/`: the previous engine, kept only as a benchmark reference.
- `crates/node`: Node.js binding of the legacy engine, used by the JS comparison.
- `bench/`: data preparation, the Rust harness and the JS comparison.
- `docs/`: benchmark reports and prior-art notes.

## Development

```bash
cargo fmt -p keyhammer -- --check
cargo clippy -p keyhammer --all-targets -- -D warnings
cargo test -p keyhammer
```

Benchmarks:

```bash
cd bench && npm install && node fetch-data.mjs && node prepare-m0-data.mjs
# then, from the repository root:
cargo run --release -p keyhammer-bench --bin m0 -- bench/data
```

The JS comparison (`node compare.mjs` in `bench/`) needs the legacy Node binding
built first. It loads `crates/node/keyhammer.node`:

```bash
cd crates/node && npm install && npx napi build --release --platform
# copy the produced .node file to crates/node/keyhammer.node
```

## Roadmap

- Improve ranking quality against the baseline.
- Unicode support and more keyboard layouts.
- Index serialization.
- Language bindings for the new engine.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

`AGPL-3.0-or-later`, see [LICENSE](LICENSE). Earlier commits, published before
the relicense commit ("chore: relicense to AGPL-3.0-or-later"), were released
under MIT.

## References

See [`docs/papers.md`](docs/papers.md).
