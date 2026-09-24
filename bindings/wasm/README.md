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
| `kh_search` | `(ptr, len, k: u32, budget: u32, ranking: u32) -> u32` | Searches for the UTF-8 query at `(ptr, len)`. `ranking`: 0 = `Coarse`, 1 = `Exact`. Returns the number of hits, or `u32::MAX` on error (no index, query longer than 128 bytes, budget too large, unknown ranking, invalid UTF-8). |
| `kh_results_ptr` / `kh_results_len` | `() -> *const u8` / `() -> u32` | The results text of the last search (format below), valid until the next `kh_build` or `kh_search`. |

Searches use `SearchConfig::default()`, subtree bound on.

**Dictionary text** (`kh_build`): one term per line, optionally followed by
`TAB weight` (0 to 65535, default 0). ASCII letters are lower-cased. Lines that
are empty, longer than 65535 bytes or whose weight is not a valid number are
skipped. Duplicates keep the highest weight.

**Results text** (`kh_results_ptr`): a header line
`nodes_expanded TAB truncated` (trie nodes expanded; `1` if the node limit
stopped the search early, else `0`), then one line per hit, best first:
`term TAB cost TAB weight`, where `cost` is the exact fixed-point cost (16 is
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

Measured by CI: about 39 KB raw and about 17 KB gzip (level 9); the test prints
the exact figures. The largest parts are the index build and the search with
the core inlined, the `dlmalloc` allocator from `std`, the sort used to build
the index, and the formatting code that panic locations in the core still pull
in.

## Limits

- ASCII letters are lower-cased; any other character is compared as it is,
  one code point per symbol (the core's alphabet since issue #19). The core's
  case and diacritic folding (`keyhammer::text`) is not used by this binding
  yet, so `É` and `é` differ and `é` against `e` costs a substitution.
- The module holds a single index; `kh_build` replaces it.
- The costs are provisional and uncalibrated.
- `unsafe` is used only at the boundary (raw pointers from JavaScript), with a
  safety comment on every block; the core crate forbids it. No input makes the
  exported functions panic, but running out of memory inside the core, which
  allocates infallibly, still traps the module.
