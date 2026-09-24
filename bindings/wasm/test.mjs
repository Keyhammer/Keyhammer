// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel
//
// Tests the WebAssembly build through its C-ABI, with no dependencies.
// Build first, from the repository root:
//   cargo build -p keyhammer-wasm --target wasm32-unknown-unknown --profile wasm
// then run: node bindings/wasm/test.mjs [path/to/keyhammer_wasm.wasm]
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { gzipSync } from 'node:zlib';

const repo = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const wasmPath =
  process.argv[2] ?? join(repo, 'target', 'wasm32-unknown-unknown', 'wasm', 'keyhammer_wasm.wasm');

const bytes = readFileSync(wasmPath);
const { instance, module } = await WebAssembly.instantiate(bytes, {});
const kh = instance.exports;
const ERR = 0xffffffff;
const enc = new TextEncoder();
const dec = new TextDecoder();

let passed = 0;
const failures = [];
function check(name, cond, detail = '') {
  if (cond) passed++;
  else failures.push(`${name}${detail ? `: ${detail}` : ''}`);
}

function withBuffer(text, f) {
  const data = enc.encode(text);
  const ptr = kh.kh_alloc(data.length);
  if (data.length > 0 && ptr === 0) throw new Error('kh_alloc failed');
  new Uint8Array(kh.memory.buffer, ptr, data.length).set(data);
  try {
    return f(ptr, data.length);
  } finally {
    kh.kh_free(ptr, data.length);
  }
}

const build = (text) => withBuffer(text, (p, n) => kh.kh_build(p, n));

function search(q, { k = 10, budget = 32, ranking = 0 } = {}) {
  const n = withBuffer(q, (p, len) => kh.kh_search(p, len, k, budget, ranking)) >>> 0;
  const text = dec.decode(new Uint8Array(kh.memory.buffer, kh.kh_results_ptr(), kh.kh_results_len()));
  if (n === ERR) return { n, text };
  const [header, ...lines] = text.split('\n').filter((l) => l.length > 0);
  const [nodes, truncated] = header.split('\t').map(Number);
  const hits = lines.map((l) => {
    const [term, cost, weight] = l.split('\t');
    return { term, cost: Number(cost), weight: Number(weight) };
  });
  return { n, hits, nodes, truncated };
}

// No imports: the module is self-contained.
check('no imports', WebAssembly.Module.imports(module).length === 0);

// Searching before any build is an error.
check('search without index', search('java').n === ERR);

const dict = [
  'javascript\t10',
  'typescript\t10',
  'python\t10',
  'rust\t10',
  'java\t10',
  'Swift\t7', // upper case is folded
  'form\t5',
  'from\t9',
  'dog\t3',
  '', // skipped: empty
  'bad\tweight', // skipped: weight is not a u16
  'java\t20', // duplicate: highest weight kept
  'go', // weight defaults to 0
].join('\r\n');
const loaded = build(dict);
check('build count', loaded === 10, `got ${loaded}`);

for (const ranking of [0, 1]) {
  const r = search('javasript', { ranking });
  check(`javasript ranking ${ranking}`, r.hits[0]?.term === 'javascript' && r.hits[0]?.cost === 16, JSON.stringify(r.hits?.[0]));
  check(`count matches ranking ${ranking}`, r.n === r.hits.length);
  check(`nodes header ranking ${ranking}`, r.nodes > 0 && r.truncated === 0);
}

// Transposition: "fomr" swaps the last two letters of "form".
const t = search('fomr', { ranking: 1 });
check('transposition', t.hits[0]?.term === 'form' && t.hits[0].cost === 12, JSON.stringify(t.hits[0]));

// Neighbouring-key substitution on the first byte: 'f' is next to 'd' on
// QWERTY (8, times 1.5 on the first byte = 12).
const s = search('fog', { ranking: 1 });
check('neighbouring key', s.hits[0]?.term === 'dog' && s.hits[0].cost === 12, JSON.stringify(s.hits));

