/// Character frequency fingerprint for deletion-free matching.
///
/// Instead of pre-generating all deletion variants (expensive build, high memory),
/// each term gets a 26-byte fingerprint counting occurrences of each letter.
/// To check if query could be a deletion/insertion of a term, compare fingerprints:
///   - Exactly 1 char difference in counts → likely 1 deletion or insertion
///   - Same counts but different string → substitution (Hamming catches this)
///
/// This eliminates the deletion_map entirely.

/// 26 bytes — one count per lowercase ascii letter. Fits in a single cache line.
pub type Fingerprint = [u8; 26];

/// Compute fingerprint of a byte slice.
#[inline]
pub fn compute(word: &[u8]) -> Fingerprint {
    let mut fp = [0u8; 26];
    for &b in word {
        let idx = b.wrapping_sub(b'a') as usize;
        if idx < 26 {
            fp[idx] = fp[idx].saturating_add(1);
        }
    }
    fp
}

/// Compare two fingerprints. Returns:
///   - total absolute difference in char counts
///   - number of positions where counts differ
#[inline]
pub fn diff(a: &Fingerprint, b: &Fingerprint) -> (u16, u8) {
    let mut total_diff: u16 = 0;
    let mut positions: u8 = 0;
    for i in 0..26 {
        let d = (a[i] as i16 - b[i] as i16).unsigned_abs();
        total_diff += d;
        if d > 0 {
            positions += 1;
        }
    }
    (total_diff, positions)
}

/// Check if `query` could be a 1-deletion of `term` (query = term minus 1 char).
/// The fingerprint of query should have exactly 1 fewer char than term's fingerprint.
#[inline]
pub fn could_be_deletion(query_fp: &Fingerprint, term_fp: &Fingerprint, query_len: usize, term_len: usize) -> bool {
    // deletion: query is 1 shorter than term
    if term_len != query_len + 1 {
        return false;
    }
    let (total, positions) = diff(query_fp, term_fp);
    // exactly 1 char missing from query
    total == 1 && positions == 1
}

/// Check if `query` could be a 1-insertion of `term` (query = term plus 1 extra char).
#[inline]
pub fn could_be_insertion(query_fp: &Fingerprint, term_fp: &Fingerprint, query_len: usize, term_len: usize) -> bool {
    // insertion: query is 1 longer than term
    if query_len != term_len + 1 {
        return false;
    }
    let (total, positions) = diff(query_fp, term_fp);
    total == 1 && positions == 1
}

/// Check if `query` could be within edit distance 2 of `term` via fingerprint.
/// Used as a cheap pre-filter before doing actual string comparison.
#[inline]
pub fn could_be_close(query_fp: &Fingerprint, term_fp: &Fingerprint, query_len: usize, term_len: usize) -> bool {
    let len_diff = if query_len > term_len { query_len - term_len } else { term_len - query_len };
    if len_diff > 2 {
        return false;
    }
    let (total, _) = diff(query_fp, term_fp);
    total <= 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_fingerprints() {
        let a = compute(b"hello");
        let b = compute(b"hello");
        let (total, positions) = diff(&a, &b);
        assert_eq!(total, 0);
        assert_eq!(positions, 0);
    }

    #[test]
    fn anagram_same_fingerprint() {
        let a = compute(b"listen");
        let b = compute(b"silent");
        let (total, _) = diff(&a, &b);
        assert_eq!(total, 0);
    }

    #[test]
    fn deletion_detected() {
        let query = compute(b"javasript");    // missing 'c'
        let term = compute(b"javascript");
        assert!(could_be_deletion(&query, &term, 9, 10));
    }

    #[test]
    fn insertion_detected() {
        let query = compute(b"javasccript");  // extra 'c'
        let term = compute(b"javascript");
        assert!(could_be_insertion(&query, &term, 11, 10));
    }

    #[test]
    fn substitution_not_deletion() {
        let query = compute(b"javoscript");   // 'a' → 'o', same length
        let term = compute(b"javascript");
        assert!(!could_be_deletion(&query, &term, 10, 10));
    }

    #[test]
    fn transposition_same_fingerprint() {
        // transposition doesn't change char counts
        let query = compute(b"javsacript");
        let term = compute(b"javascript");
        let (total, _) = diff(&query, &term);
        assert_eq!(total, 0); // same chars, different order
    }

    #[test]
    fn close_filter_works() {
        let query = compute(b"javasript");
        let term = compute(b"javascript");
        assert!(could_be_close(&query, &term, 9, 10));

        let far = compute(b"python");
        assert!(!could_be_close(&query, &far, 9, 6));
    }
}
