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
#[non_exhaustive]
pub enum BuildError {
    /// No terms were given.
    Empty,
    /// A term was the empty string.
    EmptyTerm,
    /// A term is longer than 65535 bytes.
    TermTooLong,
    /// More than `u32::MAX` terms were given: term ids and input indices are
    /// `u32`, and `u32::MAX` itself is reserved for [`NO_TERM`].
    TooManyTerms,
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BuildError::Empty => f.write_str("cannot build an index from an empty term list"),
            BuildError::EmptyTerm => f.write_str("terms must not be empty"),
            BuildError::TermTooLong => f.write_str("terms must be at most 65535 bytes"),
            BuildError::TooManyTerms => f.write_str("an index holds at most 4294967295 terms"),
        }
    }
}

/// Converts a term position or input index to `u32`, refusing values that do
/// not fit or that would equal [`NO_TERM`].
fn index_u32(i: usize) -> Result<u32, BuildError> {
    match u32::try_from(i) {
        Ok(v) if v != NO_TERM => Ok(v),
        _ => Err(BuildError::TooManyTerms),
    }
}

/// A trie over byte strings with a weight per term.
#[derive(Clone, Debug)]
pub struct Trie {
    terms: Vec<String>,
    weights: Vec<u16>,
    input_index: Vec<u32>,
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
    /// Builds a trie. Duplicate terms are merged, keeping the entry with the
    /// highest weight (the first one among equal weights).
    ///
    /// Term ids are positions in the byte-sorted, deduplicated list, not
    /// positions in `items`; [`Trie::input_index`] maps an id back to the
    /// index in `items` of the entry that was kept.
    pub fn build(items: &[(&str, u16)]) -> Result<Trie, BuildError> {
        if items.is_empty() {
            return Err(BuildError::Empty);
        }
        for (t, _) in items {
            if t.is_empty() {
                return Err(BuildError::EmptyTerm);
            }
            if t.len() > usize::from(u16::MAX) {
                return Err(BuildError::TermTooLong);
            }
        }
        // (term, weight, input index)
        let mut pairs: Vec<(&str, u16, u32)> = items
            .iter()
            .enumerate()
            .map(|(i, &(t, w))| Ok((t, w, index_u32(i)?)))
            .collect::<Result<_, BuildError>>()?;
        pairs.sort_by(|a, b| {
            a.0.as_bytes()
                .cmp(b.0.as_bytes())
                .then(b.1.cmp(&a.1))
                .then(a.2.cmp(&b.2))
        });
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
                term_id[node] = index_u32(lo)?;
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
        let input_index = pairs.iter().map(|p| p.2).collect();
        Ok(Trie {
            terms,
            weights,
            input_index,
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

    /// The index, in the slice given to [`Trie::build`], of the entry kept
    /// for the term with the given id: for duplicates, the entry with the
    /// highest weight, and the first one among equal weights. Returns
    /// `u32::MAX` for an out-of-range id.
    pub fn input_index(&self, id: u32) -> u32 {
        self.input_index
            .get(id as usize)
            .copied()
            .unwrap_or(u32::MAX)
    }

    /// The byte on the edge leading into node `v` (0 for the root).
    ///
    /// # Panics
    ///
    /// Panics if `v` is not a node index, that is, if `v >= self.node_count()`.
    /// Unlike [`Trie::term`], [`Trie::weight`] and [`Trie::input_index`], which
    /// answer for an unknown id, the per-node accessors index directly.
    ///
    /// # Examples
    ///
    /// ```
    /// use keyhammer::trie::{NO_TERM, Trie};
    /// let trie = Trie::build(&[("car", 9), ("cat", 4)]).unwrap();
    /// // Nodes in breadth-first order: root, c, a, r, t.
    /// assert_eq!(trie.node_count(), 5);
    /// assert_eq!(trie.label(0), 0);
    /// assert_eq!(trie.label(1), b'c');
    /// assert_eq!(trie.label(4), b't');
    /// ```
    ///
    /// An index past the last node panics:
    ///
    /// ```should_panic
    /// # use keyhammer::trie::Trie;
    /// let trie = Trie::build(&[("a", 1)]).unwrap();
    /// trie.label(trie.node_count());
    /// ```
    pub fn label(&self, v: usize) -> u8 {
        self.label[v]
    }

    /// The node indices of the children of `v`.
    ///
    /// # Panics
    ///
    /// Panics if `v` is not a node index, that is, if `v >= self.node_count()`.
    /// Unlike [`Trie::term`], [`Trie::weight`] and [`Trie::input_index`], which
    /// answer for an unknown id, the per-node accessors index directly.
    ///
    /// # Examples
    ///
    /// ```
    /// use keyhammer::trie::{NO_TERM, Trie};
    /// let trie = Trie::build(&[("car", 9), ("cat", 4)]).unwrap();
    /// // Nodes in breadth-first order: root, c, a, r, t.
    /// assert_eq!(trie.node_count(), 5);
    /// assert_eq!(trie.children(0), 1..2);
    /// assert_eq!(trie.children(2), 3..5); // a -> r, t
    /// assert!(trie.children(3).is_empty());
    /// ```
    pub fn children(&self, v: usize) -> Range<usize> {
        let start = self.child_start[v] as usize;
        start..start + usize::from(self.child_count[v])
    }

    /// The id of the term that ends at `v`, or [`NO_TERM`].
    ///
    /// # Panics
    ///
    /// Panics if `v` is not a node index, that is, if `v >= self.node_count()`.
    /// Unlike [`Trie::term`], [`Trie::weight`] and [`Trie::input_index`], which
    /// answer for an unknown id, the per-node accessors index directly.
    ///
    /// # Examples
    ///
    /// ```
    /// use keyhammer::trie::{NO_TERM, Trie};
    /// let trie = Trie::build(&[("car", 9), ("cat", 4)]).unwrap();
    /// // Nodes in breadth-first order: root, c, a, r, t.
    /// assert_eq!(trie.node_count(), 5);
    /// assert_eq!(trie.term_id(0), NO_TERM);
    /// assert_eq!(trie.term_id(3), 0); // "car"
    /// assert_eq!(trie.term_id(4), 1); // "cat"
    /// ```
    pub fn term_id(&self, v: usize) -> u32 {
        self.term_id[v]
    }

    /// The largest weight among the terms at or below `v`.
    ///
    /// # Panics
    ///
    /// Panics if `v` is not a node index, that is, if `v >= self.node_count()`.
    /// Unlike [`Trie::term`], [`Trie::weight`] and [`Trie::input_index`], which
    /// answer for an unknown id, the per-node accessors index directly.
    ///
    /// # Examples
    ///
    /// ```
    /// use keyhammer::trie::{NO_TERM, Trie};
    /// let trie = Trie::build(&[("car", 9), ("cat", 4)]).unwrap();
    /// // Nodes in breadth-first order: root, c, a, r, t.
    /// assert_eq!(trie.node_count(), 5);
    /// assert_eq!(trie.max_weight(0), 9);
    /// assert_eq!(trie.max_weight(4), 4);
    /// ```
    pub fn max_weight(&self, v: usize) -> u16 {
        self.max_weight[v]
    }

    /// The length of the shortest term at or below `v`.
    ///
    /// # Panics
    ///
    /// Panics if `v` is not a node index, that is, if `v >= self.node_count()`.
    /// Unlike [`Trie::term`], [`Trie::weight`] and [`Trie::input_index`], which
    /// answer for an unknown id, the per-node accessors index directly.
    ///
    /// # Examples
    ///
    /// ```
    /// use keyhammer::trie::Trie;
    /// let trie = Trie::build(&[("a", 1), ("abc", 1)]).unwrap();
    /// assert_eq!(trie.len_min(0), 1);
    /// assert_eq!(trie.len_min(trie.children(0).start), 1);
    /// ```
    pub fn len_min(&self, v: usize) -> u16 {
        self.len_min[v]
    }

    /// The length of the longest term at or below `v`.
    ///
    /// # Panics
    ///
    /// Panics if `v` is not a node index, that is, if `v >= self.node_count()`.
    /// Unlike [`Trie::term`], [`Trie::weight`] and [`Trie::input_index`], which
    /// answer for an unknown id, the per-node accessors index directly.
    ///
    /// # Examples
    ///
    /// ```
    /// use keyhammer::trie::Trie;
    /// let trie = Trie::build(&[("a", 1), ("abc", 1)]).unwrap();
    /// assert_eq!(trie.len_max(0), 3);
    /// let a = trie.children(0).start;
    /// assert_eq!(trie.len_max(a), 3);
    /// ```
    pub fn len_max(&self, v: usize) -> u16 {
        self.len_max[v]
    }

    /// The character classes that appear on edges strictly below `v`.
    ///
    /// # Panics
    ///
    /// Panics if `v` is not a node index, that is, if `v >= self.node_count()`.
    /// Unlike [`Trie::term`], [`Trie::weight`] and [`Trie::input_index`], which
    /// answer for an unknown id, the per-node accessors index directly.
    ///
    /// # Examples
    ///
    /// ```
    /// use keyhammer::cost::class;
    /// use keyhammer::trie::Trie;
    /// let trie = Trie::build(&[("ab", 1)]).unwrap();
    /// let a = trie.children(0).start;
    /// assert_eq!(trie.below_mask(0), class(b'a') | class(b'b'));
    /// assert_eq!(trie.below_mask(a), class(b'b')); // not its own edge
    /// let b = trie.children(a).start;
    /// assert_eq!(trie.below_mask(b), 0);
    /// ```
    pub fn below_mask(&self, v: usize) -> u64 {
        self.below_mask[v]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_conversion_refuses_what_does_not_fit_or_equals_no_term() {
        assert_eq!(index_u32(0), Ok(0));
        assert_eq!(index_u32(NO_TERM as usize - 1), Ok(NO_TERM - 1));
        assert_eq!(index_u32(NO_TERM as usize), Err(BuildError::TooManyTerms));
        if let Some(big) = (NO_TERM as usize).checked_add(1) {
            assert_eq!(index_u32(big), Err(BuildError::TooManyTerms));
        }
    }
}
