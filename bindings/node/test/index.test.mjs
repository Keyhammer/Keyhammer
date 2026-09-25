// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel
//
// Build the module first: npm run build:wasm (or put a build at keyhammer.wasm).
// Run: node --test test/
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { test } from 'node:test';
import { COST_PER_EDIT, Index, KeyhammerError, MAX_BUDGET, MAX_QUERY_LENGTH } from '../index.js';

const dict = [
  ['javascript', 10],
  ['typescript', 10],
  ['python', 10],
  ['rust', 10],
  ['java', 10],
  { term: 'Swift', weight: 7 }, // case is folded for matching; the hit returns 'Swift'
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
  assert.deepEqual(index.search('SWIFT').hits[0], { term: 'Swift', cost: 0, weight: 7 });
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
  assert.throws(() => index.search('a'.repeat(MAX_QUERY_LENGTH + 1)), RangeError);
  assert.doesNotThrow(() => index.search('a'.repeat(MAX_QUERY_LENGTH)));
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

test('the constructor is private', () => {
  assert.throws(() => new Index(), TypeError);
  assert.throws(() => new Index({}, {}, 1), TypeError);
});

test('surrounding white space is rejected, not trimmed', () => {
  for (const bad of [' foo', 'foo ', '\u0085', '\u00a0a', 'a\u3000', ' ']) {
    assert.throws(() => Index.build([bad]), RangeError, JSON.stringify(bad));
  }
  assert.throws(() => Index.build(['\u0085', 'a']), RangeError);
  assert.equal(Index.build(['a b']).size, 1);
  assert.equal(Index.build(['\uFEFFa']).size, 1); // not white space for the engine
});

test('lone surrogates are rejected, pairs are kept', () => {
  for (const bad of ['a\ud800', '\udc00a', 'a\ud800b']) {
    assert.throws(() => Index.build([bad]), TypeError);
    assert.throws(() => Index.build(['ok']).search(bad), TypeError);
  }
  assert.equal(Index.build(['a\u{1F600}']).size, 1);
  assert.doesNotThrow(() => Index.build(['ok']).search('a\u{1F600}'));
});

// ---- Unicode normalisation (issue #67) ----

const testdata = (name) =>
  readFileSync(new URL(`../../testdata/${name}`, import.meta.url), 'utf8').split(/\r?\n/).filter((l) => l.length > 0);

const PT = [
  ['São Paulo', 9],
  ['coração', 5],
  ['Ação', 1],
  ['ação', 8],
  ['não', 3],
  ['pé', 2],
  ['ônibus', 4],
  ['Ç', 6],
  ['Straße', 7],
  ['Müller', 3],
  ['Crème Brûlée', 5],
];

test('queries in another case or with other diacritics find the dictionary text', () => {
  const index = Index.build(PT);
  for (const [query, term] of [
    ['sao paulo', 'São Paulo'],
    ['SAO PAULO', 'São Paulo'],
    ['São Paulo', 'São Paulo'],
    ['SÃO PAULO', 'São Paulo'],
    ['ACAO', 'ação'],
    ['AÇÃO', 'ação'],
    ['coracao', 'coração'],
    ['NAO', 'não'],
    ['PE', 'pé'],
    ['onibus', 'ônibus'],
    ['c', 'Ç'],
    ['strasse', 'Straße'],
    ['STRASSE', 'Straße'],
    ['muller', 'Müller'],
    ['creme brulee', 'Crème Brûlée'],
  ]) {
    const h = index.search(query, { ranking: 'exact' }).hits[0];
    assert.deepEqual([h.term, h.cost], [term, 0], query);
  }
  assert.equal(index.search('sao paolo', { ranking: 'exact' }).hits[0].cost, COST_PER_EDIT);
});

test('hits return the original text; terms equal after folding merge', () => {
  const index = Index.build(PT);
  assert.equal(index.size, PT.length - 1); // Ação and ação
  assert.deepEqual(index.search('acao').hits[0], { term: 'ação', cost: 0, weight: 8 });
  assert.equal(index.search('straße').hits[0].term, 'Straße');
  assert.equal(Index.build(['Café']).search('cafe').hits[0].cost, 0); // decomposed accent
});

test('a term that folds to nothing is an error naming the entry', () => {
  assert.throws(() => Index.build(['ok', '\u0301\u0302']), { name: 'RangeError', message: /terms\[1\].*folds to nothing/ });
  assert.throws(() => Index.build(['\u0301']), RangeError);
});

test('the query limit counts code points', () => {
  const index = Index.build(['é']);
  assert.doesNotThrow(() => index.search('é'.repeat(MAX_QUERY_LENGTH)));
  assert.doesNotThrow(() => index.search('\u{1F600}'.repeat(MAX_QUERY_LENGTH)));
  assert.throws(() => index.search('é'.repeat(MAX_QUERY_LENGTH + 1)), RangeError);
  assert.throws(() => index.search('ß'.repeat(65)), RangeError, 'folds to 130 letters');
  // Over the limit as given (200 code points), within it after folding (100).
  assert.doesNotThrow(() => index.search('e\u0301'.repeat(100)));
  assert.throws(() => index.search('a'.repeat(100000)), RangeError);
});

test('results equal the Rust core on the shared dictionary', () => {
  // bindings/testdata/unicode_expected.tsv is produced by the core alone
  // (Trie::build_normalized + Searcher::search_text), see bindings/c tests.
  const items = testdata('unicode_dict.tsv').map((l) => {
    const [term, weight] = l.split('\t');
    return [term, Number(weight)];
  });
  const index = Index.build(items);
  const cases = testdata('unicode_cases.tsv').map((l) => l.split('\t'));
  const golden = readFileSync(new URL('../../testdata/unicode_expected.tsv', import.meta.url), 'utf8')
    .split(/\r?\n/)
    .filter((l) => l.length > 0);
  assert.equal(cases.length, golden.length);
  assert.ok(cases.length > 50);
  const byTerm = new Map(items.map(([t], i) => [t, i]));
  cases.forEach(([query, budget, ranking], i) => {
    const got = index
      .search(query, { budget: Number(budget), ranking })
      .hits.map((h) => `${byTerm.get(h.term)}:${h.cost}:${h.weight}`)
      .join(',');
    assert.equal(got, golden[i].split('\t')[1], `case ${i}: ${query}`);
  });
});
