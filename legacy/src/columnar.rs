// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

/// Column-oriented term storage for cache-friendly scans.
///
/// Terms are stored transposed: `columns[i]` holds the i-th byte of every term.
/// This lets the CPU compare `query[i]` against all terms at position `i` in one
/// contiguous memory pass — friendly to cache prefetch and auto-vectorization.

pub struct ColumnarTerms {
    columns: Vec<Vec<u8>>,
    lengths: Vec<usize>,
    num_terms: usize,
    max_len: usize,
}

impl ColumnarTerms {
    pub fn build(terms: &[Vec<u8>]) -> Self {
        let num_terms = terms.len();
        let max_len = terms.iter().map(|t| t.len()).max().unwrap_or(0);
        let lengths: Vec<usize> = terms.iter().map(|t| t.len()).collect();

        let mut columns = Vec::with_capacity(max_len);
        for col in 0..max_len {
            let mut column = Vec::with_capacity(num_terms);
            for term in terms {
                column.push(if col < term.len() { term[col] } else { 0 });
            }
            columns.push(column);
        }

        Self { columns, lengths, num_terms, max_len }
    }

    /// Scan all terms, return those within Hamming distance `k` of `query`.
    #[inline]
    pub fn scan(&self, query: &[u8], k: usize) -> Vec<(usize, usize)> {
        let qlen = query.len();
        let n = self.num_terms;
        let mut dists = vec![0u16; n];
        let mut alive = n;

        let check_len = qlen.min(self.max_len);
        for col in 0..check_len {
            if alive == 0 { break; }
            let q_byte = query[col];
            let column = &self.columns[col];

            // auto-vectorizable inner loop
            for i in 0..n {
                if column[i] != q_byte && col < self.lengths[i] {
                    dists[i] += 1;
                }
            }

            if (col & 3) == 3 {
                alive = dists.iter().filter(|&&d| d <= k as u16).count();
            }
        }

        let mut results = Vec::with_capacity(16);
        for i in 0..n {
            let len_diff = qlen.abs_diff(self.lengths[i]);
            let total = dists[i] as usize + len_diff;
            if total <= k {
                results.push((i, total));
            }
        }
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_match() {
        let col = ColumnarTerms::build(&[b"hello".to_vec(), b"world".to_vec()]);
        let r = col.scan(b"hello", 0);
        assert_eq!(r, vec![(0, 0)]);
    }

    #[test]
    fn one_mismatch() {
        let col = ColumnarTerms::build(&[b"hello".to_vec(), b"hallo".to_vec(), b"hullo".to_vec()]);
        let r = col.scan(b"hello", 1);
        assert_eq!(r.len(), 3);
    }

    #[test]
    fn respects_max_distance() {
        let col = ColumnarTerms::build(&[b"aaaa".to_vec(), b"bbbb".to_vec()]);
        assert_eq!(col.scan(b"aaaa", 1).len(), 1);
    }

    #[test]
    fn different_lengths() {
        let col = ColumnarTerms::build(&[b"ab".to_vec(), b"abc".to_vec(), b"abcd".to_vec()]);
        assert_eq!(col.scan(b"abc", 1).len(), 3);
    }

    #[test]
    fn empty_query() {
        let col = ColumnarTerms::build(&[b"a".to_vec(), b"bb".to_vec()]);
        assert_eq!(col.scan(b"", 1).len(), 1); // only "a" within distance 1
    }
}
