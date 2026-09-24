// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

//! Text normalisation: case folding and diacritic folding with tables of our
//! own, and the offset utilities needed to report ranges in the caller's text.
//!
//! [`Normalizer`] maps a `&str` to the form the index stores; apply the same
//! normaliser to terms and queries.
//!
//! What is folded, and what is not, is fixed in `docs/design/unicode.md`
//! (section 2). In short, the default normaliser:
//!
//! - drops diacritics of the letters of Latin-1 Supplement and Latin
//!   Extended-A (U+00C0 to U+017F), expands `æ`, `œ`, `ß`, `ĳ`, `þ` and the
//!   ligatures U+FB00 to U+FB06, and drops the combining marks U+0300 to U+036F;
//! - then folds case (Unicode full case folding) in ASCII and those blocks.
//!
//! Everything else (other scripts, digits, punctuation, spaces, control
//! characters, joiners, emoji) is copied unchanged. There is no locale: `I`
//! always folds to `i`.
//!
//! ```
//! use keyhammer::text::Normalizer;
//!
//! let n = Normalizer::new();
//! assert_eq!(n.normalize("São Paulo"), "sao paulo");
//! assert_eq!(n.normalize("Straße"), "strasse");
//! assert_eq!(n.normalize("Œuvre"), "oeuvre");
//! assert_eq!(n.normalize("ﬁnal"), "final");
//! // Case only: accents are kept.
//! let case = Normalizer::new().with_diacritic_folding(false);
//! assert_eq!(case.normalize("CAFÉ"), "café");
//! ```

use alloc::string::String;
use alloc::vec::Vec;
use core::ops::Range;

mod tables;

use tables::{BASE, CASE_FOLD, FIRST, LAST};

/// The Combining Diacritical Marks block, dropped by diacritic folding.
const COMBINING: Range<u32> = 0x300..0x370;

/// The letters of a Latin ligature U+FB00 to U+FB06, or `None`.
fn ligature(cp: u32) -> Option<&'static str> {
    Some(match cp {
        0xFB00 => "ff",
        0xFB01 => "fi",
        0xFB02 => "fl",
        0xFB03 => "ffi",
        0xFB04 => "ffl",
        0xFB05 | 0xFB06 => "st",
        _ => return None,
    })
}

/// Folds case and diacritics, identically for terms and queries.
///
/// Both foldings are on by default ([`Normalizer::new`]). Diacritic folding
/// runs first, then case folding; the result depends only on the input and
/// the two flags, and normalising twice gives the same result as normalising
/// once.
///
/// ```
/// use keyhammer::text::Normalizer;
///
/// let n = Normalizer::new();
/// assert_eq!(n.normalize("Ação"), "acao");
/// assert_eq!(n.normalize("naïve CAFÉ"), "naive cafe");
/// // A decomposed accent (e + U+0301) is dropped too.
/// assert_eq!(n.normalize("cafe\u{301}"), "cafe");
/// // No Turkish tailoring: every i-like letter becomes i.
/// assert_eq!(n.normalize("Iıİi"), "iiii");
/// // Other scripts are copied unchanged.
/// assert_eq!(n.normalize("Ωmega"), "Ωmega");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Normalizer {
    case: bool,
    diacritics: bool,
}

impl Default for Normalizer {
    fn default() -> Self {
        Self::new()
    }
}

impl Normalizer {
    /// Case folding and diacritic folding.
    pub const fn new() -> Self {
        Self {
            case: true,
            diacritics: true,
        }
    }

    /// The same normaliser with case folding on or off.
    #[must_use]
    pub const fn with_case_folding(self, on: bool) -> Self {
        Self { case: on, ..self }
    }

    /// The same normaliser with diacritic folding on or off.
    #[must_use]
    pub const fn with_diacritic_folding(self, on: bool) -> Self {
        Self {
            diacritics: on,
            ..self
        }
    }

    /// Whether case is folded.
    pub const fn folds_case(self) -> bool {
        self.case
    }

    /// Whether diacritics are folded.
    pub const fn folds_diacritics(self) -> bool {
        self.diacritics
    }

