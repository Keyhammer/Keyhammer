# Should `SearchConfig::default()` enable the subtree bound? (issue #49), 2026-09-24

Decision: yes. `SearchConfig::default()` now has `tsb = true`. Results are identical with it on or off, unless `max_nodes` truncates a search (then both modes return a correct prefix of the same ranked list, possibly of different length). No cost could be measured: the per-node signature data is built either way. `tsb: false` remains available.

## Summary

- Results: identical. The brute-force oracle tests (`tests/oracle.rs`, `tests/edge_cases.rs`, `tests/fuzz_like.rs`, the `tsb_equivalence` fuzz target) already run both modes; in the M0 harness MRR, R@1 and R@10 are equal to every printed digit with the bound on and off, at all three sizes, and a new test (`the_default_enables_the_bound_and_results_do_not_depend_on_it`) pins that the default equals the bound-off result and expands no more nodes.
- Work: with the bound on, the default budget (32) expands 33.2%, 34.6% and 34.3% fewer trie nodes per query at 10 000, 100 000 and 274 137 words (735 to 491, 1393 to 911, 1514 to 995).
- Latency (median of three runs): p95 is 20.8%, 20.1% and 19.0% lower, p50 is 18.7%, 19.5% and 23.3% lower.
- Memory and build time: no change. The subtree data (`below_mask`, `len_min`, `len_max`, about 12 bytes per trie node) is computed by `Trie::build` whatever `tsb` says, and the harness build times of the two rows are the same run's single build. Turning the flag off does not free anything.
- WebAssembly: the module is 39 278 bytes raw both before and after the flip; gzip -9 is 16 792 vs 16 795 bytes (budget 20 480). The bound's code was already compiled in (`tsb` is a runtime flag), so only a constant changes.
- `no_std`: `cargo build -p keyhammer --target thumbv7em-none-eabihf` builds before and after; no new dependency or allocation.

## Method

- Machine and load: one Windows 11 machine with 16 logical cores, shared with other agent processes running builds and tests at the time, so absolute latencies are noisy (the third run below was visibly quieter than the first two). Not a controlled benchmark.
- Harness: `bench/src/bin/m0.rs` as already in the repository (rows `new` = `SearchConfig::default()` with `tsb: false`, and `new+tsb` = `tsb: true`, both `k = 10`, budget 32, `Ranking::Coarse`, QWERTY costs), release build, on the 300 Birkbeck typo pairs and the same three dictionaries as `m0.md` (10 000, 100 000, 274 137 words; data not committed, see `bench/`). Each row does an untimed warm-up pass over the queries, then a timed pass. Run three times, one after the other, on the code before the flip (the rows force `tsb` explicitly, so they do not depend on the default). Reported: median of the three for latencies; nodes and quality are deterministic and identical in all three runs.
- Reproduce: `cargo run --release -p keyhammer-bench --bin m0 -- bench/data`.

## Results

Default budget (32), `Ranking::Coarse`, 300 typo pairs. Median of three runs; latencies in microseconds.

| Words | Row | MRR | R@1 | nodes/query | p50 | p95 |
|---|---|---|---|---|---|---|
| 10 000 | tsb off | 0.541 | 0.527 | 735 | 111.1 | 157.6 |
| 10 000 | tsb on | 0.541 | 0.527 | 491 | 90.3 | 124.8 |
| 100 000 | tsb off | 0.470 | 0.430 | 1393 | 258.1 | 372.7 |
| 100 000 | tsb on | 0.470 | 0.430 | 911 | 207.7 | 297.9 |
| 274 137 | tsb off | 0.429 | 0.380 | 1514 | 392.0 | 630.6 |
| 274 137 | tsb on | 0.429 | 0.380 | 995 | 300.5 | 510.8 |

Relative change with the bound on: nodes -33.2% / -34.6% / -34.3%; p50 -18.7% / -19.5% / -23.3%; p95 -20.8% / -20.1% / -19.0%.

The three individual p95 values (off; on), in microseconds, to show the spread: 10 000 words 163.3, 149.2, 157.6; 124.5, 124.8, 176.9. 100 000 words 367.8, 372.7, 395.9; 297.9, 296.0, 310.3. 274 137 words 677.1, 630.6, 476.3; 536.2, 510.8, 369.4. In 8 of the 9 (size, run) pairs p95 is lower with the bound on; the exception is the third run at 10 000 words (176.9 vs 157.6), where a single noisy run inflated the bound-on row. For the median-of-three that difference does not show, but it is a reminder that these are small absolute latencies (100 to 600 us) on a shared machine.

The budget-48 rows (`hr` vs `hr+tsb`, `recall-preset.md`) show the same direction: nodes 3562 to 2551, 6613 to 4610 and 6660 to 4590; the p95 medians of the same three runs are 822.1 to 709.0, 2513.8 to 2698.5 and 4885.7 to 3587.5 us, so at 100 000 words the budget-48 p95 was not lower with the bound on in the median (2698.5 vs 2513.8), although its p50 was (1418.4 vs 1460.1), a spread this data cannot separate from noise. This is the one place where the bound did not clearly help p95; node counts were lower in every case.

## Not measured, and caveats

- Build time and memory of the signature data: not measured separately because there is nothing to compare; the fields are filled in one pass in `Trie::build` and stored regardless of the flag (`crates/keyhammer/src/trie.rs`). The 12 bytes per node is read from the field types, not measured.
- Per-query cost of the bound: on each node the bound reads three words of the node and one word of the query's suffix masks; the suffix masks (`qmask`) are also computed for every query regardless of the flag. Whether the bound can lose on very small dictionaries or very short queries was not measured; the only evidence is the 2000-word random-dictionary unit test in `m0.md` (34.5% fewer queue entries, counted, not timed).
- Only the Birkbeck typo corpus and one English dictionary; other languages, keyboard layouts or cost models may differ.

## Consequences

- Semver: pre-1.0, so a behaviour change to a default is allowed in a minor release; it is noted in the CHANGELOG. Hits, order and `Hit::cost` do not change, except that if `max_nodes` stops a search, both modes return a correct prefix of the same ranked list, but its length can differ. What changes: `Stats::nodes_expanded` and `Stats::nodes_pushed` are lower for the same query, and a search that used to hit `max_nodes` may now complete (the truncation point depends on the work done; nothing was observed to truncate in these runs).
- Callers who want the previous behaviour set `tsb: false`. The WebAssembly interface has no such option (it builds its config from `SearchConfig::default()`), so wasm searches now run with the bound on; the numbers in `competitors-js.md` were taken with it off and are not re-measured here.
- `SearchConfig::high_recall()` sets `tsb: true` explicitly; this is now redundant but harmless, and keeps the preset independent of the default.
