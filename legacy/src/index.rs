// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

/// FuzzyIndex — maximum performance fuzzy search.
///
/// Uses fingerprint-based matching instead of pre-generated deletion variants.
/// Build is fast (no variant generation), memory is low, queries use fingerprints
/// as a cheap pre-filter before doing actual string comparison.

use std::sync::OnceLock;

use crate::altered::AlteredString;
use crate::encoding::TypoEncoding;
use crate::error::{Error, Result};
use crate::fingerprint::{self, Fingerprint};
use crate::scorer::{KeyboardLayout, TypoScorer};
use crate::tree;

/// Threshold above which a lazy CGL tree is built alongside brute force.
/// Below this, only brute force + fingerprint runs.
const TREE_THRESHOLD: usize = 5_000;

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub term: String,
    pub term_index: usize,
    pub score: f32,
    pub hamming_distance: usize,
    /// Character positions where query and term differ (for UI highlighting).
    pub match_ranges: Vec<(usize, usize)>,
}

pub struct FuzzyIndex {
    // ── built eagerly (cheap) ──
    terms: Vec<Vec<u8>>,
    terms_display: Vec<String>,
    term_lengths: Vec<usize>,
    fingerprints: Vec<Fingerprint>,
    scorer: TypoScorer,
    encoding: TypoEncoding,
    k: usize,

    // ── built lazily on first search ──
    col_terms: OnceLock<crate::columnar::ColumnarTerms>,
    encoded_terms: OnceLock<Vec<Vec<u8>>>,
    term_map: OnceLock<std::collections::HashMap<Vec<u8>, Vec<usize>>>,

    // ── lazy CGL tree (built on first search if n >= threshold) ──
    root: OnceLock<Option<tree::CglNode>>,
}

impl FuzzyIndex {
    const MAX_K: usize = 4;

    /// Build with default QWERTY layout.
    pub fn build(terms: &[&str], k: usize) -> Result<Self> {
        Self::build_with_layout(terms, k, KeyboardLayout::Qwerty)
    }

    /// Build with a specific keyboard layout for confusion-aware scoring.
    pub fn build_with_layout(terms: &[&str], k: usize, layout: KeyboardLayout) -> Result<Self> {
        if terms.is_empty() {
            return Err(Error::EmptyInput);
        }
        if k > Self::MAX_K {
            return Err(Error::KTooLarge { k, max: Self::MAX_K });
        }

        let max_len = terms.iter().map(|t| t.len()).max().unwrap_or(0);
        let n = terms.len();

        // ── eager: only the essentials ──
        let terms_owned: Vec<Vec<u8>> = terms.iter()
            .map(|t| t.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect())
            .collect();
        let terms_display: Vec<String> = terms.iter().map(|t| t.to_string()).collect();
        let term_lengths: Vec<usize> = terms_owned.iter().map(|t| t.len()).collect();
        let fingerprints: Vec<Fingerprint> = terms_owned.iter()
            .map(|t| fingerprint::compute(t))
            .collect();
        let scorer = TypoScorer::with_layout(max_len, layout);
        let encoding = TypoEncoding::from_layout(layout);

        Ok(Self {
            terms: terms_owned,
            terms_display,
            term_lengths,
            fingerprints,
            scorer,
            encoding,
            k,
            col_terms: OnceLock::new(),
            encoded_terms: OnceLock::new(),
            term_map: OnceLock::new(),
            root: OnceLock::new(),
        })
    }

    // ── lazy accessors (built on first use) ──

    fn col(&self) -> &crate::columnar::ColumnarTerms {
        self.col_terms.get_or_init(|| crate::columnar::ColumnarTerms::build(&self.terms))
    }

    fn enc_terms(&self) -> &Vec<Vec<u8>> {
        self.encoded_terms.get_or_init(|| {
            self.terms.iter().map(|t| self.encoding.encode_str(t)).collect()
        })
    }

    fn tree(&self) -> Option<&tree::CglNode> {
        self.root.get_or_init(|| {
            let n = self.terms.len();
            if n < TREE_THRESHOLD { return None; }
            let sigma = (((n as f64 + 1.0).log2() / self.k.max(1) as f64).ceil() as usize).max(2);
            let altered: Vec<AlteredString> = self.terms.iter()
                .enumerate()
                .map(|(i, t)| AlteredString::new(t, Some(i)))
                .collect();
            tree::build(altered, self.k, sigma)
        }).as_ref()
    }

