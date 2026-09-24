# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project follows
[Semantic Versioning](https://semver.org/) (0.x until the API settles).

## [Unreleased]

### Added
- New `keyhammer` core crate (M0 prototype): keyboard-weighted cost model, flat
  trie, exact best-first search, and brute-force oracle tests.
- CI workflow for the core crate.
- `bench/` harnesses and the dataset fetch script.
- `docs/papers.md` (reference check and prior art) and `docs/benchmarks/m0.md`
  (M0 benchmark results).

### Changed
- License changed from MIT to AGPL-3.0-or-later, effective from the commit
  "chore: relicense to AGPL-3.0-or-later". Versions obtained before that commit
  remain under MIT for those who received them.
- Current engine moved to `legacy/` and kept only as a benchmark reference.
