/// Typo probability scorer.
///
/// Ranks candidates by how likely the query is a human typing error.
/// Combines position weight, keyboard confusion, and transposition detection.

#[derive(Debug, Clone)]
pub struct TypoScore {
    pub score: f32,
}

/// Supported keyboard layouts for the confusion matrix.
///
/// # Example
///
/// ```
/// use keyhammer_legacy::{FuzzyIndex, KeyboardLayout};
///
/// // French keyboard
/// let index = FuzzyIndex::build_with_layout(&["bonjour"], 2, KeyboardLayout::Azerty).unwrap();
/// ```
#[derive(Clone, Copy, Debug)]
pub enum KeyboardLayout {
    Qwerty,
    Azerty,
    Qwertz,
}

pub struct TypoScorer {
    position_curve: Vec<f32>,
    pub(crate) confusion: [[f32; 26]; 26],
}

impl TypoScorer {
    /// Default scorer (QWERTY).
    #[allow(dead_code)]
    pub fn new(max_len: usize) -> Self {
        Self::with_layout(max_len, KeyboardLayout::Qwerty)
    }

    /// Scorer for a specific keyboard layout.
    pub fn with_layout(max_len: usize, layout: KeyboardLayout) -> Self {
        let curve = Self::build_position_curve(max_len);
        let confusion = Self::build_confusion_matrix(layout);
        Self { position_curve: curve, confusion }
    }

    /// Score how likely `query` is a typo of `term`. 0.0 = no way, 1.0 = exact match.
    pub fn score(&self, query: &[u8], term: &[u8]) -> TypoScore {
        if query.is_empty() || term.is_empty() {
            return TypoScore { score: 0.0 };
        }
        if query == term {
            return TypoScore { score: 1.0 };
        }

        let len = query.len().min(term.len());
        let mut penalty = 0.0f32;

        for i in 0..len {
            if query[i] != term[i] {
                let pos_w = self.position_curve.get(i).copied().unwrap_or(0.5);
                let conf = self.char_confusion(query[i], term[i]);
                penalty += (1.0 - pos_w * 0.6) * (1.0 - conf * 0.5);
            }
        }

        let len_diff = query.len().abs_diff(term.len()) as f32;
        penalty += len_diff * 0.15;

        let trans_bonus = self.count_transpositions(query, term) as f32 * 0.35;
        let raw = 1.0 - (penalty * 0.3) + trans_bonus;

        TypoScore { score: raw.clamp(0.0, 1.0) }
    }

    #[inline]
    fn char_confusion(&self, a: u8, b: u8) -> f32 {
        let ai = a.wrapping_sub(b'a') as usize;
        let bi = b.wrapping_sub(b'a') as usize;
        if ai < 26 && bi < 26 { self.confusion[ai][bi] } else { 0.05 }
    }

    #[inline]
    fn count_transpositions(&self, a: &[u8], b: &[u8]) -> usize {
        let len = a.len().min(b.len());
        if len < 2 { return 0; }
        let mut count = 0;
        let mut i = 0;
        while i < len - 1 {
            if a[i] == b[i + 1] && a[i + 1] == b[i] && a[i] != b[i] {
                count += 1;
                i += 2;
            } else {
                i += 1;
            }
        }
        count
    }

    // ── Internal builders ──────────────────────────────────────────────

    fn build_position_curve(max_len: usize) -> Vec<f32> {
        (0..max_len).map(|i| {
            let t = i as f32 / max_len.max(1) as f32;
            let w = if t < 0.2 {
                0.15 + t * 1.5
            } else if t < 0.8 {
                0.45 + (-(((t - 0.55) * 4.0).powi(2))).exp() * 0.55
            } else {
                0.5 - (t - 0.8) * 1.5
            };
            w.clamp(0.1, 1.0)
        }).collect()
    }

    fn build_confusion_matrix(layout: KeyboardLayout) -> [[f32; 26]; 26] {
        let mut m = [[0.05f32; 26]; 26];
        for i in 0..26 { m[i][i] = 1.0; }

        let (adjacent, same_finger) = Self::layout_pairs(layout);

        for (a, b, w) in adjacent {
            let (ai, bi) = ((a - b'a') as usize, (b - b'a') as usize);
            if ai < 26 && bi < 26 { m[ai][bi] = w; m[bi][ai] = w; }
        }
        for (a, b) in same_finger {
            let (ai, bi) = ((a - b'a') as usize, (b - b'a') as usize);
            if ai < 26 && bi < 26 { m[ai][bi] = 0.3; m[bi][ai] = 0.3; }
        }
        m
    }