    /// Calls `emit` with the characters `c` normalises to (none, one or up to
    /// three), in order.
    ///
    /// ```
    /// use keyhammer::text::Normalizer;
    /// let mut out = String::new();
    /// Normalizer::new().normalize_char('ß', |c| out.push(c));
    /// assert_eq!(out, "ss");
    /// ```
    pub fn normalize_char(self, c: char, mut emit: impl FnMut(char)) {
        let cp = u32::from(c);
        if self.diacritics {
            if COMBINING.contains(&cp) {
                return;
            }
            if (FIRST..=LAST).contains(&cp) {
                let [a, b] = BASE[(cp - FIRST) as usize];
                if a != 0 {
                    self.fold_case(char::from(a), &mut emit);
                    if b != 0 {
                        self.fold_case(char::from(b), &mut emit);
                    }
                    return;
                }
            }
            if let Some(l) = ligature(cp) {
                l.chars().for_each(emit);
                return;
            }
        }
        self.fold_case(c, &mut emit);
    }

    fn fold_case(self, c: char, emit: &mut impl FnMut(char)) {
        if !self.case {
            emit(c);
            return;
        }
        let cp = u32::from(c);
        match cp {
            0..0x80 => emit(c.to_ascii_lowercase()),
            0xB5 => emit('\u{3BC}'),
            0xDF => {
                emit('s');
                emit('s');
            }
            0x130 => {
                emit('i');
                emit('\u{307}');
            }
            0x149 => {
                emit('\u{2BC}');
                emit('n');
            }
            FIRST..=LAST => {
                let f = u32::from(CASE_FOLD[(cp - FIRST) as usize]);
                emit(char::from_u32(f).unwrap_or(c));
            }
            _ => match ligature(cp) {
                Some(l) => l.chars().for_each(emit),
                None => emit(c),
            },
        }
    }

    /// The normalised form of `s`.
    pub fn normalize(&self, s: &str) -> String {
        let mut out = String::new();
        self.normalize_into(s, &mut out);
        out
    }

    /// Clears `out` and writes the normalised form of `s` into it, reusing its
    /// allocation.
    pub fn normalize_into(&self, s: &str, out: &mut String) {
        out.clear();
        if s.is_ascii() {
            out.push_str(s);
            if self.case {
                out.make_ascii_lowercase();
            }
            return;
        }
        out.reserve(s.len());
        for c in s.chars() {
            self.normalize_char(c, |o| out.push(o));
        }
    }

    /// The normalised form of `s`, and in `map` where each of its code points
    /// came from (see [`SourceMap`]). `map` is cleared first.
    ///
    /// ```
    /// use keyhammer::text::{Normalizer, SourceMap};
    ///
    /// let mut map = SourceMap::new();
    /// let n = Normalizer::new().normalize_mapped("Maße", &mut map);
    /// assert_eq!(n, "masse");
    /// // "ss" (output code points 2..4) comes from "ß" (input code point 2).
    /// assert_eq!(map.to_source(2..4), Some(2..3));
    /// assert_eq!(map.to_source(3..5), Some(2..4));
    /// ```
    pub fn normalize_mapped(&self, s: &str, map: &mut SourceMap) -> String {
        map.from.clear();
        let mut out = String::with_capacity(s.len());
        let mut n = 0;
        for (i, c) in s.chars().enumerate() {
            self.normalize_char(c, |o| {
                out.push(o);
                map.from.push(i);
            });
            n = i + 1;
        }
        map.source_len = n;
        out
    }
}

/// Where each code point of a normalised string came from in its source.
///
/// Filled by [`Normalizer::normalize_mapped`]. All positions are code-point
/// indices; [`utf8_range`] and [`utf16_range`] convert a range of the source
/// to bytes or UTF-16 units.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceMap {
    from: Vec<usize>,
    source_len: usize,
}

impl SourceMap {
    /// An empty map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of code points of the normalised string.
    pub fn len(&self) -> usize {
        self.from.len()
    }

    /// Whether the normalised string is empty.
    pub fn is_empty(&self) -> bool {
        self.from.is_empty()
    }

