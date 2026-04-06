/// FuzzyIndex — maximum performance fuzzy search.
///
/// Uses fingerprint-based matching instead of pre-generated deletion variants.
/// Build is fast (no variant generation), memory is low, queries use fingerprints
/// as a cheap pre-filter before doing actual string comparison.

use std::collections::HashSet;

use crate::altered::AlteredString;
use crate::error::{Error, Result};
use crate::fingerprint::{self, Fingerprint};
use crate::inverter::FunctionInverter;
use crate::scorer::TypoScorer;
use crate::tree;

const TREE_THRESHOLD: usize = 100_000;

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub term: String,
    pub term_index: usize,
    pub score: f32,
    pub hamming_distance: usize,
}

pub struct FuzzyIndex {
    terms: Vec<Vec<u8>>,
    term_lengths: Vec<usize>,
    /// Columnar layout for SIMD-friendly scan.
    col_terms: crate::columnar::ColumnarTerms,
    /// Fingerprint per term — 26 bytes each, used for deletion/insertion detection.
    fingerprints: Vec<Fingerprint>,
    /// Exact term lookup for O(1) matching of query deletion variants.
    term_map: std::collections::HashMap<Vec<u8>, Vec<usize>>,
    root: Option<tree::CglNode>,
    inverter: Option<FunctionInverter>,
    leaf_map: std::collections::HashMap<usize, usize>,
    scorer: TypoScorer,
    k: usize,
    sigma: usize,
    use_tree: bool,
}

impl FuzzyIndex {
    pub fn build(terms: &[&str], k: usize) -> Result<Self> {
        if terms.is_empty() {
            return Err(Error::EmptyInput);
        }

        let max_len = terms.iter().map(|t| t.len()).max().unwrap_or(0);
        let n = terms.len();
        let scorer = TypoScorer::new(max_len);
        let terms_owned: Vec<Vec<u8>> = terms.iter().map(|t| t.as_bytes().to_vec()).collect();
        let term_lengths: Vec<usize> = terms_owned.iter().map(|t| t.len()).collect();

        // fingerprints: 26 bytes per term, O(n) total
        let fingerprints: Vec<Fingerprint> = terms_owned.iter()
            .map(|t| fingerprint::compute(t))
            .collect();

        // term map for O(1) exact lookup
        let mut term_map: std::collections::HashMap<Vec<u8>, Vec<usize>> =
            std::collections::HashMap::with_capacity(n);
        for (i, term) in terms_owned.iter().enumerate() {
            term_map.entry(term.clone()).or_default().push(i);
        }

        let col_terms = crate::columnar::ColumnarTerms::build(&terms_owned);

        if n < TREE_THRESHOLD {
            return Ok(Self {
                terms: terms_owned,
                term_lengths,
                col_terms,
                fingerprints,
                term_map,
                root: None,
                inverter: None,
                leaf_map: std::collections::HashMap::new(),
                scorer,
                k,
                sigma: 0,
                use_tree: false,
            });
        }

        let sigma = (((n as f64 + 1.0).log2() / k.max(1) as f64).ceil() as usize).max(2);

        let altered: Vec<AlteredString> = terms.iter()
            .enumerate()
            .map(|(i, t)| AlteredString::new(t.as_bytes(), Some(i)))
            .collect();

        let root = tree::build(altered, k, sigma);
        let leaf_map = tree::build_leaf_map(root.as_ref());

        let leaf_map_clone = leaf_map.clone();
        let f = move |i: usize| -> Option<usize> {
            leaf_map_clone.get(&i).copied()
        };
        let inverter = FunctionInverter::build(n, sigma, &f);

        Ok(Self {
            terms: terms_owned,
            term_lengths,
            col_terms,
            fingerprints,
            term_map,
            root,
            inverter: Some(inverter),
            leaf_map,
            scorer,
            k,
            sigma,
            use_tree: true,
        })
    }

