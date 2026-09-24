// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel
//
// Node.js package over the Keyhammer WebAssembly build (bindings/wasm). It is
// plain JavaScript over the module's C-ABI: no native addon, no generated glue
// and no dependencies. See docs/design/node-binding.md for why.
import { readFileSync } from 'node:fs';

/** Cost of one ordinary edit in the engine's fixed-point costs. */
export const COST_PER_EDIT = 16;

/** Largest accepted query, in UTF-8 bytes. */
export const MAX_QUERY_BYTES = 128;

/** Largest accepted budget (a limit of the core, `SearchError::BudgetTooLarge`). */
export const MAX_BUDGET = 64;

const MAX_TERM_BYTES = 65535;
const ERROR = 0xffffffff;
const RANKINGS = { coarse: 0, exact: 1 };

const encoder = new TextEncoder();
const decoder = new TextDecoder('utf-8', { fatal: true });

/** Error raised when the engine reports a failure that the argument checks did not catch. */
export class KeyhammerError extends Error {
  constructor(message, options) {
    super(message, options);
    this.name = 'KeyhammerError';
  }
}

let compiled;
function compile() {
  if (compiled) return compiled;
  let bytes;
  try {
    bytes = readFileSync(new URL('./keyhammer.wasm', import.meta.url));
  } catch (cause) {
    throw new KeyhammerError(
      'keyhammer.wasm not found next to index.js; build it with `npm run build:wasm` (see README.md)',
      { cause },
    );
  }
  compiled = new WebAssembly.Module(bytes);
  return compiled;
}

function isU(n, max) {
  return Number.isInteger(n) && n >= 0 && n <= max;
}

// One dictionary entry as a line of the module's text format.
function line(entry, i) {
  let term = entry;
  let weight = 0;
  if (Array.isArray(entry)) {
    [term, weight = 0] = entry;
  } else if (entry !== null && typeof entry === 'object') {
    ({ term, weight = 0 } = entry);
  }
  if (typeof term !== 'string') {
    throw new TypeError(`terms[${i}]: the term must be a string`);
  }
  if (/[\t\r\n]/.test(term)) {
    throw new RangeError(`terms[${i}]: a term must not contain a tab or a line break`);
  }
  if (term.trim().length === 0) {
    throw new RangeError(`terms[${i}]: a term must not be empty`);
  }
  if (encoder.encode(term).length > MAX_TERM_BYTES) {
    throw new RangeError(`terms[${i}]: a term is limited to ${MAX_TERM_BYTES} UTF-8 bytes`);
  }
  if (!isU(weight, 0xffff)) {
    throw new RangeError(`terms[${i}]: the weight must be an integer from 0 to 65535`);
  }
  return `${term}\t${weight}`;
}

/**
 * A search index over a dictionary. Every index owns one WebAssembly instance
 * (and therefore its own linear memory, which never shrinks); it is released by
 * the garbage collector when the index is no longer referenced.
 */
export class Index {
  #kh;
  #size;

  constructor(kh, size) {
    this.#kh = kh;
    this.#size = size;
  }

  /**
   * Builds an index. `terms` is an iterable of strings, `[term, weight]` pairs
   * or `{ term, weight }` objects (weight: integer 0 to 65535, default 0).
   * ASCII letters are lower-cased. Duplicates keep the highest weight.
   */
  static build(terms) {
    if (terms === null || typeof terms !== 'object' || typeof terms[Symbol.iterator] !== 'function') {
      throw new TypeError('terms must be an iterable of strings, [term, weight] pairs or { term, weight } objects');
    }
    const lines = [];
    let i = 0;
    for (const entry of terms) lines.push(line(entry, i++));
    if (lines.length === 0) throw new RangeError('terms must not be empty');
    const kh = new WebAssembly.Instance(compile(), {}).exports;
    const data = encoder.encode(lines.join('\n'));
    const n = withBuffer(kh, data, (ptr, len) => kh.kh_build(ptr, len)) >>> 0;
    if (n === 0) throw new KeyhammerError('the engine could not build an index from these terms');
    return new Index(kh, n);
  }

  /** Number of distinct terms in the index. */
  get size() {
    return this.#size;
  }

  /**
   * Searches for `query` (ASCII letters are lower-cased). Options: `k`, the
   * number of hits (default 10); `budget`, an integer from 0 to 64 (default
   * 32, about two edits; 48 is the high-recall setting); `ranking`, `'coarse'`
   * (default) or `'exact'`. Returns the hits, best first, with the number of
   * trie nodes expanded and whether the node limit stopped the search early.
   * A hit's `cost` is fixed-point: `COST_PER_EDIT` (16) is one ordinary edit.
   */
  search(query, options = {}) {
    if (typeof query !== 'string') throw new TypeError('query must be a string');
    if (options === null || typeof options !== 'object') throw new TypeError('options must be an object');
    const { k = 10, budget = 32, ranking = 'coarse' } = options;
    if (!isU(k, 0xffffffff)) throw new RangeError('k must be a non-negative integer');
    if (!isU(budget, MAX_BUDGET)) throw new RangeError(`budget must be an integer from 0 to ${MAX_BUDGET}`);
    if (!Object.hasOwn(RANKINGS, ranking)) throw new TypeError("ranking must be 'coarse' or 'exact'");
    const data = encoder.encode(query);
    if (data.length > MAX_QUERY_BYTES) {
      throw new RangeError(`query is limited to ${MAX_QUERY_BYTES} UTF-8 bytes`);
    }
    const kh = this.#kh;
    const n = withBuffer(kh, data, (ptr, len) => kh.kh_search(ptr, len, k, budget, RANKINGS[ranking])) >>> 0;
    if (n === ERROR) throw new KeyhammerError('the engine rejected the search');
    const text = decoder.decode(new Uint8Array(kh.memory.buffer, kh.kh_results_ptr(), kh.kh_results_len()));
    const [header, ...rows] = text.split('\n');
    const [nodes, truncated] = header.split('\t');
    const hits = [];
    for (const row of rows) {
      if (row.length === 0) continue;
      const [term, cost, weight] = row.split('\t');
      hits.push({ term, cost: Number(cost), weight: Number(weight) });
    }
    return { hits, nodesExpanded: Number(nodes), truncated: truncated === '1' };
  }
}

// Copies `data` into a fresh linear-memory buffer for the duration of `f`.
function withBuffer(kh, data, f) {
  const ptr = kh.kh_alloc(data.length);
  if (data.length > 0 && ptr === 0) throw new KeyhammerError('out of WebAssembly memory');
  new Uint8Array(kh.memory.buffer, ptr, data.length).set(data);
  try {
    return f(ptr, data.length);
  } finally {
    kh.kh_free(ptr, data.length);
  }
}
