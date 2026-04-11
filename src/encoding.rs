/// Typo-optimal character encoding.
///
/// Maps each ASCII byte to a code where bit distance (popcount of XOR)
/// correlates with typo probability. Commonly confused characters get
/// codes with few differing bits. This bakes the confusion model directly
/// into the representation — string comparison becomes XOR + popcount.
///
/// # Example
///
/// ```
/// use keyhammer::{TypoEncoding, KeyboardLayout};
///
/// let enc = TypoEncoding::from_layout(KeyboardLayout::Qwerty);
/// let a = enc.encode_str(b"javascript");
/// let b = enc.encode_str(b"javoscript"); // a→o vowel swap
/// let c = enc.encode_str(b"javzscript"); // a→z unlikely
///
/// assert!(TypoEncoding::bit_distance(&a, &b) < TypoEncoding::bit_distance(&a, &c));
/// ```

use crate::scorer::KeyboardLayout;

pub struct TypoEncoding {
    table: [u8; 256],
    #[allow(dead_code)]
    decode: [u8; 256],
}

impl TypoEncoding {
    /// Generate encoding from a keyboard layout.
    pub fn from_layout(layout: KeyboardLayout) -> Self {
        let groups = match layout {
            KeyboardLayout::Qwerty => Self::qwerty_groups(),
            KeyboardLayout::Azerty => Self::azerty_groups(),
            KeyboardLayout::Qwertz => Self::qwertz_groups(),
        };
        Self::from_groups(&groups)
    }

    fn from_groups(groups: &[Vec<u8>]) -> Self {
        let mut table = [0u8; 256];
        let mut decode = [0u8; 256];
        let mut code: u8 = 1;

        for group in groups {
            let base = code;
            for (i, &ch) in group.iter().enumerate() {
                let assigned = base.wrapping_add(i as u8);
                table[ch as usize] = assigned;
                table[ch.to_ascii_uppercase() as usize] = assigned;
                decode[assigned as usize] = ch;
            }
            code = (code.wrapping_add(group.len() as u8) + 7) & !7;
        }

        let digit_base = code;
        for d in b'0'..=b'9' {
            let assigned = digit_base + (d - b'0');
            table[d as usize] = assigned;
            decode[assigned as usize] = d;
        }

        Self { table, decode }
    }

    #[inline]
    pub fn encode_byte(&self, b: u8) -> u8 {
        self.table[b as usize]
    }

    /// Encode a byte slice into a new buffer.
    #[inline]
    pub fn encode_str(&self, s: &[u8]) -> Vec<u8> {
        s.iter().map(|&b| self.table[b as usize]).collect()
    }

    /// Encode into an existing buffer (avoids allocation).
    #[inline]
    pub fn encode_into(&self, s: &[u8], out: &mut Vec<u8>) {
        out.clear();
        out.extend(s.iter().map(|&b| self.table[b as usize]));
    }

    /// Total bit distance between two encoded strings (XOR + popcount).
    #[inline]
    pub fn bit_distance(a: &[u8], b: &[u8]) -> u32 {
        let len = a.len().min(b.len());
        let mut total: u32 = 0;

        let chunks = len / 8;
        let mut i = 0;
        for _ in 0..chunks {
            let va = u64::from_ne_bytes(a[i..i + 8].try_into().unwrap());
            let vb = u64::from_ne_bytes(b[i..i + 8].try_into().unwrap());
            total += (va ^ vb).count_ones();
            i += 8;
        }
        for j in i..len {
            total += (a[j] ^ b[j]).count_ones();
        }

        total += a.len().abs_diff(b.len()) as u32 * 8;
        total
    }

    /// Normalized bit distance: 0.0 (identical) to 1.0 (completely different).
    #[inline]
    pub fn normalized_distance(a: &[u8], b: &[u8]) -> f32 {
        let bits = Self::bit_distance(a, b);
        let max_bits = a.len().max(b.len()) as u32 * 8;
        if max_bits == 0 { return 0.0; }
        (bits as f32 / max_bits as f32).min(1.0)
    }

