// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

/** Cost of one ordinary edit in the engine's fixed-point costs. */
export const COST_PER_EDIT: 16;
/** Largest accepted query, in code points counted after case and diacritic folding (by the engine; `ß` counts as two, a decomposed accent as one letter). Input over 4 x this many UTF-8 bytes is refused at once. */
export const MAX_QUERY_LENGTH: 128;
/** Largest accepted budget. */
export const MAX_BUDGET: 64;

/** A dictionary entry: a term, a `[term, weight]` pair or a `{ term, weight }` object. */
export type Entry = string | readonly [term: string, weight?: number] | { term: string; weight?: number };

export interface SearchOptions {
  /** Number of hits to return. Default 10. */
  k?: number;
  /** Cost budget, an integer from 0 to 64. Default 32 (about two edits); 48 is the high-recall setting. */
  budget?: number;
  /** `'coarse'` (default) or `'exact'`. */
  ranking?: 'coarse' | 'exact';
}

export interface Hit {
  /** The dictionary term as it was given to `Index.build` (original case and accents), not the folded form. */
  term: string;
  /** Fixed-point cost: `COST_PER_EDIT` (16) is one ordinary edit. */
  cost: number;
  /** The term's weight (0 to 65535). */
  weight: number;
}

export interface SearchResult {
  /** Hits, best first. */
  hits: Hit[];
  /** Trie nodes expanded by the search. */
  nodesExpanded: number;
  /** True if the node limit stopped the search early. */
  truncated: boolean;
}

/** Thrown when the engine reports a failure that the argument checks did not catch. */
export class KeyhammerError extends Error {
  readonly name: 'KeyhammerError';
}

export class Index {
  private constructor();
  /**
   * Builds an index. Throws `TypeError` or `RangeError` for invalid entries or
   * an empty dictionary, and `KeyhammerError` if the engine fails.
   */
  static build(terms: Iterable<Entry>): Index;
  /** Number of distinct terms. */
  readonly size: number;
  /**
   * Searches for `query`. Throws `TypeError` or `RangeError` for invalid
   * arguments (query over 128 code points, budget over 64, unknown ranking)
   * and `KeyhammerError` if the engine rejects the search.
   */
  search(query: string, options?: SearchOptions): SearchResult;
}
