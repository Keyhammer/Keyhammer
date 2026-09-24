// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

//! A flat, breadth-first trie stored as parallel arrays.
//!
//! Children of a node are contiguous and always have larger indices than the
//! node, so the structure has no cycles by construction.

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;
use core::ops::Range;

use crate::cost::class;

/// Value of [`Trie::term_id`] for nodes where no term ends.
pub const NO_TERM: u32 = u32::MAX;

/// Why a trie could not be built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildError {
    /// No terms were given.
    Empty,
    /// A term was the empty string.
    EmptyTerm,
    /// A term is longer than 65535 bytes.
    TermTooLong,
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BuildError::Empty => f.write_str("cannot build an index from an empty term list"),
            BuildError::EmptyTerm => f.write_str("terms must not be empty"),
            BuildError::TermTooLong => f.write_str("terms must be at most 65535 bytes"),
        }
    }
}

/// A trie over byte strings with a weight per term.
#[derive(Clone, Debug)]
pub struct Trie {
    terms: Vec<String>,
    weights: Vec<u16>,
    label: Vec<u8>,
    child_start: Vec<u32>,
    child_count: Vec<u16>,
    term_id: Vec<u32>,
    max_weight: Vec<u16>,
    len_min: Vec<u16>,
    len_max: Vec<u16>,
    below_mask: Vec<u64>,
}

impl Trie {
    /// Builds a trie. Duplicate terms are merged, keeping the highest weight.
    /// Term ids are indices into the byte-sorted, deduplicated list.
    pub fn build(items: &[(&str, u16)]) -> Result<Trie, BuildError> {
        if items.is_empty() {
            return Err(BuildError::Empty);
        }
        let mut pairs: Vec<(&str, u16)> = items.to_vec();
        for (t, _) in &pairs {
            if t.is_empty() {
                return Err(BuildError::EmptyTerm);
            }
            if t.len() > usize::from(u16::MAX) {
                return Err(BuildError::TermTooLong);
            }
        }
        pairs.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()).then(b.1.cmp(&a.1)));
        pairs.dedup_by(|cur, prev| cur.0 == prev.0);

        let bytes: Vec<&[u8]> = pairs.iter().map(|p| p.0.as_bytes()).collect();
        let mut label = vec![0u8];
        let mut child_start = vec![0u32];
        let mut child_count = vec![0u16];
        let mut term_id = vec![NO_TERM];
        let mut depth_of = vec![0u16];

        let mut queue: VecDeque<(usize, usize, usize, usize)> = VecDeque::new();
        queue.push_back((0, 0, bytes.len(), 0));
        while let Some((node, mut lo, hi, depth)) = queue.pop_front() {
            if bytes[lo].len() == depth {
                term_id[node] = lo as u32;
                lo += 1;
            }
            let first_child = label.len() as u32;
            let mut count = 0u16;
            let mut i = lo;
            while i < hi {
                let b = bytes[i][depth];
                let mut j = i + 1;
                while j < hi && bytes[j][depth] == b {
                    j += 1;
                }
                let child = label.len();
                label.push(b);
                child_start.push(0);
                child_count.push(0);
                term_id.push(NO_TERM);
                depth_of.push((depth + 1) as u16);
                queue.push_back((child, i, j, depth + 1));
                count += 1;
                i = j;
            }
            child_start[node] = first_child;
            child_count[node] = count;
        }

        let weights: Vec<u16> = pairs.iter().map(|p| p.1).collect();
        let n = label.len();
        let mut max_weight = vec![0u16; n];
        let mut len_min = vec![u16::MAX; n];
        let mut len_max = vec![0u16; n];
        let mut below_mask = vec![0u64; n];
        for v in (0..n).rev() {
            if term_id[v] != NO_TERM {
                max_weight[v] = weights[term_id[v] as usize];
                len_min[v] = depth_of[v];
                len_max[v] = depth_of[v];
            }
            let start = child_start[v] as usize;
            for c in start..start + usize::from(child_count[v]) {
                max_weight[v] = max_weight[v].max(max_weight[c]);
                len_min[v] = len_min[v].min(len_min[c]);
                len_max[v] = len_max[v].max(len_max[c]);
                below_mask[v] |= class(label[c]) | below_mask[c];
            }
        }

        let terms = pairs.iter().map(|p| String::from(p.0)).collect();
        Ok(Trie {
            terms,
            weights,
            label,
            child_start,
            child_count,
            term_id,
            max_weight,
            len_min,
            len_max,
            below_mask,
        })
    }

    /// Number of distinct terms.
    pub fn len(&self) -> usize {
        self.terms.len()
    }

    /// Whether the trie holds no terms (never true for a built trie).
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    /// Number of trie nodes, root included.
    pub fn node_count(&self) -> usize {
        self.label.len()
    }

    /// The term with the given id.
    pub fn term(&self, id: u32) -> &str {
        self.terms.get(id as usize).map_or("", String::as_str)
    }

    /// The weight of the term with the given id.
    pub fn weight(&self, id: u32) -> u16 {
        self.weights.get(id as usize).copied().unwrap_or(0)
    }

    /// The byte on the edge leading into node `v` (0 for the root).
    pub fn label(&self, v: usize) -> u8 {
        self.label[v]
    }

    /// The node indices of the children of `v`.
    pub fn children(&self, v: usize) -> Range<usize> {
        let start = self.child_start[v] as usize;
        start..start + usize::from(self.child_count[v])
    }

    /// The id of the term that ends at `v`, or [`NO_TERM`].
    pub fn term_id(&self, v: usize) -> u32 {
        self.term_id[v]
    }

    /// The largest weight among the terms at or below `v`.
    pub fn max_weight(&self, v: usize) -> u16 {
        self.max_weight[v]
    }

    /// The length of the shortest term at or below `v`.
    pub fn len_min(&self, v: usize) -> u16 {
        self.len_min[v]
    }

    /// The length of the longest term at or below `v`.
    pub fn len_max(&self, v: usize) -> u16 {
        self.len_max[v]
    }

    /// The character classes that appear on edges strictly below `v`.
    pub fn below_mask(&self, v: usize) -> u64 {
        self.below_mask[v]
    }
}
