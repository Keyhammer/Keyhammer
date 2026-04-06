/// Columnar term storage for SIMD-friendly brute force scan.
///
/// Instead of storing terms row-wise (each term is a contiguous byte array),
/// stores them column-wise: column[i] has the i-th byte of every term.
/// This lets us compare query[i] against ALL terms at position i in one
/// cache-friendly pass — the CPU loads a full cache line of position-i bytes
/// and blasts through them.

pub struct ColumnarTerms {
    /// column[i] = the i-th byte of every term, padded with 0 for shorter terms.
    columns: Vec<Vec<u8>>,
    /// Length of each term (needed to skip padding).
    lengths: Vec<usize>,
    /// Number of terms.
    num_terms: usize,
    /// Max term length (= number of columns).
    max_len: usize,
}

impl ColumnarTerms {
    /// Build columnar storage from a list of terms.
    pub fn build(terms: &[Vec<u8>]) -> Self {
        let num_terms = terms.len();
        let max_len = terms.iter().map(|t| t.len()).max().unwrap_or(0);
        let lengths: Vec<usize> = terms.iter().map(|t| t.len()).collect();

        let mut columns = Vec::with_capacity(max_len);
        for col in 0..max_len {
            let mut column = Vec::with_capacity(num_terms);
            for term in terms {
                if col < term.len() {
                    column.push(term[col]);
                } else {
                    column.push(0); // padding for shorter terms
                }
            }
            columns.push(column);
        }

        Self { columns, lengths, num_terms, max_len }
    }

    /// Columnar Hamming scan: find all terms within distance `k` of `query`.
    /// Returns (term_index, distance) pairs.
    ///
    /// Instead of comparing query against each term sequentially,
    /// processes column by column — comparing query[i] against position i
    /// of ALL terms in one pass. This is cache-friendly and auto-vectorizable.
    #[inline]
    pub fn scan(&self, query: &[u8], k: usize) -> Vec<(usize, usize)> {
        let qlen = query.len();
        let n = self.num_terms;

        // distance accumulator for each term — starts at 0
        let mut dists = vec![0u16; n];

        // eliminated[i] = true once term i exceeds distance k
        let mut alive = n;

        // process column by column
        let check_len = qlen.min(self.max_len);
        for col in 0..check_len {
            if alive == 0 { break; }

            let q_byte = query[col];
            let column = &self.columns[col];

            // this loop is auto-vectorizable by LLVM:
            // comparing one byte against a contiguous array
            for i in 0..n {
                // skip already-eliminated terms (but keep iterating for vectorization)
                // mismatch: col < term length AND bytes differ
                if column[i] != q_byte && col < self.lengths[i] {
                    dists[i] += 1;
                }
            }

            // periodic check: prune terms that exceeded k
            // (don't check every column — amortize the branch cost)
            if (col & 3) == 3 {
                alive = 0;
                for i in 0..n {
                    if dists[i] <= k as u16 {
                        alive += 1;
                    }
                }
            }
        }

        // add length difference penalty
        let mut results = Vec::with_capacity(16);
        for i in 0..n {
            let len_diff = if qlen > self.lengths[i] {
                qlen - self.lengths[i]
            } else {
                self.lengths[i] - qlen
            };
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
        let terms = vec![b"hello".to_vec(), b"world".to_vec(), b"help".to_vec()];
        let col = ColumnarTerms::build(&terms);
        let results = col.scan(b"hello", 0);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], (0, 0));
    }

    #[test]
    fn one_mismatch() {
        let terms = vec![b"hello".to_vec(), b"hallo".to_vec(), b"hullo".to_vec()];
        let col = ColumnarTerms::build(&terms);
        let results = col.scan(b"hello", 1);
        assert!(results.iter().any(|&(i, d)| i == 0 && d == 0)); // exact
        assert!(results.iter().any(|&(i, d)| i == 1 && d == 1)); // hallo
        assert!(results.iter().any(|&(i, d)| i == 2 && d == 1)); // hullo
    }

    #[test]
    fn respects_max_distance() {
        let terms = vec![b"aaaa".to_vec(), b"bbbb".to_vec(), b"cccc".to_vec()];
        let col = ColumnarTerms::build(&terms);
        let results = col.scan(b"aaaa", 1);
        assert_eq!(results.len(), 1); // only "aaaa" matches
    }

    #[test]
    fn handles_different_lengths() {
        let terms = vec![b"ab".to_vec(), b"abc".to_vec(), b"abcd".to_vec()];
        let col = ColumnarTerms::build(&terms);
        let results = col.scan(b"abc", 1);
        // "ab" = 1 (length diff), "abc" = 0 (exact), "abcd" = 1 (length diff)
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn empty_query() {
        let terms = vec![b"a".to_vec(), b"bb".to_vec()];
        let col = ColumnarTerms::build(&terms);
        let results = col.scan(b"", 1);
        // "" vs "a" = dist 1, "" vs "bb" = dist 2
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], (0, 1));
    }
}
