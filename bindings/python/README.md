# keyhammer (Python)

Python binding for the Keyhammer core: typo-tolerant top-k search over a
compact trie, with keyboard-aware edit costs. **Unpublished prototype**: nothing
is on PyPI and the API may change. The binding is a thin PyO3 layer over the
`keyhammer` Rust crate in this repository; the algorithm and its measurements
are described in the [top-level README](../../README.md) and
[`docs/benchmarks/`](../../docs/benchmarks/).

## Install (from source)

You need a Rust toolchain (1.85 or newer) and Python 3.9 or newer.

```sh
python -m venv .venv && . .venv/bin/activate      # Scripts\activate on Windows
pip install maturin
cd bindings/python
maturin develop --release        # or: maturin build --release, then pip install the wheel
```

The wheel is built with the stable ABI (`abi3`, CPython 3.9+), so one wheel per
platform serves every supported Python version. CI builds and tests it on
Linux, macOS and Windows; wheels are not published.

## Usage

```python
import keyhammer as kh

index = kh.Index([("javascript", 10), ("typescript", 10), ("python", 10), ("java", 10)])
result = index.search("javasript", k=3)          # k, budget, ranking are optional
for hit in result.hits:                         # best first
    print(hit.term, hit.cost, hit.weight)       # javascript 16 10
print(result.nodes_expanded, result.truncated)

# Config objects and enums
cfg = kh.SearchConfig(k=5, budget=48, ranking=kh.Ranking.EXACT, tsb=True)
index.search("pyhton", config=cfg)
kh.SearchConfig.high_recall()                   # the core's opt-in preset (budget 48)
```

- `Index(items)`: `items` is any iterable of `(term: str, weight: int)`, weight
  in `0..=65535`. Duplicate terms are merged and keep the highest weight.
- `Index.search(query, k=None, budget=None, ranking=None, *, config=None)`:
  keyword values override `config`; without either, the core defaults are used
  (`k=10`, `budget=32`, `Ranking.COARSE`). `cost` is fixed point: 16 is one
  ordinary edit. `budget` is at most 64.
- `Ranking.COARSE` and `Ranking.EXACT` are the core's two orderings.
- Errors are exceptions, all subclasses of `keyhammer.KeyhammerError`, itself a
  `ValueError`: `BuildError` (empty input, empty or over-long term, weight out
  of range, non-ASCII term), `SearchError` with `QueryTooLongError` (query over
  128 bytes) and `BudgetTooLargeError`. A malformed item raises `TypeError`.
- Type stubs (`_keyhammer.pyi`) and `py.typed` ship with the package.

## Limits

- **ASCII only, lowercase letters in practice.** The core compares bytes and
  is designed for `a-z` until Unicode support lands (issue #19). The binding
  lower-cases ASCII letters and refuses non-ASCII terms and queries with an
  error instead of comparing UTF-8 bytes one by one. Other ASCII characters
  (digits, punctuation) are accepted but compared verbatim, with no keyboard
  neighbourhood; results on them are not tuned or measured.
- **Build once, no changes.** The issue asks for add, remove and export. The
  core has no overlay for insertions and deletions (#31) and no serialisation
  (#26), so the binding has none of them either: to change the dictionary,
  build a new `Index`. They will be added when the core supports them.
- **One cost model.** Only the QWERTY model is exposed; its costs are
  provisional (see the benchmark notes).
- Ranking quality against a plain "edit distance plus frequency" baseline is
  not established; see the top-level README.

## Thread safety

`Index` is immutable after construction (a frozen Python object holding a
read-only trie), so any number of threads can call `search` on the same
instance concurrently. Each call uses its own search scratch state, and the
GIL is released while building and searching. There is no shared mutable
state. The tests run 8 threads against one index and compare with the
single-threaded answer; that is a check, not a proof, and no free-threaded
(no-GIL) build was tested.

## Development

```sh
cd bindings/python
maturin develop --release
pip install pytest
pytest tests
cargo fmt --check && cargo clippy -- -D warnings
```

The crate is outside the root Cargo workspace (like the Node binding) and has
its own `Cargo.lock`, so the root `cargo deny` does not cover it.

## Safety and licences

The crate is `#![forbid(unsafe_code)]`: no hand-written `unsafe`. The only
`unsafe` is inside PyO3 and its macros, which is the FFI boundary. The
binding is AGPL-3.0-or-later like the rest of the repository. New dependencies
(all permissive, so they can be combined into an AGPL-3.0-or-later work;
licences from `cargo metadata`):

| Dependency | Role | Licence |
| --- | --- | --- |
| `pyo3`, `pyo3-ffi`, `pyo3-macros`, `pyo3-macros-backend`, `pyo3-build-config` 0.29.2 | linked into the wheel (`pyo3-macros*` and `pyo3-build-config` at build time only) | MIT OR Apache-2.0 |
| `libc`, `once_cell`, `portable-atomic`, `heck`, `proc-macro2`, `quote`, `syn` | transitive | MIT OR Apache-2.0 |
| `unicode-ident` | transitive, build time | (MIT OR Apache-2.0) AND Unicode-3.0 |
| `target-lexicon` | transitive, build time | Apache-2.0 WITH LLVM-exception |
| `maturin` (build tool, not linked) | packaging | MIT OR Apache-2.0 |
| `pytest` (tests only) | tests | MIT |

Exact versions are pinned in `bindings/python/Cargo.lock` (and pyo3 with `=`
in `Cargo.toml`).