    fn qwerty_groups() -> Vec<Vec<u8>> {
        vec![
            vec![b'a', b'e', b'i', b'o', b'u'],
            vec![b's', b'd', b'f'],
            vec![b'j', b'k', b'l'],
            vec![b'q', b'w', b'r', b't'],
            vec![b'y', b'p'],
            vec![b'b', b'v', b'g'],
            vec![b'c', b'x', b'z'],
            vec![b'n', b'm'],
            vec![b'h'],
        ]
    }

    fn azerty_groups() -> Vec<Vec<u8>> {
        vec![
            vec![b'a', b'e', b'i', b'o', b'u'],
            vec![b'q', b's', b'd', b'f'],
            vec![b'j', b'k', b'l', b'm'],
            vec![b'a', b'z', b'r', b't'],
            vec![b'y', b'p'],
            vec![b'b', b'v', b'g'],
            vec![b'c', b'x', b'w'],
            vec![b'n'],
            vec![b'h'],
        ]
    }

    fn qwertz_groups() -> Vec<Vec<u8>> {
        vec![
            vec![b'a', b'e', b'i', b'o', b'u'],
            vec![b's', b'd', b'f'],
            vec![b'j', b'k', b'l'],
            vec![b'q', b'w', b'r', b't'],
            vec![b'z', b'p'],
            vec![b'b', b'v', b'g'],
            vec![b'c', b'x', b'y'],
            vec![b'n', b'm'],
            vec![b'h'],
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vowels_close() {
        let enc = TypoEncoding::from_layout(KeyboardLayout::Qwerty);
        let ae = (enc.encode_byte(b'a') ^ enc.encode_byte(b'e')).count_ones();
        let az = (enc.encode_byte(b'a') ^ enc.encode_byte(b'z')).count_ones();
        assert!(ae < az);
    }

    #[test]
    fn identical_zero() {
        let enc = TypoEncoding::from_layout(KeyboardLayout::Qwerty);
        let a = enc.encode_str(b"hello");
        assert_eq!(TypoEncoding::bit_distance(&a, &a), 0);
    }

    #[test]
    fn case_insensitive() {
        let enc = TypoEncoding::from_layout(KeyboardLayout::Qwerty);
        assert_eq!(enc.encode_byte(b'a'), enc.encode_byte(b'A'));
    }

    #[test]
    fn neighbors_closer() {
        let enc = TypoEncoding::from_layout(KeyboardLayout::Qwerty);
        let sd = (enc.encode_byte(b's') ^ enc.encode_byte(b'd')).count_ones();
        let sp = (enc.encode_byte(b's') ^ enc.encode_byte(b'p')).count_ones();
        assert!(sd <= sp);
    }

    #[test]
    fn vowel_swap_closer_than_unlikely() {
        let enc = TypoEncoding::from_layout(KeyboardLayout::Qwerty);
        let js = enc.encode_str(b"javascript");
        let vo = enc.encode_str(b"javoscript");
        let zs = enc.encode_str(b"javzscript");
        assert!(TypoEncoding::bit_distance(&js, &vo) < TypoEncoding::bit_distance(&js, &zs));
    }

    #[test]
    fn length_penalized() {
        let enc = TypoEncoding::from_layout(KeyboardLayout::Qwerty);
        let d = TypoEncoding::bit_distance(&enc.encode_str(b"hello"), &enc.encode_str(b"hell"));
        assert!(d >= 8);
    }

    #[test]
    fn normalized_range() {
        let enc = TypoEncoding::from_layout(KeyboardLayout::Qwerty);
        let a = enc.encode_str(b"abc");
        let same = enc.encode_str(b"abc");
        let diff = enc.encode_str(b"xyz");
        assert_eq!(TypoEncoding::normalized_distance(&a, &same), 0.0);
        assert!(TypoEncoding::normalized_distance(&a, &diff) > 0.0);
    }

    #[test]
    fn digits_grouped() {
        let enc = TypoEncoding::from_layout(KeyboardLayout::Qwerty);
        let d01 = (enc.encode_byte(b'0') ^ enc.encode_byte(b'1')).count_ones();
        let d0a = (enc.encode_byte(b'0') ^ enc.encode_byte(b'a')).count_ones();
        assert!(d01 < d0a);
    }
}
