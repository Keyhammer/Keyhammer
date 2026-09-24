---
title: How it works
sidebar_position: 2
slug: /how-it-works
custom_edit_url: https://github.com/Keyhammer/Keyhammer/edit/main/website/content/how-it-works.md
---

# How it works

This page describes the M0 prototype in `crates/keyhammer/src` (`trie.rs`,
`cost.rs`, `search.rs`). Values quoted here are the ones in the code today.

## Trie layout

`Trie::build` sorts the terms by bytes, merges duplicates (keeping the highest
weight) and builds a flat, breadth-first trie stored as parallel arrays. The
children of a node are contiguous and always have larger indices than the node,
so there are no cycles by construction. Term ids are indices into the sorted,
deduplicated term list.

Besides the edge label, each node stores summaries of the terms at or below it:
the largest weight, the shortest and longest term length, and a 64-bit mask of
the character classes seen on the edges below it (the subtree signature).

## Costs are fixed point

Costs are `u16` integers where 16 is one ordinary edit. Integer costs make the
search bounds exact and the results identical on every platform. `INF` (30000)
marks "unreachable or over budget".

## The cost table

`CostModel::qwerty()` currently uses:

| Edit | Cost |
|---|---|
| Substitution | 16 |
| Substitution with a neighbouring QWERTY key | 8 |
| Insertion or deletion | 16 |
| Insertion or deletion of a doubled letter | 8 |
| Transposition of two adjacent bytes | 12 |
| Any edit at the first query byte | multiplied by 1.5 |

**These values are provisional.** No calibration has been done, and the
[M0 results](/docs/results) list the cost weighting among the untested
hypotheses for why ranking quality is below the baseline.

## Weighted OSA rows in a narrow band

The distance is a weighted optimal string alignment distance: substitutions,
insertions, deletions and transpositions of adjacent bytes. Each queued trie
node carries one row of the edit-distance matrix, restricted to a band around
the diagonal. The band half-width is `W = budget / c_indel_min`, where
`c_indel_min` is the cheapest insertion or deletion; `W` is capped at 8, so
larger budgets are rejected with an error. A cell above the budget is set to
`INF`. Queries are limited to 128 bytes.

## Exact best-first top-k

Nodes and terminals share one priority queue ordered by cost, then by higher
weight, then by term id. A node is queued with a lower bound on the cost of
every term below it. A terminal is queued with its exact cost. Because the
bound never exceeds the true cost, the first terminal popped is always the best
remaining one, so the search stops as soon as `k` terminals have been popped.
The result is exact, and the test suite compares it with a brute-force oracle.

`SearchConfig::max_nodes` is a hard limit on expanded nodes. If it is reached,
`Stats::truncated` is set and the result may be incomplete.

## The subtree signature bound

Without `tsb`, the lower bound of a node is the smallest cell of its row, or
the smallest cell of the parent row plus the cheapest transposition. With
`tsb: true`, each cell is raised by what the rest of the query must still pay,
given what exists below the node:

- a length term: if no term below the node has a length compatible with the
  rest of the query, the gap costs at least the cheapest insertion or deletion
  per byte;
- a letter term: each character class present in the rest of the query but
  absent from the subtree mask costs at least the cheapest edit.

The larger of the two is added. The bound only prunes; it does not change the
hits. On the M0 benchmark it expanded about a third fewer nodes; see the
[results](/docs/results). Every ingredient is known from the literature, and
the project claims no novelty (see [prior art](/docs/prior-art)).
