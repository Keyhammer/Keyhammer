# keyhammer (Node.js)

Node.js package for the Keyhammer engine. It is plain JavaScript over the
WebAssembly build in `bindings/wasm`: no native addon, no dependencies, one
`.wasm` file of about 39 KB (about 17 KB gzip). Status: unpublished prototype
(`"private": true`); the interface may change at any time. The reasoning is in
[`docs/design/node-binding.md`](../../docs/design/node-binding.md).

```js
import { Index } from 'keyhammer';

// Terms are strings, [term, weight] pairs or { term, weight } objects
// (weight 0 to 65535, default 0; ties in cost are broken by higher weight).
const index = Index.build([['javascript', 10], ['typescript', 10], 'python']);

const { hits, nodesExpanded, truncated } = index.search('javasript', {
  k: 10,             // number of hits, default 10
  budget: 32,        // 0 to 64, default 32 (about two edits); 48 is the high-recall setting
  ranking: 'coarse', // or 'exact'
});
// hits: [{ term: 'javascript', cost: 16, weight: 10 }]  (cost 16 = one ordinary edit)
```

## Errors

Invalid arguments throw `TypeError` (wrong type, unknown `ranking`) or
`RangeError` (empty dictionary, empty term or a term with a tab or line break,
weight outside 0 to 65535, `k` not a non-negative integer, `budget` above 64,
query over 128 UTF-8 bytes) before the engine is called. `KeyhammerError` is
for what still comes back as a failure from the engine, and for a missing
`keyhammer.wasm`. The mapping from the module's `u32::MAX` and `0` returns is
in `index.js`.

## Build and test

From the repository root, with the `wasm32-unknown-unknown` target installed
(`rustup target add wasm32-unknown-unknown`):

```bash
cd bindings/node
npm run build:wasm   # builds bindings/wasm with the project's flags, copies keyhammer.wasm here
npm test             # node --test
```

`keyhammer.wasm` is not committed. CI builds it once and tests that same file
on Linux, macOS and Windows with Node 18 and 22.

## Limits

Terms and queries with lone UTF-16 surrogates are rejected (`TypeError`), and a
term must not start or end with white space (`RangeError`), so nothing is
changed silently.

Those of the WebAssembly build (`bindings/wasm/README.md`): lowercase ASCII
`a-z` only (other bytes are compared verbatim, no Unicode folding), provisional
costs, no `tsb` option (the subtree bound is off), and a linear memory that
does not shrink. Every `Index` has its own WebAssembly instance, so many small
indexes cost more memory than one; searches are synchronous and run on the
calling thread. Node only: the loader reads the file with `node:fs`.
