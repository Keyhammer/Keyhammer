# Node.js binding: a wasm-based package, not a native addon

Issue #34. Status: decided, implemented in `bindings/node`. The legacy binding
(`crates/node`, napi over `legacy/`) is removed.

## Decision

The npm package is a small JavaScript module over the existing WebAssembly
build (`bindings/wasm`). There is no new Rust crate and no native addon.

## Options

**A. napi-rs addon over the core.** A new `cdylib` crate depending on `napi` and
`napi-derive`, with the napi boundary the only place that needs care, and prebuilt
binaries.

**B. Wasm as the package** (chosen). `bindings/node/index.js` instantiates
`keyhammer.wasm` with the plain `WebAssembly` API and wraps its C-ABI.

## Evidence

Measured in this repository:

| | A. napi addon | B. wasm package |
| --- | --- | --- |
| Artifacts to ship | one binary per platform and libc, each a separate npm package chosen through `optionalDependencies` (the napi-rs convention; at least Windows x64, macOS arm64 and x64, Linux x64 glibc, and more for musl and arm64) | one file, `keyhammer.wasm`, 39 218 bytes (16 896 gzip) |
| Published package | not built; size of a `.node` binary not measured (hypothesis: hundreds of KB to MB) | `npm pack` (Linux checkout): 20 692 bytes tarball, 48 256 bytes unpacked (README.md, index.js, index.d.ts, keyhammer.wasm, package.json) |
| Install-time steps | none if a prebuilt matches; otherwise the package fails to load (unless a source-build fallback is added, which needs a Rust toolchain) | none on any platform Node supports |
| CI to test three platforms | build matrix of 3 or more targets (cross-compilation for arm64 and musl), then test each | build once on Linux, run the same file on ubuntu, macos and windows (the `node` job) |
| `unsafe` | napi-rs user code is safe Rust, but the `#[napi]` macros expand to `unsafe` code, which conflicts with the workspace's `unsafe_code = "forbid"` (the crate would need its own lint setting), plus a new dependency tree: `napi`, `napi-derive`, `napi-build` and the `@napi-rs/cli` toolchain | none in the package; the existing wasm crate already has the audited raw-pointer boundary |
| Dependencies | Rust and npm dependencies to vet; `deny.toml` covers only the root workspace (#54) | none |
| Also runs in | Node only | Node; the wasm module itself is not Node specific (only the file loader in `index.js` is), but it was not tested elsewhere here |

Speed, from `docs/benchmarks/competitors-js.md` (one machine, one run, Birkbeck
queries, p50, wasm build without the subtree bound): 195.9, 443.8 and 543.4 us
at 10 000, 100 000 and 274 137 terms, against the native build's 89.1, 207.9 and
250.0 us in the Rust report, so the wasm build takes about 2.2 times the native
time, from two different harnesses and runs. That report also found the
marshalling (query encoding, result decoding) not distinguishable from noise in
a throwaway measurement, and did not separate how much of the 2.2 is WebAssembly
itself, the size-optimised profile or the missing bound. In that report the wasm
build is already ahead of MiniSearch at the full dictionary (p50 543.4 against
587.9 us) and far ahead of Fuse and uFuzzy there.

Not measured: a napi addon. Its per-call overhead (string conversion, the result
array of objects) was not measured, so the expected speed-up over wasm is a
hypothesis bounded above by the 2.2 factor, not a result. Nothing here says
which option a user in a hot loop at hundreds of thousands of queries per second
would prefer; both are sub-millisecond per query at every size measured.

## Why B

Fewest moving parts that meet the acceptance criteria: the cost of A is a
platform matrix, a second publishing pipeline and new `unsafe` and dependencies,
for a speed-up of at most about 2.2 times (a cross-harness estimate, see above) on queries that already take 0.2
to 0.5 ms. B ships one file that is byte-identical on every platform, so testing
it on the three operating systems tests what users get. The costs of B are
accepted openly: slower than native by the factor above; each index owns a
WebAssembly instance (linear memory grows and never shrinks; the wasm memory
growth per term is in `competitors-js.md`); no `tsb` option until the wasm
interface exposes it; the wasm interface parses a text format, so `index.js`
checks arguments and the dictionary before the call.

Revisit A if a user needs the native speed. The API below is deliberately small
so that a native implementation could keep it.

## Node versions

`engines` stays `>=18` and CI tests Node 18 and 22: the code uses only features
available in 18 (private fields, `Object.hasOwn`, lookbehind), Node 18 is what
older deployments still run, and the extra matrix entries cost about 15 seconds
each. Node 18 is past its upstream end of life; drop it from `engines` and CI
together when that stops being useful.

## API

`Index.build(terms)` and `index.search(query, { k, budget, ranking })`, mirroring
`kh_build` and `kh_search`, with types in `index.d.ts`.

- Errors: argument problems throw `TypeError` or `RangeError` before the engine
  runs (a term with a tab or line break would corrupt the text format; an empty
  dictionary, a query over 128 code points, a budget over 64 are refused by the
  engine, but a JavaScript error with a message is better than `u32::MAX`).
  `KeyhammerError` covers engine failures that get through, and a missing
  module.
- `search` returns `{ hits, nodesExpanded, truncated }`; `cost` is the engine's
  fixed-point cost (16 = one ordinary edit).
- Differences from the wasm ABI: strings and objects instead of pointers,
  `ranking` as `'coarse'`/`'exact'`, one instance per `Index` (the module holds a
  single index).

## The legacy binding and `bench/compare.mjs`

`bench/compare.mjs` used only `KeyhammerIndex.build(dict, 2)` and
`search(q, 10)` returning terms. The new package covers both, so the legacy
binding is removed and the harness now loads `bindings/node/index.js`. That
changes what its keyhammer row measures: it is now the new engine, not the
legacy one, so its numbers are not comparable with the earlier runs of that
script. The rigorous comparison stays in `bench/js-competitors` and
`docs/benchmarks/competitors-js.md`. Not carried over from the legacy binding:
`buildWithLayout`, `import/export`, `add`, `remove`, `searchWithThreshold`,
`stats` and match ranges, which the new engine does not have.

## Supply chain and CI scope

`bindings/node` has no Rust code and no npm dependencies, so there is nothing
new for `cargo deny` or Dependabot to cover. `crates/node`, which was an excluded
manifest with its own lockfile, is gone; the `exclude` list in `Cargo.toml` and
the notes in `deny.toml` and `dependabot.yml` are updated. Issue #54 still
applies to `crates/keyhammer/fuzz` and `bench/competitors`.

## Not done

- Nothing is published: `package.json` has `"private": true`. Before publishing
  the package would need a `LICENSE` file in the package directory and a
  decision on the package name, on provenance, and on an ESM-only package (no
  `require` entry point; recent Node versions can `require` ES modules, older
  ones cannot).
- No type-check of `index.d.ts` in CI (no TypeScript dependency added); it was
  written by hand against `index.js`.
- No native addon prototype and therefore no latency figure for one.
- The latency of this package was not benchmarked separately from the wasm
  numbers above.
