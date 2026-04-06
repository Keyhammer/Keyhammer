/// Variant generation for covering Levenshtein-type errors with Hamming search.
///
/// The trick: generate deletion/transposition variants at build time and
/// deletion variants of the query at search time. This lets the Hamming-based
/// CGL tree catch insertions and deletions without changing the core algorithm.
///
/// Covers all 4 Damerau-Levenshtein operations:
///   - Substitution → Hamming handles natively
///   - Transposition → Hamming handles (2 substitutions) + dedicated variants
///   - Deletion → query variant (delete 1 char from query) matches full term
///   - Insertion → term variant (delete 1 char from term) matches full query

/// Generate all single-character deletion variants of a word.
/// "abc" → ["bc", "ac", "ab"]
pub fn deletion_variants(word: &[u8]) -> Vec<Vec<u8>> {
    let mut variants = Vec::with_capacity(word.len());
    for i in 0..word.len() {
        let mut v = Vec::with_capacity(word.len() - 1);
        v.extend_from_slice(&word[..i]);
        v.extend_from_slice(&word[i + 1..]);
        variants.push(v);
    }
    variants
}

/// Generate all adjacent transposition variants of a word.
/// "abc" → ["bac", "acb"]
#[allow(dead_code)]
pub fn transposition_variants(word: &[u8]) -> Vec<Vec<u8>> {
    let mut variants = Vec::with_capacity(word.len().saturating_sub(1));
    for i in 0..word.len().saturating_sub(1) {
        let mut v = word.to_vec();
        v.swap(i, i + 1);
        if v != word { // skip if swap produces same string (repeated chars)
            variants.push(v);
        }
    }
    variants
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deletion_produces_correct_variants() {
        let v = deletion_variants(b"abc");
        assert_eq!(v.len(), 3);
        assert_eq!(v[0], b"bc");
        assert_eq!(v[1], b"ac");
        assert_eq!(v[2], b"ab");
    }

    #[test]
    fn transposition_produces_correct_variants() {
        let v = transposition_variants(b"abc");
        assert_eq!(v.len(), 2);
        assert_eq!(v[0], b"bac");
        assert_eq!(v[1], b"acb");
    }

    #[test]
    fn deletion_empty_word() {
        let v = deletion_variants(b"");
        assert!(v.is_empty());
    }

    #[test]
    fn transposition_single_char() {
        let v = transposition_variants(b"a");
        assert!(v.is_empty());
    }

    #[test]
    fn transposition_skips_repeated_chars() {
        let v = transposition_variants(b"aab");
        // swap(0,1) → "aab" (same as original, skipped)
        // swap(1,2) → "aba"
        assert_eq!(v.len(), 1);
        assert_eq!(v[0], b"aba");
    }
}