    fn tmap(&self) -> &std::collections::HashMap<Vec<u8>, Vec<usize>> {
        self.term_map.get_or_init(|| {
            let mut m = std::collections::HashMap::with_capacity(self.terms.len());
            for (i, term) in self.terms.iter().enumerate() {
                m.entry(term.clone()).or_insert_with(Vec::new).push(i);
            }
            m
        })
    }

    /// Search with a custom sort function.
    pub fn search_with_sort(
        &self,
        query: &str,
        max_results: usize,
        sort_fn: impl FnMut(&SearchResult, &SearchResult) -> std::cmp::Ordering,
    ) -> Result<Vec<SearchResult>> {
        let mut results = self.search(query, max_results * 2)?; // get more, then re-sort
        results.sort_unstable_by(sort_fn);
        results.truncate(max_results);
        Ok(results)
    }

    /// Search with a minimum score threshold. Results below `threshold` are discarded.
    pub fn search_with_threshold(&self, query: &str, max_results: usize, threshold: f32) -> Result<Vec<SearchResult>> {
        let mut results = self.search(query, max_results)?;
        results.retain(|r| r.score >= threshold);
        Ok(results)
    }

    pub fn search(&self, query: &str, max_results: usize) -> Result<Vec<SearchResult>> {
        let q_lower: Vec<u8> = query.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
        let q = &q_lower;
        let q_encoded = self.encoding.encode_str(q);

        // brute force always runs (fast for any n)
        // tree supplements with additional candidates if available
        let mut candidates = self.search_brute(q);
        if let Some(root) = self.tree() {
            let tree_matches = self.search_tree_node(root, q);
            // merge tree candidates that brute force missed
            let brute_set: std::collections::HashSet<usize> = candidates.iter().map(|&(i, _)| i).collect();
            for (i, d) in tree_matches {
                if !brute_set.contains(&i) {
                    candidates.push((i, d));
                }
            }
        }

        let mut seen_idx: Vec<bool> = vec![false; self.terms.len()];
        let mut results: Vec<SearchResult> = candidates.into_iter()
            .map(|(idx, hd)| {
                seen_idx[idx] = true;
                let term = &self.terms[idx];
                // combine two signals:
                // 1. typo scorer (position weight + transposition + confusion matrix)
                let typo_score = self.scorer.score(q, term);
                // 2. encoding distance (bit-level confusion baked into representation)
                let enc_dist = TypoEncoding::normalized_distance(&q_encoded, &self.enc_terms()[idx]);
                // final score: blend both (encoding is a tiebreaker for similar typo scores)
                // field-length normalization: shorter terms score slightly higher
                let len_norm = 1.0 / (1.0 + (term.len() as f32 - q.len() as f32).abs() * 0.05);
                let combined = if hd == 0 && q.len() == term.len() {
                    1.0
                } else {
                    (typo_score.score * 0.7 + (1.0 - enc_dist) * 0.3) * len_norm
                };
                SearchResult {
                    term: self.terms_display[idx].clone(),
                    term_index: idx,
                    score: combined,
                    hamming_distance: hd,
                    match_ranges: compute_match_ranges(q, term),
                }
            })
            .collect();

        // multi-word fallback: if query contains spaces and few results,
        // search for individual tokens and boost terms that match multiple tokens
        if q.contains(&b' ') && results.len() < max_results {
            let tokens: Vec<&[u8]> = q.split(|b| *b == b' ')
                .filter(|t| t.len() >= 2)
                .collect();

            if tokens.len() > 1 {
                // for each term, count how many query tokens fuzzy-match
                for (idx, term) in self.terms.iter().enumerate() {
                    if seen_idx[idx] { continue; }
                    let mut token_matches = 0u32;
                    for token in &tokens {
                        // check if token appears as a fuzzy substring of the term
                        if term.windows(token.len()).any(|w| {
                            let d: usize = w.iter().zip(token.iter())
                                .filter(|(a, b)| a != b).count();
                            d <= self.k
                        }) {
                            token_matches += 1;
                        }
                    }
                    if token_matches > 0 {
                        let score = token_matches as f32 / tokens.len() as f32;
                        results.push(SearchResult {
                            term: self.terms_display[idx].clone(),
                            term_index: idx,
                            score: score * 0.8,
                            hamming_distance: self.k,
                            match_ranges: compute_match_ranges(q, term),
                        });
                    }
                }
            }
        }

        results.sort_unstable_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(max_results);
        Ok(results)
    }

