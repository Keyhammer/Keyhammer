// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

/// Edit operation variants for covering Damerau-Levenshtein with Hamming search.

/// All single-character deletions of `word`. "abc" → ["bc", "ac", "ab"].
#[allow(dead_code)]
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

/// All adjacent transposition variants. "abc" → ["bac", "acb"].
#[allow(dead_code)]
pub fn transposition_variants(word: &[u8]) -> Vec<Vec<u8>> {
    let mut variants = Vec::with_capacity(word.len().saturating_sub(1));
    for i in 0..word.len().saturating_sub(1) {
        let mut v = word.to_vec();
        v.swap(i, i + 1);
        if v != word { variants.push(v); }
    }
    variants
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deletion() {
        let v = deletion_variants(b"abc");
        assert_eq!(v, vec![b"bc".to_vec(), b"ac".to_vec(), b"ab".to_vec()]);
    }

    #[test]
    fn transposition() {
        let v = transposition_variants(b"abc");
        assert_eq!(v, vec![b"bac".to_vec(), b"acb".to_vec()]);
    }

    #[test]
    fn deletion_empty() { assert!(deletion_variants(b"").is_empty()); }

    #[test]
    fn transposition_single() { assert!(transposition_variants(b"a").is_empty()); }

    #[test]
    fn transposition_skips_repeated() {
        let v = transposition_variants(b"aab");
        assert_eq!(v, vec![b"aba".to_vec()]);
    }
}
