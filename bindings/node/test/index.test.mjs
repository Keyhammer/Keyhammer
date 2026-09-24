// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel
//
// Build the module first: npm run build:wasm (or put a build at keyhammer.wasm).
// Run: node --test test/
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { test } from 'node:test';
import { COST_PER_EDIT, Index, KeyhammerError, MAX_BUDGET, MAX_QUERY_BYTES } from '../index.js';

const dict = [
  ['javascript', 10],
  ['typescript', 10],
  ['python', 10],
  ['rust', 10],
  ['java', 10],
  { term: 'Swift', weight: 7 }, // upper case is folded
  ['form', 5],
  ['from', 9],
  ['dog', 3],
  ['java', 20], // duplicate: highest weight kept
  'go', // weight defaults to 0
];

test('the wasm module is in place', () => {
  assert.ok(existsSync(new URL('../keyhammer.wasm', import.meta.url)), 'run npm run build:wasm');
});

test('build counts distinct terms', () => {
  const index = Index.build(dict);
  assert.equal(index.size, 10);
});

test('build accepts any iterable', () => {
  assert.equal(Index.build(new Set(['alpha', 'beta'])).size, 2);
  assert.equal(Index.build((function* () { yield 'alpha'; })()).size, 1);
});

for (const ranking of ['coarse', 'exact']) {
  test(`a typo finds the word (${ranking})`, () => {
    const r = Index.build(dict).search('javasript', { ranking });
    assert.equal(r.hits[0].term, 'javascript');
    assert.equal(r.hits[0].cost, COST_PER_EDIT);
    assert.ok(r.nodesExpanded > 0);
    assert.equal(r.truncated, false);
  });
}

test('transposition and neighbouring-key costs', () => {
  const index = Index.build(dict);
  const t = index.search('fomr', { ranking: 'exact' }).hits[0];
  assert.deepEqual([t.term, t.cost], ['form', 12]);
  // f is next to d on QWERTY: 8, times 1.5 on the first byte.
  const s = index.search('fog', { ranking: 'exact' }).hits[0];
  assert.deepEqual([s.term, s.cost], ['dog', 12]);
});

test('query case folding, weights', () => {
  const index = Index.build(dict);
  assert.deepEqual(index.search('SWIFT').hits[0], { term: 'swift', cost: 0, weight: 7 });
  assert.equal(index.search('java').hits[0].weight, 20);
  assert.equal(index.search('go').hits[0].weight, 0);
});

test('k limits the hits', () => {
  const index = Index.build(dict);
  assert.equal(index.search('java', { k: 1 }).hits.length, 1);
  assert.equal(index.search('java', { k: 0 }).hits.length, 0);
});

test('budget: 48 (high recall) and 64 are accepted, 65 is not', () => {
  const index = Index.build(dict);
  assert.equal(index.search('javasript', { budget: 48 }).hits[0].term, 'javascript');
  assert.doesNotThrow(() => index.search('java', { budget: MAX_BUDGET }));
  assert.throws(() => index.search('java', { budget: MAX_BUDGET + 1 }), RangeError);
});

test('search argument errors', () => {
  const index = Index.build(dict);
  assert.throws(() => index.search(42), TypeError);
  assert.throws(() => index.search('java', null), TypeError);
  assert.throws(() => index.search('java', { ranking: 'fancy' }), TypeError);
  assert.throws(() => index.search('java', { ranking: 'toString' }), TypeError);
  assert.throws(() => index.search('java', { k: -1 }), RangeError);
  assert.throws(() => index.search('java', { k: 1.5 }), RangeError);
  assert.throws(() => index.search('java', { budget: '32' }), RangeError);
  assert.throws(() => index.search('a'.repeat(MAX_QUERY_BYTES + 1)), RangeError);
  assert.throws(() => index.search('é'.repeat(MAX_QUERY_BYTES / 2 + 1)), RangeError, 'limit counts UTF-8 bytes');
  assert.doesNotThrow(() => index.search('a'.repeat(MAX_QUERY_BYTES)));
  assert.doesNotThrow(() => index.search(''));
});

test('build argument errors', () => {
  assert.throws(() => Index.build(), TypeError);
  assert.throws(() => Index.build('abc'), TypeError);
  assert.throws(() => Index.build([]), RangeError);
  assert.throws(() => Index.build([42]), TypeError);
  assert.throws(() => Index.build(['ok', '']), RangeError);
  assert.throws(() => Index.build(['a\tb']), RangeError);
  assert.throws(() => Index.build(['a\nb']), RangeError);
  assert.throws(() => Index.build([['a', -1]]), RangeError);
  assert.throws(() => Index.build([['a', 65536]]), RangeError);
  assert.throws(() => Index.build([['a', 1.5]]), RangeError);
  assert.throws(() => Index.build(['x'.repeat(65536)]), RangeError);
  assert.equal(Index.build([['a', 65535]]).size, 1);
});

test('KeyhammerError is an Error with its own name', () => {
  const e = new KeyhammerError('x');
  assert.ok(e instanceof Error);
  assert.equal(e.name, 'KeyhammerError');
});

test('indexes are independent', () => {
  const a = Index.build(['alpha']);
  const b = Index.build(['beta']);
  assert.equal(a.search('alpha').hits[0].term, 'alpha');
  assert.equal(b.search('alpha').hits.some((h) => h.term === 'alpha'), false);
  assert.equal(b.search('beta').hits[0].term, 'beta');
});

test('a larger dictionary and memory growth do not break results', () => {
  const terms = [];
  for (let i = 0; i < 20000; i++) terms.push([`term${i.toString(36)}x`, i % 100]);
  const index = Index.build(terms);
  assert.equal(index.size, 20000);
  for (let i = 0; i < 300; i++) index.search('termax', { budget: 48 });
  assert.equal(index.search('termax').hits[0].term, 'termax');
});

test('the module carries no build-machine path', () => {
  const bytes = readFileSync(new URL('../keyhammer.wasm', import.meta.url));
  const strings = bytes.toString('latin1').match(/[\x20-\x7e]{4,}/g) ?? [];
  const leaking = strings.filter((s) => /[A-Za-z]:\\|\/Users\/|\/home\/|\/c\/Users/.test(s));
  assert.deepEqual(leaking, []);
});
