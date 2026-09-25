# Fuzzing

The core must never panic, hang or use unbounded memory, whatever it is fed.
`crates/keyhammer/fuzz/` holds [cargo-fuzz](https://rust-fuzz.github.io/book/cargo-fuzz.html)
(libFuzzer) targets. It is a separate Cargo workspace, so the main workspace and
its CI need neither nightly nor libFuzzer.

| Target | Property |
| --- | --- |
| `never_panics` | Building and searching on arbitrary bytes, with an arbitrary `k`, budget, node limit, ranking and `tsb`, returns `Ok` or `Err` and never panics; hits respect `k` and the budget. The same holds for `Trie::build_normalized` with `search_text` and `search_prefix_text`. |
| `oracle_equality` | On small inputs, hits equal a brute-force reference (`tests/support`), for `tsb` on and off and both rankings. |
| `tsb_equivalence` | Results with `tsb: true` equal those with `tsb: false`. |
| `prefix_oracle_equality` | `search_prefix` hits equal a brute-force prefix reference (minimum over all term prefixes), for `tsb` on and off and both rankings; a node-limited run returns true costs. |
| `normalize` | `text::Normalizer` on any `&str` (combining marks, joiners, bidirectional marks, emoji, NUL included), in all four modes, never panics, is deterministic and idempotent, and its source map stays monotone and in range and converts to valid byte and UTF-16 ranges. |
| `text_oracle_equality` | On a trie built with `Trie::build_normalized` (one of the four normaliser modes) from text with case pairs, precomposed and combining accents, `ß`, `æ`, Cyrillic and an emoji, `search_text` and `search_prefix_text` equal the brute-force references run on the normalised query, for `tsb` on and off and both rankings. |
| `highlight` | For every hit of `search_text` and `search_prefix_text` on the text of `text_oracle_equality`, `highlight_text` in the matching mode succeeds, its cost equals `Hit::cost` and the brute-force cost, and its ranges are valid, agreeing code-point, UTF-8 and UTF-16 ranges of the original term (sorted, disjoint, inside `aligned`). On arbitrary bytes, highlighting a made-up hit with any query and source string returns `Ok` or `Err` and never panics. |

Input layouts are documented in `crates/keyhammer/tests/fuzz_props/mod.rs`, which
holds the properties themselves.

## Running

```sh
cargo install cargo-fuzz
cd crates/keyhammer
cargo +nightly fuzz run never_panics -- -max_total_time=60
cargo +nightly fuzz run oracle_equality
cargo +nightly fuzz run tsb_equivalence
cargo +nightly fuzz run prefix_oracle_equality
cargo +nightly fuzz run normalize
cargo +nightly fuzz run text_oracle_equality
cargo +nightly fuzz run highlight
```

libFuzzer works best on Linux and macOS; on Windows use WSL. Corpora and crash
artifacts (`fuzz/corpus`, `fuzz/artifacts`) are git-ignored. To replay a crash:
`cargo +nightly fuzz run <target> fuzz/artifacts/<target>/<file>`.

`.github/workflows/fuzz.yml` runs each target for 60 seconds every week and on
demand (Actions, "Run workflow", with a `seconds` input).

## Without libFuzzer

`crates/keyhammer/tests/fuzz_like.rs` feeds the same properties with seeded
pseudo-random bytes, so plain `cargo test -p keyhammer` covers them everywhere.
Structured inputs put terms within a few edits of the query. Measured with
temporary counters, of the calls that reach a search: never_panics 20 000 calls,
76% build a trie, 64% search, 56% return a hit, 7% return a full `k`;
oracle_equality 4 000 calls, 99% search, 84% return a hit, 32% a full `k`;
tsb_equivalence 8 000 calls, 99% search, 78% return a hit, 57% a full `k`;
text_oracle_equality 3 000 calls, 2 860 build a trie and search, 2 388 have at
least one hit within the budget (QWERTY, coarse ranking), 2 300 have non-ASCII
text in the normalised query or a term; highlight 3 000 calls highlight 17 426
hits (9 548 of them prefix hits, 2 943 with more than one range) and 107 made-up
hits on raw input that happen to be consistent. It
catches these mutations of the core: dropping the transposition term in
`lower_bound`, an under-counting `tsb` bound, and wrong `len_min`, `len_max` or
`below_mask` trie metadata. It is a regression net, not a substitute for coverage-guided fuzzing.
