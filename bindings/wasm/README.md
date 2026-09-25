# keyhammer-wasm

A WebAssembly build of the `keyhammer` core with a tiny C-ABI, loaded with the
plain `WebAssembly` API (no wasm-bindgen, no generated glue code, no
dependencies besides `keyhammer`). It powers the live demo page of the
documentation site. Status: unpublished prototype; the interface may change at
any time.

## Exported functions

| Export | Signature | Meaning |
| --- | --- | --- |
| `memory` | linear memory | Where the buffers below live. |
| `kh_alloc` | `(len: u32) -> *mut u8` | Allocates `len` bytes for JavaScript to fill. Returns 0 if the allocation fails. |
| `kh_free` | `(ptr: *mut u8, len: u32)` | Frees a `kh_alloc` buffer; `len` must be the same. |
| `kh_build` | `(ptr, len) -> u32` | Builds the index from UTF-8 text (format below). Returns the number of distinct terms, or 0 on failure; after a failure there is no index. |
| `kh_search` | `(ptr, len, k: u32, budget: u32, ranking: u32) -> u32` | Searches for the UTF-8 query at `(ptr, len)`. `ranking`: 0 = `Coarse`, 1 = `Exact`. The query is normalised like the terms. Returns the number of hits, or `u32::MAX` on error (no index, query longer than 128 code points after normalisation, budget too large, unknown ranking, invalid UTF-8). |
| `kh_results_ptr` / `kh_results_len` | `() -> *const u8` / `() -> u32` | The results text of the last search (format below), valid until the next `kh_build` or `kh_search`. |

Searches use `SearchConfig::default()`, subtree bound on.

**Dictionary text** (`kh_build`): one term per line, optionally followed by
`TAB weight` (0 to 65535, default 0). Terms are normalised with the core's
default `Normalizer` (case and diacritics folded: `São Paulo` is stored as
`sao paulo`, `Straße` as `strasse`; policy in `docs/design/unicode.md`).
Lines that are empty, longer than 65535 bytes, whose weight is not a valid
number, or that normalise to nothing (only combining marks) are skipped.
Terms that are equal after normalisation are merged (highest weight, then the
first line).

**Results text** (`kh_results_ptr`): a header line
`nodes_expanded TAB truncated` (trie nodes expanded; `1` if the node limit
stopped the search early, else `0`), then one line per hit, best first:
`term TAB cost TAB weight`, where `term` is the text as given to `kh_build`
(original case and accents, not the normalised form; for merged terms, the line
that was kept) and `cost` is the exact fixed-point cost (16 is
one ordinary edit). Every line ends with `\n`. After an error the text is
empty.

## Build and test

From the repository root, with the `wasm32-unknown-unknown` target installed
(`rustup target add wasm32-unknown-unknown`):

```bash
remap="--remap-path-prefix=$PWD=/src"
remap="$remap --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo"
remap="$remap --remap-path-prefix=${RUSTUP_HOME:-$HOME/.rustup}=/rustup"
RUSTFLAGS="$remap" cargo build -p keyhammer-wasm --target wasm32-unknown-unknown --profile wasm
node bindings/wasm/test.mjs
```

The module is `target/wasm32-unknown-unknown/wasm/keyhammer_wasm.wasm`. The
`wasm` profile (in the workspace `Cargo.toml`) optimises for size and leaves
`release`, which the benchmarks use, unchanged.

The `--remap-path-prefix` flags matter because the module is served publicly:
panic locations embed source paths, which would otherwise include the build
machine's home and workspace directories. On Windows, pass the paths in the
form rustc sees them (for example with `cygpath -w` in Git Bash). The test
fails if the module contains such a path; set `KH_ALLOW_PATHS=1` to run the
functional tests on a build without the flags.

To try the demo page locally, copy the module into the site before building
it:

```bash
mkdir -p website/static/wasm
cp target/wasm32-unknown-unknown/wasm/keyhammer_wasm.wasm website/static/wasm/keyhammer.wasm
```

## Size

Measured on one machine with the CI flags: 45 771 bytes raw and 19 559 bytes
with `gzip -9 -n` (what the CI size gate measures; the budget is 20 480), against
42 506 and 18 155 before the normaliser was linked (+1 404 bytes gzip); the test
prints the raw size and Node's zlib level 9 figure, which is a little larger
(19 805). The increase is the folding tables and the code of the normaliser. The CI job
measured 19 728 bytes for the last commit of that change (19 511 for the first; the local figure is
about 50 bytes higher); the budget has under 1 KB left, so the next feature that
grows the module must either shrink it elsewhere or raise `WASM_GZIP_BUDGET` in
`ci.yml` with a stated reason. The largest parts are the index build and the search with
the core inlined, the `dlmalloc` allocator from `std`, the sort used to build
the index, and the formatting code that panic locations in the core still pull
in.

## Limits

- Case and diacritics are folded with the core's default normaliser only; there
  is no option to turn either off. The tables cover Latin-1 and Latin
  Extended-A (plus ligatures); other scripts are compared per code point as they
  are (no Greek or Cyrillic case folding). The 128 code point query limit is
  counted after folding (`ß` counts as two).
- The module keeps a copy of the terms as given (to return them), so the
  memory of the terms is roughly doubled.
- The module holds a single index; `kh_build` replaces it.
- The costs are provisional and uncalibrated.
- `unsafe` is used only at the boundary (raw pointers from JavaScript), with a
  safety comment on every block; the core crate forbids it. No input makes the
  exported functions panic, but running out of memory inside the core, which
  allocates infallibly, still traps the module.
