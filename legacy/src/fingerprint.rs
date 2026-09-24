// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

/// Character frequency fingerprint.
///
/// 96-byte array covering printable ASCII (32-126). Used as a cheap pre-filter
/// to detect possible deletions/insertions without generating variants.

/// Covers printable ASCII. Fits in 1.5 cache lines.
pub type Fingerprint = [u8; 96];

/// Compute fingerprint: count occurrences of each printable ASCII byte.
#[inline]
pub fn compute(word: &[u8]) -> Fingerprint {
    let mut fp = [0u8; 96];
    for &b in word {
        let idx = b.wrapping_sub(32) as usize;
        if idx < 96 {
            fp[idx] = fp[idx].saturating_add(1);
        }
    }
    fp
}

/// Total absolute difference between two fingerprints.
#[inline]
pub fn diff(a: &Fingerprint, b: &Fingerprint) -> u16 {
    let mut total: u16 = 0;
    for i in 0..96 {
        total += (a[i] as i16 - b[i] as i16).unsigned_abs();
    }
    total
}

/// Could `query` be `term` with one character deleted?
#[inline]
pub fn could_be_deletion(query_fp: &Fingerprint, term_fp: &Fingerprint, query_len: usize, term_len: usize) -> bool {
    term_len == query_len + 1 && diff(query_fp, term_fp) == 1
}

/// Could `query` be `term` with one character inserted?
#[inline]
pub fn could_be_insertion(query_fp: &Fingerprint, term_fp: &Fingerprint, query_len: usize, term_len: usize) -> bool {
    query_len == term_len + 1 && diff(query_fp, term_fp) == 1
}

/// Could `query` be within edit distance 2 of `term`?
#[inline]
pub fn could_be_close(query_fp: &Fingerprint, term_fp: &Fingerprint, query_len: usize, term_len: usize) -> bool {
    let len_diff = query_len.abs_diff(term_len);
    len_diff <= 2 && diff(query_fp, term_fp) <= 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical() { assert_eq!(diff(&compute(b"hello"), &compute(b"hello")), 0); }

    #[test]
    fn anagram() { assert_eq!(diff(&compute(b"listen"), &compute(b"silent")), 0); }

    #[test]
    fn deletion() { assert!(could_be_deletion(&compute(b"javasript"), &compute(b"javascript"), 9, 10)); }

    #[test]
    fn insertion() { assert!(could_be_insertion(&compute(b"javasccript"), &compute(b"javascript"), 11, 10)); }

    #[test]
    fn substitution_not_deletion() { assert!(!could_be_deletion(&compute(b"javoscript"), &compute(b"javascript"), 10, 10)); }

    #[test]
    fn transposition_same_fp() { assert_eq!(diff(&compute(b"javsacript"), &compute(b"javascript")), 0); }

    #[test]
    fn close_filter() {
        assert!(could_be_close(&compute(b"javasript"), &compute(b"javascript"), 9, 10));
        assert!(!could_be_close(&compute(b"javasript"), &compute(b"python"), 9, 6));
    }

    #[test]
    fn numbers() { assert_eq!(diff(&compute(b"v1.0.0"), &compute(b"v1.0.1")), 2); }

    #[test]
    fn symbols() { assert_eq!(diff(&compute(b"hello@world"), &compute(b"hello#world")), 2); }

    #[test]
    fn spaces() { assert!(could_be_deletion(&compute(b"helloworld"), &compute(b"hello world"), 10, 11)); }
}