    /// Brute force with fingerprint-based deletion/insertion detection.
    /// No pre-generated variants needed.
    #[inline]
    fn search_brute(&self, q: &[u8]) -> Vec<(usize, usize)> {
        let n = self.terms.len();
        let k = self.k;
        let qlen = q.len();
        let q_fp = fingerprint::compute(q);

        let mut seen = vec![false; n];
        let mut results = Vec::with_capacity(16);

        // pass 1: columnar Hamming scan (substitutions + transpositions)
        for (i, d) in self.col().scan(q, k) {
            seen[i] = true;
            results.push((i, d));
        }

        // pass 2: fingerprint scan for deletions/insertions
        // this replaces the old deletion_map with a single scan
        for i in 0..n {
            if seen[i] { continue; }

            let tlen = self.term_lengths[i];
            let tfp = &self.fingerprints[i];

            // check deletion: user omitted a char ("javasript" → "javascript")
            if fingerprint::could_be_deletion(&q_fp, tfp, qlen, tlen) {
                // fingerprint says it's plausible — verify with actual string comparison
                if self.verify_deletion(q, &self.terms[i]) {
                    seen[i] = true;
                    results.push((i, 1));
                    continue;
                }
            }

            // check insertion: user added a char ("javasccript" → "javascript")
            if fingerprint::could_be_insertion(&q_fp, tfp, qlen, tlen) {
                if self.verify_insertion(q, &self.terms[i]) {
                    seen[i] = true;
                    results.push((i, 1));
                    continue;
                }
            }

            // check close (combined errors) if k >= 2
            if k >= 2 && !seen[i] && fingerprint::could_be_close(&q_fp, tfp, qlen, tlen) {
                // do actual edit distance check only on candidates that pass fingerprint
                let d = self.quick_edit_distance(q, &self.terms[i], k);
                if d <= k {
                    seen[i] = true;
                    results.push((i, d));
                }
            }
        }

        // pass 3: O(qlen) lookups — deletion variants of query for exact term match
        // covers: user inserted a char, and the shortened query matches a term exactly
        let mut buf = Vec::with_capacity(qlen);
        for skip in 0..qlen {
            buf.clear();
            buf.extend_from_slice(&q[..skip]);
            buf.extend_from_slice(&q[skip + 1..]);
            if let Some(indices) = self.tmap().get(&buf) {
                for &idx in indices {
                    if !seen[idx] {
                        seen[idx] = true;
                        results.push((idx, 1));
                    }
                }
            }
        }

        results
    }

    /// Verify that `query` is actually a 1-deletion of `term` (not just fingerprint match).
    #[inline]
    fn verify_deletion(&self, query: &[u8], term: &[u8]) -> bool {
        // query should be term with 1 char removed
        if term.len() != query.len() + 1 { return false; }
        let mut qi = 0;
        let mut ti = 0;
        let mut diffs = 0;
        while qi < query.len() && ti < term.len() {
            if query[qi] == term[ti] {
                qi += 1;
                ti += 1;
            } else {
                diffs += 1;
                if diffs > 1 { return false; }
                ti += 1; // skip 1 char in term
            }
        }
        true
    }

    /// Verify that `query` is actually a 1-insertion of `term`.
    #[inline]
    fn verify_insertion(&self, query: &[u8], term: &[u8]) -> bool {
        // query should be term with 1 extra char
        if query.len() != term.len() + 1 { return false; }
        let mut qi = 0;
        let mut ti = 0;
        let mut diffs = 0;
        while qi < query.len() && ti < term.len() {
            if query[qi] == term[ti] {
                qi += 1;
                ti += 1;
            } else {
                diffs += 1;
                if diffs > 1 { return false; }
                qi += 1; // skip 1 char in query
            }
        }
        true
    }

    /// Bounded Damerau-Levenshtein distance (includes transposition as 1 op).
    #[inline]
    fn quick_edit_distance(&self, a: &[u8], b: &[u8], max_k: usize) -> usize {
        let m = a.len();
        let n = b.len();
        if m.abs_diff(n) > max_k { return max_k + 1; }

        // Damerau-Levenshtein needs 3 rows: prev2, prev, curr
        let mut prev2 = vec![0u16; n + 1];
        let mut prev = vec![0u16; n + 1];
        let mut curr = vec![0u16; n + 1];
        for j in 0..=n { prev[j] = j as u16; }

        for i in 1..=m {
            curr[0] = i as u16;
            let mut row_min = curr[0];
            for j in 1..=n {
                let cost = if a[i - 1] == b[j - 1] { 0u16 } else { 1 };
                curr[j] = (prev[j] + 1)        // deletion
                    .min(curr[j - 1] + 1)       // insertion
                    .min(prev[j - 1] + cost);   // substitution

                // transposition: swap of adjacent chars
                if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                    curr[j] = curr[j].min(prev2[j - 2] + 1);
                }
                row_min = row_min.min(curr[j]);
            }
            if row_min as usize > max_k { return max_k + 1; }
            std::mem::swap(&mut prev2, &mut prev);
            std::mem::swap(&mut prev, &mut curr);
        }

