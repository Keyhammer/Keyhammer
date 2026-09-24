# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project follows
[Semantic Versioning](https://semver.org/) (0.x until the API settles).

## [Unreleased]

### Added
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