    fn layout_pairs(layout: KeyboardLayout) -> (Vec<(u8, u8, f32)>, Vec<(u8, u8)>) {
        match layout {
            KeyboardLayout::Qwerty => (
                vec![
                    (b'q',b'w',0.7),(b'w',b'e',0.7),(b'e',b'r',0.7),(b'r',b't',0.7),
                    (b't',b'y',0.7),(b'y',b'u',0.7),(b'u',b'i',0.7),(b'i',b'o',0.7),
                    (b'o',b'p',0.7),(b'a',b's',0.8),(b's',b'd',0.8),(b'd',b'f',0.8),
                    (b'f',b'g',0.7),(b'g',b'h',0.7),(b'h',b'j',0.7),(b'j',b'k',0.7),
                    (b'k',b'l',0.7),(b'z',b'x',0.7),(b'x',b'c',0.7),(b'c',b'v',0.7),
                    (b'v',b'b',0.7),(b'b',b'n',0.7),(b'n',b'm',0.7),
                    (b'q',b'a',0.5),(b'w',b's',0.5),(b'e',b'd',0.5),(b'r',b'f',0.5),
                    (b't',b'g',0.5),(b'y',b'h',0.5),(b'u',b'j',0.5),(b'i',b'k',0.5),
                    (b'o',b'l',0.5),(b'a',b'z',0.4),(b's',b'x',0.4),(b'd',b'c',0.4),
                    (b'f',b'v',0.4),(b'g',b'b',0.4),(b'h',b'n',0.4),(b'j',b'm',0.4),
                ],
                vec![(b'e',b'c'),(b'r',b'v'),(b't',b'b'),(b'y',b'n'),
                     (b'u',b'm'),(b'i',b'k'),(b'w',b'x'),(b'q',b'z')],
            ),
            KeyboardLayout::Azerty => (
                vec![
                    (b'a',b'z',0.7),(b'z',b'e',0.7),(b'e',b'r',0.7),(b'r',b't',0.7),
                    (b't',b'y',0.7),(b'y',b'u',0.7),(b'u',b'i',0.7),(b'i',b'o',0.7),
                    (b'o',b'p',0.7),(b'q',b's',0.8),(b's',b'd',0.8),(b'd',b'f',0.8),
                    (b'f',b'g',0.7),(b'g',b'h',0.7),(b'h',b'j',0.7),(b'j',b'k',0.7),
                    (b'k',b'l',0.7),(b'l',b'm',0.7),(b'w',b'x',0.7),(b'x',b'c',0.7),
                    (b'c',b'v',0.7),(b'v',b'b',0.7),(b'b',b'n',0.7),
                    (b'a',b'q',0.5),(b'z',b's',0.5),(b'e',b'd',0.5),(b'r',b'f',0.5),
                    (b't',b'g',0.5),(b'y',b'h',0.5),(b'u',b'j',0.5),(b'i',b'k',0.5),
                    (b'o',b'l',0.5),
                ],
                vec![(b'e',b'c'),(b'r',b'v'),(b't',b'b'),(b'y',b'n'),
                     (b'u',b'm'),(b'z',b'x'),(b'a',b'w')],
            ),
            KeyboardLayout::Qwertz => (
                vec![
                    (b'q',b'w',0.7),(b'w',b'e',0.7),(b'e',b'r',0.7),(b'r',b't',0.7),
                    (b't',b'z',0.7),(b'z',b'u',0.7),(b'u',b'i',0.7),(b'i',b'o',0.7),
                    (b'o',b'p',0.7),(b'a',b's',0.8),(b's',b'd',0.8),(b'd',b'f',0.8),
                    (b'f',b'g',0.7),(b'g',b'h',0.7),(b'h',b'j',0.7),(b'j',b'k',0.7),
                    (b'k',b'l',0.7),(b'y',b'x',0.7),(b'x',b'c',0.7),(b'c',b'v',0.7),
                    (b'v',b'b',0.7),(b'b',b'n',0.7),(b'n',b'm',0.7),
                    (b'q',b'a',0.5),(b'w',b's',0.5),(b'e',b'd',0.5),(b'r',b'f',0.5),
                    (b't',b'g',0.5),(b'z',b'h',0.5),(b'u',b'j',0.5),(b'i',b'k',0.5),
                    (b'o',b'l',0.5),
                ],
                vec![(b'e',b'c'),(b'r',b'v'),(b't',b'b'),(b'z',b'n'),
                     (b'u',b'm'),(b'i',b'k'),(b'w',b'x'),(b'q',b'y')],
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical() { assert_eq!(TypoScorer::new(20).score(b"hello", b"hello").score, 1.0); }

    #[test]
    fn transposition_beats_random() {
        let s = TypoScorer::new(20);
        assert!(s.score(b"teh", b"the").score > s.score(b"txe", b"the").score);
    }

    #[test]
    fn early_error_penalized() {
        let s = TypoScorer::new(20);
        assert!(s.score(b"javascxipt", b"javascript").score > s.score(b"xavascript", b"javascript").score);
    }

    #[test]
    fn adjacent_beats_distant() {
        let s = TypoScorer::new(20);
        assert!(s.score(b"sbc", b"abc").score > s.score(b"pbc", b"abc").score);
    }

    #[test]
    fn symmetric() {
        let s = TypoScorer::new(10);
        assert_eq!(s.char_confusion(b'a', b's'), s.char_confusion(b's', b'a'));
    }
}