        prev[n] as usize
    }

    /// CGL tree search with confusion-aware pruning + fingerprint deletion/insertion.
    /// Query the CGL tree for Hamming matches (supplements brute force).
    fn search_tree_node(&self, root: &tree::CglNode, q: &[u8]) -> Vec<(usize, usize)> {
        let alt_query = AlteredString::new(q, None);
        let k = self.k;
        let qlen = q.len();

        let config = tree::PruneConfig {
            confusion: Some(&self.scorer.confusion),
            prune_threshold: 0.08,
        };
        let raw = tree::query_with_pruning(Some(root), &alt_query, k, &config);

        let mut results = Vec::with_capacity(raw.len());
        for m in raw {
            if let Some(term) = self.terms.get(m.origin) {
                let d = fast_hamming(q, qlen, term, k);
                if d <= k {
                    results.push((m.origin, d));
                }
            }
        }
        results
    }

    pub fn len(&self) -> usize {
        self.terms.len()
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    /// Add a term to the index. Invalidates lazy caches (rebuilt on next search).
    pub fn add(&mut self, term: &str) {
        let lower: Vec<u8> = term.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
        self.term_lengths.push(lower.len());
        self.fingerprints.push(fingerprint::compute(&lower));
        self.terms.push(lower);
        self.terms_display.push(term.to_string());
        // invalidate lazy caches
        self.col_terms = OnceLock::new();
        self.encoded_terms = OnceLock::new();
        self.term_map = OnceLock::new();
        self.root = OnceLock::new();
    }

    /// Remove a term by index. Invalidates lazy caches.
    pub fn remove(&mut self, index: usize) {
        if index >= self.terms.len() { return; }
        self.terms.remove(index);
        self.terms_display.remove(index);
        self.term_lengths.remove(index);
        self.fingerprints.remove(index);
        self.col_terms = OnceLock::new();
        self.encoded_terms = OnceLock::new();
        self.term_map = OnceLock::new();
        self.root = OnceLock::new();
    }

    /// Remove all terms matching a predicate. Returns number removed.
    pub fn remove_where(&mut self, predicate: impl Fn(&str) -> bool) -> usize {
        let mut removed = 0;
        let mut i = 0;
        while i < self.terms_display.len() {
            if predicate(&self.terms_display[i]) {
                self.terms.remove(i);
                self.terms_display.remove(i);
                self.term_lengths.remove(i);
                self.fingerprints.remove(i);
                removed += 1;
            } else {
                i += 1;
            }
        }
        if removed > 0 {
            self.col_terms = OnceLock::new();
            self.encoded_terms = OnceLock::new();
            self.term_map = OnceLock::new();
            self.root = OnceLock::new();
        }
        removed
    }

    /// Export index data for serialization. Returns (terms, k) that can reconstruct the index.
    pub fn export(&self) -> (Vec<String>, usize) {
        (self.terms_display.clone(), self.k)
    }

    /// Reconstruct an index from exported data.
    pub fn import(terms: &[String], k: usize) -> Result<Self> {
        let refs: Vec<&str> = terms.iter().map(|s| s.as_str()).collect();
        Self::build(&refs, k)
    }

    pub fn stats(&self) -> IndexStats {
        IndexStats {
            num_terms: self.terms.len(),
            max_mismatches: self.k,
            has_tree: self.root.get().is_some_and(|r| r.is_some()),
        }
    }
}

/// Compute contiguous ranges where query matches term (for highlighting).
/// Returns (start, end) pairs of matching character spans.
fn compute_match_ranges(query: &[u8], term: &[u8]) -> Vec<(usize, usize)> {
    let len = query.len().min(term.len());
    let mut ranges = Vec::new();
    let mut start: Option<usize> = None;

    for i in 0..len {
        if query[i] == term[i] {
            if start.is_none() { start = Some(i); }
        } else if let Some(s) = start.take() {
            ranges.push((s, i));
        }
    }
    if let Some(s) = start {
        ranges.push((s, len));
    }
    ranges
}