// Case folding of the query, weight and default weight.
const sw = search('SWIFT');
check('query case folding', sw.hits[0]?.term === 'swift' && sw.hits[0].cost === 0 && sw.hits[0].weight === 7);
check('duplicate keeps highest weight', search('java').hits[0]?.weight === 20);
check('default weight 0', search('go').hits[0]?.weight === 0);

// k limits the hits.
check('k = 1', search('java', { k: 1 }).n === 1);
check('k = 0', search('java', { k: 0 }).n === 0);

// Errors return u32::MAX and clear the results text.
check('budget 48 accepted', search('javasript', { budget: 48 }).hits[0]?.term === 'javascript');
check('budget too large', search('java', { budget: 1000 }).n === ERR);
check('budget over u16', search('java', { budget: 70000 }).n === ERR);
check('query too long', search('a'.repeat(129)).n === ERR);
check('bad ranking', search('java', { ranking: 2 }).n === ERR);
check('empty text after error', kh.kh_results_len() === 0);
check('query at limit', search('a'.repeat(128)).n !== ERR);
check('invalid utf-8 query', (() => {
  const p = kh.kh_alloc(1);
  new Uint8Array(kh.memory.buffer, p, 1)[0] = 0xff;
  const r = kh.kh_search(p, 1, 10, 32, 0) >>> 0;
  kh.kh_free(p, 1);
  return r === ERR;
})());
check('null pointer with length', (kh.kh_search(0, 5, 10, 32, 0) >>> 0) === ERR);
check('empty query', search('').n !== ERR);

// Empty dictionaries and invalid text fail and drop the index.
check('empty dictionary', build('') === 0);
check('no index after empty build', search('java').n === ERR);
check('only blank lines', build('\n\n  \n') === 0);
check('huge alloc fails cleanly', kh.kh_alloc(0xffffffff) === 0);
check('long line skipped', build(`${'a'.repeat(70000)}\nok`) === 1);

// Repeated build/search cycles must not grow memory without bound.
const big = Array.from({ length: 2000 }, (_, i) => `term${i.toString(36)}x\t${i % 100}`).join('\n');
const cycle = () => {
  build(big);
  for (const q of ['termax', 'trem1x', 'term', 'zzzz', 'javasript']) search(q, { budget: 48 });
};
for (let i = 0; i < 20; i++) cycle();
const before = kh.memory.buffer.byteLength;
for (let i = 0; i < 300; i++) cycle();
const after = kh.memory.buffer.byteLength;
check('memory stable', after === before, `${before} -> ${after}`);

// The module is served publicly: it must not contain build-machine paths
// (panic locations embed source paths unless they are remapped, see
// README.md). Set KH_ALLOW_PATHS=1 to skip this check for a plain local build.
const strings = bytes.toString('latin1').match(/[\x20-\x7e]{4,}/g) ?? [];
const pathLike = [...new Set(strings.filter((s) => /\.rs\b|[\\/]src[\\/]/.test(s)))];
const leaking = strings.filter((s) => /[A-Za-z]:\\|\/Users\/|\/home\/|\/c\/Users/.test(s));
if (process.env.KH_ALLOW_PATHS) {
  console.log(`path check skipped (KH_ALLOW_PATHS set): ${leaking.length} machine path(s)`);
} else {
  check('no build-machine paths', leaking.length === 0, leaking.slice(0, 5).join(' | '));
}
console.log(`path-like strings left (${pathLike.length}):`);
for (const s of pathLike) console.log(`  ${s}`);

const gz = gzipSync(bytes, { level: 9 }).length;
console.log(`wasm size: ${bytes.length} bytes raw, ${gz} bytes gzip (level 9)`);
if (failures.length > 0) {
  for (const f of failures) console.error(`FAIL ${f}`);
  console.log(`keyhammer-wasm: ${passed} passed, ${failures.length} failed`);
  process.exit(1);
}
console.log(`keyhammer-wasm: ${passed} passed, 0 failed`);
