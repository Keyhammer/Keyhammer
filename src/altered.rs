/// A character substitution at a specific position.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct Alteration {
    pub index: usize,
    pub ch: u8,
}

/// String with lazy alterations stacked on top.
/// Caches the materialized form to avoid re-computing on every comparison.
#[derive(Clone, Debug)]
pub struct AlteredString {
    #[allow(dead_code)]
    pub base: Vec<u8>,
    pub alterations: Vec<Alteration>,
    pub origin: Option<usize>,
    cached: Vec<u8>, // materialized bytes, kept in sync
}

impl AlteredString {
    pub fn new(base: &[u8], origin: Option<usize>) -> Self {
        let cached = base.to_vec();
        Self {
            base: base.to_vec(),
            alterations: Vec::new(),
            origin,
            cached,
        }
    }

    /// Get the materialized bytes (zero-cost, returns cached slice).
    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        &self.cached
    }

    /// Create a new AlteredString with one more alteration applied.
    pub fn with_alteration(&self, index: usize, ch: u8) -> Self {
        let mut new = self.clone();
        new.alterations.push(Alteration { index, ch });
        if index < new.cached.len() {
            new.cached[index] = ch;
        }
        new
    }
}

/// Hamming distance between two altered strings (used in tests and by the index).
#[inline]
#[allow(dead_code)]
pub fn hamming(a: &AlteredString, b: &AlteredString) -> usize {
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    let len = ab.len().min(bb.len());
    let mut dist = 0;
    for i in 0..len {
        if ab[i] != bb[i] {
            dist += 1;
        }
    }
    dist
}

/// Longest common prefix length.
#[inline]
pub fn lcp(a: &AlteredString, b: &AlteredString) -> usize {
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    let len = ab.len().min(bb.len());
    let mut i = 0;
    while i < len && ab[i] == bb[i] {
        i += 1;
    }
    i
}

/// Lexicographic comparison on cached bytes.
#[inline]
pub fn lex_cmp(a: &AlteredString, b: &AlteredString) -> std::cmp::Ordering {
    a.as_bytes().cmp(b.as_bytes())
}

/// Pivot-alter: force `s` to match `pivot` at one more position.
pub fn pivot_alter(s: &AlteredString, pivot: &AlteredString) -> AlteredString {
    let i = lcp(s, pivot);
    let pb = pivot.as_bytes();
    if i >= pb.len() {
        return s.clone();
    }
    s.with_alteration(i, pb[i])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hamming_identical() {
        let a = AlteredString::new(b"hello", None);
        let b = AlteredString::new(b"hello", None);
        assert_eq!(hamming(&a, &b), 0);
    }

    #[test]
    fn hamming_one_diff() {
        let a = AlteredString::new(b"hello", None);
        let b = AlteredString::new(b"hallo", None);
        assert_eq!(hamming(&a, &b), 1);
    }

    #[test]
    fn pivot_alter_reduces_distance() {
        let s = AlteredString::new(b"abc", None);
        let pivot = AlteredString::new(b"axc", None);
        let query = AlteredString::new(b"axc", None);

        let before = hamming(&s, &query);
        let altered = pivot_alter(&s, &pivot);
        let after = hamming(&altered, &query);

        assert!(after < before, "pivot-alter should reduce distance");
    }

    #[test]
    fn lcp_basic() {
        let a = AlteredString::new(b"javascript", None);
        let b = AlteredString::new(b"javelin", None);
        assert_eq!(lcp(&a, &b), 3);
    }

    #[test]
    fn cached_stays_in_sync() {
        let s = AlteredString::new(b"hello", None);
        let altered = s.with_alteration(1, b'a');
        assert_eq!(altered.as_bytes(), b"hallo");
    }
}