#[inline]
fn fast_hamming(q: &[u8], qlen: usize, term: &[u8], max_k: usize) -> usize {
    let tlen = term.len();
    let len_diff = if qlen > tlen { qlen - tlen } else { tlen - qlen };
    if len_diff > max_k { return max_k + 1; }

    let len = qlen.min(tlen);
    let mut d = len_diff;

    let chunks = len / 8;
    let mut i = 0;
    for _ in 0..chunks {
        let a = u64::from_ne_bytes(q[i..i + 8].try_into().unwrap());
        let b = u64::from_ne_bytes(term[i..i + 8].try_into().unwrap());
        let xor = a ^ b;
        if xor != 0 {
            let mask = 0x7F7F_7F7F_7F7F_7F7Fu64;
            let lo = xor & mask;
            let hi = (xor >> 7) & 0x0101_0101_0101_0101u64;
            let nonzero = lo | (lo >> 1) | (lo >> 2) | (lo >> 3)
                | (lo >> 4) | (lo >> 5) | (lo >> 6) | hi;
            let ones = nonzero & 0x0101_0101_0101_0101u64;
            d += ones.count_ones() as usize;
            if d > max_k { return d; }
        }
        i += 8;
    }

    for j in i..len {
        if q[j] != term[j] {
            d += 1;
            if d > max_k { return d; }
        }
    }

    d
}

#[derive(Debug)]
pub struct IndexStats {
    pub num_terms: usize,
    pub max_mismatches: usize,
    pub has_tree: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_search() {
        let terms = vec!["javascript", "typescript", "python", "rust", "golang"];
        let idx = FuzzyIndex::build(&terms, 2).unwrap();
        let results = idx.search("javascrip", 5).unwrap();
        assert!(!results.is_empty());
    }

    #[test]
    fn exact_match_highest_score() {
        let terms = vec!["hello", "hallo", "hullo", "jello"];
        let idx = FuzzyIndex::build(&terms, 2).unwrap();
        let results = idx.search("hello", 10).unwrap();
        assert_eq!(results[0].term, "hello");
        assert_eq!(results[0].score, 1.0);
    }

    #[test]
    fn empty_input_errors() {
        let terms: Vec<&str> = vec![];
        let result = FuzzyIndex::build(&terms, 2);
        assert!(result.is_err());
    }

    #[test]
    fn transposition_ranks_higher() {
        let terms = vec!["the", "teh", "txe"];
        let idx = FuzzyIndex::build(&terms, 2).unwrap();
        let results = idx.search("teh", 10).unwrap();
        let the_score = results.iter().find(|r| r.term == "the").map(|r| r.score).unwrap_or(0.0);
        let txe_score = results.iter().find(|r| r.term == "txe").map(|r| r.score).unwrap_or(0.0);
        assert!(the_score >= txe_score);
    }

    #[test]
    fn small_dataset_uses_brute() {
        let terms = vec!["a", "b", "c"];
        let idx = FuzzyIndex::build(&terms, 1).unwrap();
        assert!(!idx.stats().has_tree);
    }

    #[test]
    fn large_dataset_builds_tree() {
        let terms: Vec<String> = (0..6000).map(|i| format!("term{:05}", i)).collect();
        let refs: Vec<&str> = terms.iter().map(|s| s.as_str()).collect();
        let idx = FuzzyIndex::build(&refs, 2).unwrap();
        // tree is lazy — not built until first search
        assert!(!idx.stats().has_tree);
        let results = idx.search("term03000", 5).unwrap();
        assert!(!results.is_empty());
        // now tree should be built
        assert!(idx.stats().has_tree);
    }

    #[test]
    fn finds_deletion_typo() {
        let terms = vec!["javascript", "typescript", "python"];
        let idx = FuzzyIndex::build(&terms, 2).unwrap();
        let results = idx.search("javasript", 10).unwrap();
        assert!(
            results.iter().any(|r| r.term == "javascript"),
            "should find 'javascript' via fingerprint deletion, got: {:?}",
            results.iter().map(|r| &r.term).collect::<Vec<_>>()
        );
    }

    #[test]
    fn finds_transposition_typo() {
        let terms = vec!["javascript", "typescript", "python"];
        let idx = FuzzyIndex::build(&terms, 2).unwrap();
        let results = idx.search("javsacript", 10).unwrap();
        assert!(
            results.iter().any(|r| r.term == "javascript"),
            "should find 'javascript' via transposition, got: {:?}",
            results.iter().map(|r| &r.term).collect::<Vec<_>>()
        );
    }

    #[test]
    fn finds_insertion_typo() {
        let terms = vec!["javascript", "typescript", "python"];
        let idx = FuzzyIndex::build(&terms, 2).unwrap();
        let results = idx.search("javasccript", 10).unwrap();
        assert!(
            results.iter().any(|r| r.term == "javascript"),
            "should find 'javascript' via fingerprint insertion, got: {:?}",
            results.iter().map(|r| &r.term).collect::<Vec<_>>()
        );
    }
}