    pub fn search(&self, query: &str, max_results: usize) -> Result<Vec<SearchResult>> {
        let q = query.as_bytes();

        let candidates = if self.use_tree {
            self.search_tree(q)
        } else {
            self.search_brute(q)
        };

        let mut results: Vec<SearchResult> = candidates.into_iter()
            .map(|(idx, hd)| {
                let term = &self.terms[idx];
                let typo_score = self.scorer.score(q, term);
                SearchResult {
                    term: unsafe { String::from_utf8_unchecked(term.clone()) },
                    term_index: idx,
                    score: typo_score.score,
                    hamming_distance: hd,
                }
            })
            .collect();

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
        for (i, d) in self.col_terms.scan(q, k) {
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
            if let Some(indices) = self.term_map.get(&buf) {
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

    /// Quick bounded edit distance for combined error detection.
    #[inline]
    fn quick_edit_distance(&self, a: &[u8], b: &[u8], max_k: usize) -> usize {
        let m = a.len();
        let n = b.len();
        if m.abs_diff(n) > max_k { return max_k + 1; }

        // simple bounded Levenshtein via 2-row DP
        let mut prev = vec![0u16; n + 1];
        let mut curr = vec![0u16; n + 1];
        for j in 0..=n { prev[j] = j as u16; }

        for i in 1..=m {
            curr[0] = i as u16;
            let mut row_min = curr[0];
            for j in 1..=n {
                let cost = if a[i - 1] == b[j - 1] { 0u16 } else { 1 };
                curr[j] = (prev[j] + 1)
                    .min(curr[j - 1] + 1)
                    .min(prev[j - 1] + cost);
                row_min = row_min.min(curr[j]);
            }
            if row_min as usize > max_k { return max_k + 1; }
            std::mem::swap(&mut prev, &mut curr);
        }

        prev[n] as usize
    }

    /// CGL tree search with confusion-aware pruning + fingerprint deletion/insertion.
    fn search_tree(&self, q: &[u8]) -> Vec<(usize, usize)> {
        let alt_query = AlteredString::new(q, None);
        let qlen = q.len();
        let k = self.k;
        let q_fp = fingerprint::compute(q);

        let config = tree::PruneConfig {
            confusion: Some(&self.scorer.confusion),
            prune_threshold: 0.08,
        };
        let raw = tree::query_with_pruning(self.root.as_ref(), &alt_query, self.k, &config);

        let mut seen = HashSet::new();
        let mut results = Vec::with_capacity(16);

        // CGL tree matches (Hamming-based: substitutions + transpositions)
        for m in &raw {
            seen.insert(m.origin);
        }

        if let Some(ref inverter) = self.inverter {
            let leaf_map = &self.leaf_map;
            let f = |i: usize| -> Option<usize> { leaf_map.get(&i).copied() };
            for m in &raw {
                if let Some(&label) = leaf_map.get(&m.origin) {
                    for idx in inverter.invert(label, &f) {
                        seen.insert(idx);
                    }
                }
            }
        }

        // verify Hamming on tree candidates
        for idx in &seen {
            if let Some(term) = self.terms.get(*idx) {
                let d = fast_hamming(q, qlen, term, k);
                if d <= k {
                    results.push((*idx, d));
                }
            }
        }

        // fingerprint scan for deletions/insertions (tree only does same-length Hamming)
        let n = self.terms.len();
        for i in 0..n {
            if seen.contains(&i) { continue; }

            let tlen = self.term_lengths[i];
            let tfp = &self.fingerprints[i];

            if fingerprint::could_be_deletion(&q_fp, tfp, qlen, tlen) {
                if self.verify_deletion(q, &self.terms[i]) {
                    seen.insert(i);
                    results.push((i, 1));
                    continue;
                }
            }

            if fingerprint::could_be_insertion(&q_fp, tfp, qlen, tlen) {
                if self.verify_insertion(q, &self.terms[i]) {
                    seen.insert(i);
                    results.push((i, 1));
                    continue;
                }
            }
        }

        // term_map lookup for query deletion variants
        let mut buf = Vec::with_capacity(qlen);
        for skip in 0..qlen {
            buf.clear();
            buf.extend_from_slice(&q[..skip]);
            buf.extend_from_slice(&q[skip + 1..]);
            if let Some(indices) = self.term_map.get(&buf) {
                for &idx in indices {
                    if !seen.contains(&idx) {
                        seen.insert(idx);
                        results.push((idx, 1));
                    }
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

    pub fn stats(&self) -> IndexStats {
        IndexStats {
            num_terms: self.terms.len(),
            max_mismatches: self.k,
            sigma: self.sigma,
            tree_nodes: tree::node_count(self.root.as_ref()),
            leaf_count: self.leaf_map.values().collect::<HashSet<_>>().len(),
            strategy: if self.use_tree { "tree" } else { "brute" }.into(),
        }
    }
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
    pub sigma: usize,
    pub tree_nodes: usize,
    pub leaf_count: usize,
    pub strategy: String,
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
        assert_eq!(idx.stats().strategy, "brute");
    }

    #[test]
    fn inverter_recovers_terms() {
        let terms: Vec<&str> = (0..50).map(|i| match i % 5 {
            0 => "apple",
            1 => "apply",
            2 => "ample",
            3 => "ankle",
            _ => "angle",
        }).collect();
        let idx = FuzzyIndex::build(&terms, 2).unwrap();
        let results = idx.search("apple", 10).unwrap();
        assert!(!results.is_empty());
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
