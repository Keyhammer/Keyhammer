# The code-point alphabet on the ASCII path (issue #19), 2026-09-24

Question: does moving the search alphabet from UTF-8 bytes to code points (and adding the
`text` module) change anything for ASCII dictionaries and queries? Design:
`docs/design/unicode.md`.

## Summary

- Results and work: identical. On the M0 data (300 Birkbeck typo pairs; 10 000, 100 000 and
  274 137 words), `main` (commit 28c0a4c) and this branch give the same trie (node count and a
  digest of every node's label, children, term id, lengths and `below_mask`) and, for ten
  configurations (default, `tsb: false`, `Ranking::Exact`, `high_recall()`, `high_recall()` with
  `tsb: false`, each in exact and prefix mode), the same totals of `nodes_expanded`,
  `nodes_pushed` and `rows_computed` and the same digest of every hit (id, cost, weight). The M0
  harness prints the same MRR, R@1, R@10 and nodes/query on both. This is deterministic.
- Latency: no difference that the runs can separate from noise. Medians of three runs of the M0
  harness per side, p50 / p95 of the default configuration (`new+tsb`), `main` against the
  branch: 92.1 / 127.5 against 91.6 / 126.6 us at 10 000 words, 210.3 / 300.6 against 212.7 /
  306.2 us at 100 000, 260.6 / 393.2 against 263.2 / 389.0 us at 274 137. The largest median
  difference in any row is +3.1% (p50 of `new`, the bound off, at 100 000 words: 251.7 against
  259.6 us); single runs of the same binary differ by up to 14% (p50 of `new` on `main` at
  274 137 words: 305.8 to 348.6 us).
- Build time: the harness's single build per run, median of three, is 2 / 24 / 68 ms on `main`
  and 3 / 25 / 72 ms on the branch (10 000 / 100 000 / 274 137 words). The 4 ms (about 6%) at the
  full dictionary may be real: `Trie::build` now checks whether every term is ASCII and walks
  non-ASCII dictionaries as a `u32` buffer. Not investigated further.
- Memory of an ASCII (or Latin-1) trie: unchanged by construction (labels stay one byte per node;
  a `Vec<u32>` is used only when a label is above U+00FF). Not measured.
- WebAssembly: 38 836 bytes raw and 16 657 bytes gzip -9 on `main`, 42 566 and 18 177 on the
  branch (budget 20 480): +1 520 bytes gzip (+9.1%), 2 303 bytes left under the gate. The
  binding does not call the normaliser, so its tables are not in the module; the growth is the
  UTF-8 decoding of queries, the symbol version of the search and two instances of the trie
  layout (bytes for ASCII dictionaries, code points otherwise). The byte fast path of the build
  costs 598 of the 1 520 bytes (17 579 bytes gzip without it).
- `no_std`: `cargo build -p keyhammer --target thumbv7em-none-eabihf` and the `wasm32` build pass;
  no dependency was added.

## Method

- Machine: one Windows 11 PC (the one of `tsb-default.md`), shared with other agents. Before
  each pair of runs the process list held two idle `node.exe` processes and no `cargo`, `rustc`
  or other benchmark; the machine was not sampled during the runs. Not a controlled benchmark.
- Deterministic comparison: a throwaway program (not committed) built against `git archive
  origin/main` and against the branch, reading the M0 data, printing per dictionary the node
  count, a trie digest and, per configuration and mode, the summed work counters and a digest of
  the hits. The two outputs were compared with `diff` (build times removed): no difference.
- Latency: `bench/src/bin/m0.rs` unchanged, release build of each tree, run alternately
  (`main`, branch) three times: `cargo run --release -p keyhammer-bench --bin m0 -- bench/data`.
  Medians of the three runs.
- Size: `cargo build -p keyhammer-wasm --target wasm32-unknown-unknown --profile wasm`, then
  `gzip -9 -n -c | wc -c`, without the CI's path remapping on either side (so the absolute numbers
  are a few bytes off the CI's; the difference is what matters).

## Results

`new+tsb` is `SearchConfig::default()`; `new` has `tsb: false`; `hr+tsb` is
`SearchConfig::high_recall()`. Latencies in microseconds, median of three runs.

| Words | Row | nodes/query | p50 main | p50 branch | p95 main | p95 branch |
|---|---|---|---|---|---|---|
| 10 000 | new+tsb | 491 | 92.1 | 91.6 | 127.5 | 126.6 |
| 10 000 | new | 735 | 110.1 | 109.4 | 148.6 | 151.4 |
| 10 000 | hr+tsb | 2551 | 512.3 | 510.5 | 711.2 | 708.1 |
| 100 000 | new+tsb | 911 | 210.3 | 212.7 | 300.6 | 306.2 |
| 100 000 | new | 1393 | 251.7 | 259.6 | 362.7 | 363.9 |
| 100 000 | hr+tsb | 4610 | 1211.9 | 1217.8 | 2168.7 | 2230.4 |
| 274 137 | new+tsb | 995 | 260.6 | 263.2 | 393.2 | 389.0 |
| 274 137 | new | 1514 | 308.5 | 310.3 | 495.6 | 474.5 |
| 274 137 | hr+tsb | 4590 | 972.5 | 964.6 | 2903.7 | 2825.0 |

MRR, R@1 and R@10 are equal on both sides in every row (for example 0.429 / 0.380 / 0.517 for
`new+tsb` at 274 137 words).

Deterministic totals over the 300 queries at 274 137 words (both sides): default exact search
298 628 nodes expanded, 354 476 pushed, 1 571 351 rows; default prefix search 235 481 / 320 294 /
1 236 137; `high_recall()` exact 1 377 052 / 2 135 421 / 5 089 925. Trie: 606 247 nodes.

## Not measured, and caveats

- Non-ASCII dictionaries: no benchmark corpus exists here, so the speed of the new paths (wide
  labels, normalisation) is not measured; only their correctness is tested.
- The latency medians come from three runs per side on a shared machine; differences of a few
  percent, in either direction, are within the spread of the runs.
- Other layouts than QWERTY were not timed; their results on `a`-`z` text are pinned by
  `tests/ascii_regression.rs`.