    /// Number of code points of the source.
    pub fn source_len(&self) -> usize {
        self.source_len
    }

    /// The source code point that produced normalised code point `i`.
    pub fn source_of(&self, i: usize) -> Option<usize> {
        self.from.get(i).copied()
    }

    /// The smallest range of source code points that produced the normalised
    /// code points `range`. It is extended at the end over source characters
    /// that produced nothing (a dropped combining mark after the last letter
    /// belongs to that letter). An empty range maps to an empty range at the
    /// matching source position. `None` if `range` is not within
    /// `0..=self.len()` or is reversed.
    pub fn to_source(&self, range: Range<usize>) -> Option<Range<usize>> {
        let (a, b) = (range.start, range.end);
        let n = self.from.len();
        if a > b || b > n {
            return None;
        }
        let at = |i: usize| self.from.get(i).copied().unwrap_or(self.source_len);
        if a == b {
            let p = at(a);
            return Some(p..p);
        }
        let start = self.from[a];
        let last = self.from[b - 1];
        let next = at(b);
        let end = if next > last { next } else { last + 1 };
        Some(start..end)
    }
}

/// The byte offset in `s` of code point index `cp`: `s.len()` for
/// `cp == s.chars().count()`, `None` beyond that.
///
/// ```
/// assert_eq!(keyhammer::text::utf8_offset("aé€b", 3), Some(6));
/// ```
pub fn utf8_offset(s: &str, cp: usize) -> Option<usize> {
    match s.char_indices().nth(cp) {
        Some((b, _)) => Some(b),
        None if s.chars().count() == cp => Some(s.len()),
        None => None,
    }
}

/// The UTF-16 offset (in 16-bit units, as JavaScript counts) in `s` of code
/// point index `cp`; `None` beyond the end.
///
/// ```
/// // The emoji is one code point and two UTF-16 units.
/// assert_eq!(keyhammer::text::utf16_offset("a😀b", 2), Some(3));
/// ```
pub fn utf16_offset(s: &str, cp: usize) -> Option<usize> {
    let mut units = 0;
    let mut chars = s.chars();
    for _ in 0..cp {
        units += chars.next()?.len_utf16();
    }
    Some(units)
}

/// A range of code points of `s` as a range of bytes; `None` if it is not
/// within `s` or is reversed.
pub fn utf8_range(s: &str, range: Range<usize>) -> Option<Range<usize>> {
    if range.start > range.end {
        return None;
    }
    Some(utf8_offset(s, range.start)?..utf8_offset(s, range.end)?)
}

