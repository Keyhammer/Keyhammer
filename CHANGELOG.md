# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project follows
[Semantic Versioning](https://semver.org/) (0.x until the API settles).

## [Unreleased]

### Changed
- `SearchConfig::default()` now has `tsb: true` (the subtree bound was off). Hits, order and
  costs are unchanged unless `max_nodes` truncates a search (then both modes return a correct prefix of the same list, possibly of different length) (oracle-tested); on the 300 Birkbeck typo pairs it expands 33-35% fewer nodes with
  about 19-21% lower p95 latency at the default budget (one machine, shared, median of three runs),
  and costs no extra memory (the per-node data is always built). `Stats` node counts change, and the
  WebAssembly module, which builds its config from the default, now runs with the bound on. Set
  `tsb: false` for the old behaviour. See `docs/benchmarks/tsb-default.md` (issue #49).

### Added
- `cost::Layout` (QWERTY, QWERTZ, AZERTY, ABNT2, Dvorak, Colemak; `#[non_exhaustive]`) and `CostModel::for_layout`: the neighbour table is derived from key geometry (staggered rows), `CostModel::qwerty()` is unchanged (regression-tested against the old table). Only a-z pairs count: ABNT2's a-z letters equal QWERTY's because `ç` is outside the alphabet (issue #19). Search, band width and subtree bound use the model's minimum costs, so they hold for every layout (oracle and fuzz tests run all layouts). `CostModel` now also derives `PartialEq`/`Eq`. First part of issue #20: not exposed in the bindings yet, no dead keys or `ç` costs until #19.
- `docs/benchmarks/sum-bound.md`, `tests/sum_bound.rs`, `bench/src/bin/sumbound.rs`: research on a summed subtree-signature bound (#44). The sum is admissible under the current costs (argued in `docs/design/lower-bound.md` section 3a, no counterexample in about 13.2 million tested cases) but expands only 0.14-0.33% fewer nodes than `max`; not adopted, core unchanged.
- `bench/src/bin/{recall,phonetic}.rs` and `docs/benchmarks/recall-beyond-two-edits.md`: research on recall
  beyond two edits (budgets 32, 48 and 64 with paired differences, nodes and latency; what the unreachable pairs
  are) and on phonetic and spelling-rule candidates prototyped outside the core with held-out splits. Verdict: no
  budget-64 preset, no spelling-rule operation; phonetic candidates are a promising follow-up (issue #22).
- `Searcher::search_prefix`: a prefix (autocomplete) mode. A term matches when the query is within
  the budget of some prefix of it (cost = best alignment of the whole query against any prefix,
  transpositions only inside the prefix); ranking is as in `search` (cost rank, weight, id). The
  subtree bound is adapted (only the upper length end constrains a prefix) and subtrees whose
  terms all cost the same are enumerated by weight without DP rows. Oracle-tested against a
  brute-force prefix reference (tsb on/off, both rankings), with a property test of the adapted
  bound and a new `prefix_oracle_equality` fuzz target. `Stats` gains `rows_computed` and is now `#[non_exhaustive]` (it can no longer be built with a struct literal outside the crate). Work counters
  against the exact mode are in `docs/benchmarks/prefix-mode.md`; the proof is in
  `docs/design/prefix-mode.md` (issue #30).
- `bench/src/bin/calib.rs` (behind `--features calibration`), `bench/experiments/cost-knobs.patch` and
  `docs/benchmarks/calibration{-preregistration,}.md`: a pre-registered calibration of the cost model on
  four corpora with train, validation and test splits (300 models). The rule proposes a follow-up
  (first-byte factor 1.25, neighbouring-key substitution 12, indel 12: +0.0134 MRR@10 on the test split,
  SE 0.0015, but about 1.67x the nodes expanded); the shipped costs are unchanged and the core crate is
  untouched (issue #21).
- `bindings/node`: a Node.js package for the new engine, plain JavaScript over the
  WebAssembly build (`Index.build`, `index.search` with `k`, `budget` and `ranking`,
  TypeScript types, argument errors); tested by a CI job on ubuntu, macos and windows
  with Node 18 and 22. Nothing is published. The choice against a native addon is in
  `docs/design/node-binding.md`.
- `bench/src/bin/tiebreak.rs` and `docs/benchmarks/tiebreak-{preregistration,result}.md`: a pre-registered
  test of a ranking tie-break variant on disjoint typo samples (`fetch-typo-corpora.mjs --holdout`).
  Verdict: do not adopt (+0.0030 MRR@10 at 274 137 words, but a significant loss on two error
  categories); the default ranking is unchanged.
- `bindings/python`: a Python package (PyO3 and maturin, abi3 wheels) with
  `Index`, `SearchConfig`, `Ranking`, errors as exceptions and type stubs.
  ASCII only for now; no add/remove/export because the core has none yet
  (#31, #26). A CI job builds and tests the wheel on Linux, macOS and Windows;
  nothing is published. Its dependencies (pyo3 and maturin) are MIT OR
  Apache-2.0; see `bindings/python/README.md`.
- `bench/fetch-finger-slips.mjs` and `bench/src/bin/slips.rs`: a corpus of real typing
  errors built from the 136M Keystrokes dataset (SHA-256 checked, never committed,
  non-commercial licence), classified by edit operation, and a first measurement of the
  engine against a unit-cost baseline on it. The shipped engine ranked below a plain
  OSA<=2 baseline (MRR@10 -0.031, SE 0.004), all of it on first-letter pairs; elsewhere
  +0.002 (SE 0.003); no measurable benefit of the adjacency costs; see
  `docs/benchmarks/finger-slips.md`.
- `keyhammer-c` (`bindings/c`): a stable, handle-based C ABI (`kh_index_build`,
  `kh_search`, explicit free functions, status codes with `kh_last_error`,
  `kh_abi_version`), a cbindgen-generated `include/keyhammer.h` checked for
  freshness in CI, a C smoke test compiled in CI, and `docs/design/c-abi.md`
  with the ABI stability rules. Unsafe code is confined to this crate.
- Fuzz targets (`crates/keyhammer/fuzz`, cargo-fuzz) for build and search
  robustness, brute-force oracle equality and `tsb` on/off equivalence, a
  weekly `fuzz` workflow, and a deterministic `tests/fuzz_like.rs` running the
  same properties in `cargo test`; see `docs/fuzzing.md`.
- CI: `cargo deny` (licences, advisories, bans, sources; `deny.toml`), a weekly
  Miri run over the core tests, and a gzipped `.wasm` size gate (budget 20 KB).
- `SearchConfig::high_recall()`: an opt-in preset with `budget = 48` and
  `tsb: true`; the default budget stays 32. On the 300 Birkbeck typo pairs it
  found the right word more often. With `tsb: true` on both sides, it expanded
  about 4.6-5.2x the nodes and had about 5.5-7.3x the p95 latency (about
  5.2-7.5x with the bound off); see `docs/benchmarks/recall-preset.md`. The M0 harness reports the
  preset too.
- New `keyhammer` core crate (M0 prototype): keyboard-weighted cost model, flat
  trie, exact best-first search, and brute-force oracle tests.
- CI workflow for the core crate.
- `bench/js-competitors/`: a harness that runs the WebAssembly build in Node against MiniSearch,
  Fuse.js, uFuzzy and fuzzysort, with its report `docs/benchmarks/competitors-js.md`; a CI job runs
  a small smoke configuration of it.
- `bench/` harnesses and the dataset fetch script.
- `docs/papers.md` (reference check and prior art) and `docs/benchmarks/m0.md`
  (M0 benchmark results).
- `search::Ranking` (`Coarse`, the default, and `Exact`), the
  `SearchConfig::ranking` field, `cost::COST_UNIT` and `cost::whole_units`.
- `Trie::input_index`: maps a term id (a position in the sorted,
  deduplicated list) back to the index in the input slice of the entry that
  was kept.
- `trie::BuildError::TooManyTerms`: `Trie::build` returns it instead of
  wrapping when given more than `u32::MAX` terms.
- `docs/design/lower-bound.md`: a written proof that the search's lower bound
  is admissible, with what the tests check and what is only argued.

### Removed
- `crates/node`, the Node binding of the legacy engine; `bench/compare.mjs` now
  loads `bindings/node` and so runs the new engine.

### Changed
- The M0 harness (`bench/src/bin/m0.rs`) now exits with status 1 when a gate
  check on the largest dictionary prints FAIL ((b) p95 ratio, (c) strict MRR;
  the informational (c') does not count), when a legacy search returns an
  error (counted and shown as `errors=N` in the legacy row) or when a gate row
  is missing. Malformed TSV lines and non-`u16` frequencies now abort with
  status 2 and the file and line, instead of being dropped or read as 0. Each
  engine runs an untimed warm-up pass before timing. `--help` documents the
  exit codes. Printed quality numbers are unchanged. On the published data (c)
  fails on the full dictionary, so the harness exits 1 there; this is the
  documented G0 partial pass (docs/benchmarks/m0.md), not a new failure.
- License changed from MIT to AGPL-3.0-or-later, effective from the commit
  "chore: relicense to AGPL-3.0-or-later". Versions obtained before that commit
  remain under MIT for those who received them.
- Current engine moved to `legacy/` and kept only as a benchmark reference.
- Default ranking is now the weighted cost rounded up to whole units of 16,
  then higher weight, then term id (`Ranking::Coarse`). This is not a count of
  edits: the x1.5 factor on the first byte makes an ordinary
  (non-neighbouring-key) edit there cost 24, i.e. two units, while a
  neighbouring-key substitution there costs 12 (one unit) and a transposition
  18 (two units); two cheap edits (8 + 8) count as one. `Ranking::Exact` restores the previous order by exact
  weighted cost. `Hit::cost` is the exact weighted cost in both modes.
- With the default ranking, `Output::hits` is no longer ordered by ascending
  exact `cost`: do not assume that `hits[0].cost` is the minimum; use
  `Ranking::Exact` for that order.
- `SearchConfig` gained the public field `ranking` and is not
  `#[non_exhaustive]`, so code that builds it with a struct literal without
  `..SearchConfig::default()` no longer compiles.