/// A range of code points of `s` as a range of UTF-16 units; `None` if it is
/// not within `s` or is reversed.
pub fn utf16_range(s: &str, range: Range<usize>) -> Option<Range<usize>> {
    if range.start > range.end {
        return None;
    }
    Some(utf16_offset(s, range.start)?..utf16_offset(s, range.end)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    const MODES: [Normalizer; 4] = [
        Normalizer::new(),
        Normalizer::new().with_diacritic_folding(false),
        Normalizer::new().with_case_folding(false),
        Normalizer::new()
            .with_case_folding(false)
            .with_diacritic_folding(false),
    ];

    fn all_chars() -> impl Iterator<Item = char> {
        // Under Miri only a sample: every 97th scalar value plus the tables.
        let step = if cfg!(miri) { 97 } else { 1 };
        (0..=0x10_FFFFu32)
            .step_by(step)
            .chain(0xB5..0x180)
            .filter_map(char::from_u32)
    }

    fn norm(n: Normalizer, c: char) -> String {
        let mut s = String::new();
        n.normalize_char(c, |o| s.push(o));
        s
    }

    #[test]
    fn every_mode_is_idempotent_on_every_scalar_value() {
        for n in MODES {
            for c in all_chars() {
                let once = norm(n, c);
                assert!(once.chars().count() <= 3, "{c:?}");
                assert_eq!(n.normalize(&once), once, "{n:?} {c:?} -> {once:?}");
            }
        }
    }

    #[test]
    fn the_identity_mode_changes_nothing() {
        let n = MODES[3];
        for c in all_chars() {
            assert_eq!(norm(n, c), c.to_string());
        }
    }

    #[test]
    fn case_folding_agrees_with_core_lowercase_except_full_foldings() {
        let n = Normalizer::new().with_diacritic_folding(false);
        for c in all_chars() {
            let cp = u32::from(c);
            let covered = cp < 0x80 || (FIRST..=LAST).contains(&cp);
            let got = norm(n, c);
            let want = match cp {
                0xB5 => "\u{3BC}".to_string(),
                0xDF => "ss".to_string(),
                0x149 => "\u{2BC}n".to_string(),
                0x17F => "s".to_string(),
                0xFB00..=0xFB06 => ligature(cp).unwrap_or_default().to_string(),
                _ if covered => c.to_lowercase().collect(),
                _ => c.to_string(),
            };
            assert_eq!(got, want, "{c:?} U+{cp:04X}");
        }
    }

    #[test]
    fn diacritic_folding_leaves_only_ascii_for_the_latin_letters() {
        let n = Normalizer::new();
        for cp in FIRST..=LAST {
            let Some(c) = char::from_u32(cp) else {
                continue;
            };
            let out = norm(n, c);
            if cp == 0xD7 || cp == 0xF7 {
                assert_eq!(out, c.to_string(), "× and ÷ are not letters");
            } else {
                assert!(
                    !out.is_empty() && out.is_ascii() && out == out.to_lowercase(),
                    "U+{cp:04X} -> {out:?}"
                );
            }
        }
    }

    #[test]
    fn the_documented_policies_hold() {
        let n = Normalizer::new();
        for (input, want) in [
            ("São Paulo", "sao paulo"),
            ("AÇÃO", "acao"),
            ("Ç", "c"),
            ("café", "cafe"),
            ("naïve", "naive"),
            ("Straße", "strasse"),
            ("MÜLLER", "muller"),
            ("Æsir", "aesir"),
            ("cœur", "coeur"),
            ("Œ", "oe"),
            ("ﬁ ﬂ ﬀ ﬃ ﬄ ﬅ ﬆ", "fi fl ff ffi ffl st st"),
            ("ĳ", "ij"),
            ("Þór", "thor"),
            ("ðø", "do"),
            ("Łódź", "lodz"),
            ("ŉ", "'n"),
            ("ĸ", "q"),
            ("ŋ", "n"),
            ("ſ", "s"),
            ("İstanbul", "istanbul"),
            ("ıI", "ii"),
            ("µ", "\u{3BC}"),
            ("e\u{301}\u{308}", "e"),
            ("a\u{200D}b\u{200F}\u{0}", "a\u{200D}b\u{200F}\u{0}"),
            ("😀ÉÀ", "😀ea"),
            ("ΑΩ", "ΑΩ"),
        ] {
            assert_eq!(n.normalize(input), want, "{input:?}");
        }
        let case = Normalizer::new().with_diacritic_folding(false);
        for (input, want) in [
            ("CAFÉ", "café"),
            ("Straße", "strasse"),
            ("İ", "i\u{307}"),
            ("I", "i"),
            ("ı", "ı"),
            ("Æ", "æ"),
            ("ﬁ", "fi"),
            ("ŉ", "\u{2BC}n"),
            ("e\u{301}", "e\u{301}"),
        ] {
            assert_eq!(case.normalize(input), want, "{input:?}");
        }
        let accents = Normalizer::new().with_case_folding(false);
        for (input, want) in [("CAFÉ", "CAFE"), ("Æ", "AE"), ("ß", "ss"), ("ﬁ", "fi")] {
            assert_eq!(accents.normalize(input), want, "{input:?}");
        }
    }

    #[test]
    fn strings_normalise_as_their_characters() {
        let s = "Ação ß İ ŉ ﬃ e\u{301} ı Ω 😀 \u{0}";
        for n in MODES {
            let mut want = String::new();
            for c in s.chars() {
                want.push_str(&norm(n, c));
            }
            assert_eq!(n.normalize(s), want);
            let mut buf = String::from("stale");
            n.normalize_into(s, &mut buf);
            assert_eq!(buf, want);
            let mut map = SourceMap::new();
            assert_eq!(n.normalize_mapped(s, &mut map), want);
            assert_eq!(map.len(), want.chars().count());
            assert_eq!(map.source_len(), s.chars().count());
            assert_eq!(
                n.normalize("ABC xyz"),
                n.normalize_mapped("ABC xyz", &mut map)
            );
        }
    }

    #[test]
    fn source_maps_cover_expansions_and_dropped_marks() {
        let n = Normalizer::new();
        let mut map = SourceMap::new();
        // Source: c a f e U+0301 ß ; output: c a f e s s
        let out = n.normalize_mapped("cafe\u{301}ß", &mut map);
        assert_eq!(out, "cafess");
        assert_eq!(map.source_of(4), Some(5));
        assert_eq!(map.source_of(6), None);
        assert_eq!(map.to_source(0..4), Some(0..5)); // "cafe" takes its accent
        assert_eq!(map.to_source(3..4), Some(3..5));
        assert_eq!(map.to_source(4..5), Some(5..6)); // half of "ss" is all of ß
        assert_eq!(map.to_source(5..6), Some(5..6));
        assert_eq!(map.to_source(0..6), Some(0..6));
        assert_eq!(map.to_source(2..2), Some(2..2));
        assert_eq!(map.to_source(6..6), Some(6..6));
        assert_eq!(map.to_source(0..7), None);
        assert_eq!(map.to_source(Range { start: 3, end: 2 }), None);
        // Nothing produced at all.
        let out = n.normalize_mapped("\u{301}\u{302}", &mut map);
        assert!(out.is_empty() && map.is_empty());
        assert_eq!(map.to_source(0..0), Some(2..2));
    }

    #[test]
    fn source_maps_agree_with_brute_force() {
        let s = "ÀÉ ß\u{301}ﬃxĲ\u{300}ŉ😀";
        for n in MODES {
            let mut map = SourceMap::new();
            let out: Vec<char> = n.normalize_mapped(s, &mut map).chars().collect();
            let src: Vec<char> = s.chars().collect();
            for a in 0..=out.len() {
                for b in a..=out.len() {
                    let r = map.to_source(a..b).unwrap_or(0..0);
                    // Normalising the source range yields a superstring of the
                    // output range, and the range is not wider than needed at
                    // the start.
                    let part: String = src[r.clone()].iter().collect();
                    let got: String = n.normalize(&part);
                    let want: String = out[a..b].iter().collect();
                    assert!(got.contains(&want), "{n:?} {a}..{b} -> {r:?}");
                    if a < b {
                        assert_eq!(Some(r.start), map.source_of(a));
                    }
                }
            }
        }
    }

    #[test]
    fn offsets_in_bytes_and_utf16_units() {
        let s = "aé€😀b";
        let bytes = [0, 1, 3, 6, 10, 11];
        let units = [0, 1, 2, 3, 5, 6];
        for cp in 0..=5 {
            assert_eq!(utf8_offset(s, cp), Some(bytes[cp]));
            assert_eq!(utf16_offset(s, cp), Some(units[cp]));
        }
        assert_eq!(utf8_offset(s, 6), None);
        assert_eq!(utf16_offset(s, 6), None);
        assert_eq!(utf8_range(s, 1..4), Some(1..10));
        assert_eq!(utf16_range(s, 1..4), Some(1..5));
        assert_eq!(utf8_range(s, 4..9), None);
        assert_eq!(utf16_range(s, Range { start: 2, end: 1 }), None);
        assert_eq!(utf8_offset("", 0), Some(0));
        assert_eq!(utf16_offset("", 1), None);
        // Against the standard encoders.
        for cp in 0..=5 {
            let prefix: String = s.chars().take(cp).collect();
            assert_eq!(utf8_offset(s, cp), Some(prefix.len()));
            assert_eq!(utf16_offset(s, cp), Some(prefix.encode_utf16().count()));
        }
    }
}
