// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

//! Fixed-point edit costs (unit = 16) and keyboard-layout adjacency.
//!
//! Integer costs make the search bounds exact and the results identical on
//! every platform. All values below are provisional until the calibration
//! phase.

/// A fixed-point edit cost (16 = one ordinary edit).
pub type Cost = u16;

/// The cost of one ordinary edit. Every cost in the table below is expressed
/// in these units: cheaper edits (a neighbouring key, a doubled letter, a
/// transposition) cost a fraction of it, edits on the first byte cost more.
pub const COST_UNIT: Cost = 16;

/// The weighted `cost` rounded up to whole units of [`COST_UNIT`]: 0 stays 0,
/// 1 to 16 is one unit, 17 to 32 two units, and so on.
///
/// This is not a count of edits. The x1.5 factor on the first query byte
/// makes an ordinary (non-neighbouring-key) edit there cost 24, i.e. two
/// units, while a neighbouring-key substitution there costs 12 (one unit) and
/// a transposition 18 (two units); two cheap edits (8 + 8) count as one.
#[inline]
pub fn whole_units(cost: Cost) -> Cost {
    cost.div_ceil(COST_UNIT)
}

/// Sentinel for "unreachable or over budget". `INF` plus any single edit cost
/// still fits in a `u16`.
pub const INF: Cost = 30_000;

/// Bit that represents the character class of symbol `s` (used by subtree
/// signatures). Symbols are Unicode scalar values (see `docs/design/unicode.md`).
///
/// - ASCII (`s < 128`): bit `s & 63`, so ASCII characters that agree modulo 64
///   share a class (`a` (97) and `!` (33), for instance, or digits and `p`-`y`).
///   The letters `a`-`z` take the classes 33 to 58 and are distinct from one
///   another.
/// - Everything else: one of the 38 classes the letters `a`-`z` never use (0 to
///   32 and 59 to 63), chosen by `s % 38`, so that an accented letter or a
///   letter of another script never hides a missing `a`-`z` letter.
///
/// The collisions are harmless: the signatures only bound the search, and a
/// collision can make a bound weaker (a class looks present when it is not),
/// never wrong. The proof needs only that equal symbols have equal classes.
///
/// # Examples
///
/// ```
/// use keyhammer::cost::symbol_class;
///
/// assert_ne!(symbol_class('a'.into()), symbol_class('b'.into()));
/// assert_eq!(symbol_class('a'.into()), symbol_class('!'.into())); // 97 and 33, modulo 64
/// // No non-ASCII symbol shares a class with a letter a-z.
/// let letters = ('a'..='z').fold(0, |m, c| m | symbol_class(c.into()));
/// assert_eq!(symbol_class('é'.into()) & letters, 0);
/// ```
#[inline]
pub fn symbol_class(s: u32) -> u64 {
    /// The classes that no letter `a`-`z` uses.
    const FREE: [u8; 38] = [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
        25, 26, 27, 28, 29, 30, 31, 32, 59, 60, 61, 62, 63,
    ];
    if s < 128 {
        1u64 << (s & 63)
    } else {
        1u64 << FREE[(s % 38) as usize]
    }
}

/// The class of the byte `b` read as the code point U+0000 to U+00FF: the same
/// as [`symbol_class`]`(b.into())`. For ASCII that is bit `b & 63`.
///
/// # Examples
///
/// ```
/// use keyhammer::cost::class;
///
/// assert_ne!(class(b'a'), class(b'b'));
/// assert_eq!(class(b'a'), class(b'!')); // 97 and 33 are equal modulo 64
/// ```
#[inline]
pub fn class(b: u8) -> u64 {
    symbol_class(u32::from(b))
}

/// One row of a keyboard [`Layout`]: the keys left to right and where the
/// first one starts.
///
/// Positions are in half key widths, so key `i` is at `offset + 2 * i`. Every
/// key takes part in the cost model, letters `a` to `z` and others (`ç`, `ù`,
/// `;`, ...) alike.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Row {
    offset: u8,
    keys: &'static str,
}

impl Row {
    #[cfg(test)]
    fn keys(&self) -> &'static str {
        self.keys
    }
}

const fn row(offset: u8, keys: &'static str) -> Row {
    Row { offset, keys }
}

const QWERTY: [Row; 3] = [row(0, "qwertyuiop"), row(1, "asdfghjkl"), row(2, "zxcvbnm")];
const QWERTZ: [Row; 3] = [
    row(0, "qwertzuiopü"),
    row(1, "asdfghjklöä"),
    row(2, "yxcvbnm"),
];
const AZERTY: [Row; 3] = [
    row(0, "azertyuiop"),
    row(1, "qsdfghjklmù"),
    row(2, "wxcvbn"),
];
const ABNT2: [Row; 3] = [
    row(0, "qwertyuiop"),
    row(1, "asdfghjklç"),
    row(2, "zxcvbnm,.;"),
];
const DVORAK: [Row; 3] = [
    row(0, "',.pyfgcrl"),
    row(1, "aoeuidhtns"),
    row(2, ";qjkxbmwvz"),
];
const COLEMAK: [Row; 3] = [
    row(0, "qwfpgjluy;"),
    row(1, "arstdhneio"),
    row(2, "zxcvbkm"),
];

/// Walks every unordered pair of neighbouring keys of `rows` (the rule
/// described on [`Layout`]), at compile time. Pairs of two letters `a`-`z`
/// go into the bit sets `adj`; every other pair is written to `extra` while
/// it has room. Returns the number of other pairs, written or not.
const fn walk(rows: &[Row; 3], adj: &mut [u32; 26], extra: &mut [(u32, u32)]) -> usize {
    let mut n = 0;
    let mut r = 0;
    while r < 3 {
        let a_row = rows[r].keys.as_bytes();
        let mut i = 0; // key index (a character, not a byte)
        let mut ba = 0; // byte index of key i
        while ba < a_row.len() {
            let xa = rows[r].offset as usize + 2 * i;
            let ka = decode_key(a_row, ba);
            // Next key in the same row.
            let bn = next_key(a_row, ba);
            if bn < a_row.len() {
                n = link(adj, extra, n, ka, decode_key(a_row, bn));
            }
            if r + 1 < 3 {
                let b_row = rows[r + 1].keys.as_bytes();
                let mut j = 0;
                let mut bb = 0;
                while bb < b_row.len() {
                    let xb = rows[r + 1].offset as usize + 2 * j;
                    if xa.abs_diff(xb) <= 1 {
                        n = link(adj, extra, n, ka, decode_key(b_row, bb));
                    }
                    bb = next_key(b_row, bb);
                    j += 1;
                }
            }
            ba = bn;
            i += 1;
        }
        r += 1;
    }
    n
}

/// Neighbour bit sets of the letters `a`-`z` of one layout.
const fn table(rows: &[Row; 3]) -> [u32; 26] {
    let mut adj = [0u32; 26];
    walk(rows, &mut adj, &mut []);
    adj
}

/// Number of neighbour pairs of one layout that involve a key outside `a`-`z`.
const fn extra_count(rows: &[Row; 3]) -> usize {
    walk(rows, &mut [0u32; 26], &mut [])
}

/// The neighbour pairs of one layout that involve a key outside `a`-`z`.
const fn extra<const N: usize>(rows: &[Row; 3]) -> [(u32, u32); N] {
    let mut out = [(0u32, 0u32); N];
    walk(rows, &mut [0u32; 26], &mut out);
    out
}

/// The code point of the UTF-8 key starting at byte `b`.
const fn decode_key(bytes: &[u8], b: usize) -> u32 {
    let x = bytes[b] as u32;
    if x < 0x80 {
        x
    } else if x < 0xE0 {
        ((x & 0x1F) << 6) | cont(bytes, b + 1)
    } else if x < 0xF0 {
        ((x & 0x0F) << 12) | (cont(bytes, b + 1) << 6) | cont(bytes, b + 2)
    } else {
        ((x & 0x07) << 18)
            | (cont(bytes, b + 1) << 12)
            | (cont(bytes, b + 2) << 6)
            | cont(bytes, b + 3)
    }
}

/// The six payload bits of the UTF-8 continuation byte at `i`.
const fn cont(bytes: &[u8], i: usize) -> u32 {
    (bytes[i] & 0x3F) as u32
}

/// Byte index of the key after the one starting at `b` (keys are UTF-8).
const fn next_key(bytes: &[u8], b: usize) -> usize {
    let mut n = b + 1;
    while n < bytes.len() && bytes[n] & 0xC0 == 0x80 {
        n += 1;
    }
    n
}

/// Records the pair `a`, `b`: in `adj` if both are letters `a`-`z`, else as
/// pair number `n` in `extra` (if there is room). Returns the new count of
/// other pairs.
const fn link(adj: &mut [u32; 26], extra: &mut [(u32, u32)], n: usize, a: u32, b: u32) -> usize {
    let (ia, ib) = (a.wrapping_sub(0x61), b.wrapping_sub(0x61));
    if ia < 26 && ib < 26 {
        adj[ia as usize] |= 1 << ib;
        adj[ib as usize] |= 1 << ia;
        return n;
    }
    if n < extra.len() {
        extra[n] = (a, b);
    }
    n + 1
}

const QWERTY_ADJ: [u32; 26] = table(&QWERTY);
const QWERTZ_ADJ: [u32; 26] = table(&QWERTZ);
const AZERTY_ADJ: [u32; 26] = table(&AZERTY);
const ABNT2_ADJ: [u32; 26] = table(&ABNT2);
const DVORAK_ADJ: [u32; 26] = table(&DVORAK);
const COLEMAK_ADJ: [u32; 26] = table(&COLEMAK);

const QWERTY_EXTRA: [(u32, u32); extra_count(&QWERTY)] = extra(&QWERTY);
const QWERTZ_EXTRA: [(u32, u32); extra_count(&QWERTZ)] = extra(&QWERTZ);
const AZERTY_EXTRA: [(u32, u32); extra_count(&AZERTY)] = extra(&AZERTY);
const ABNT2_EXTRA: [(u32, u32); extra_count(&ABNT2)] = extra(&ABNT2);
const DVORAK_EXTRA: [(u32, u32); extra_count(&DVORAK)] = extra(&DVORAK);
const COLEMAK_EXTRA: [(u32, u32); extra_count(&COLEMAK)] = extra(&COLEMAK);

/// A physical keyboard layout, from which the neighbouring-key relation of a
/// [`CostModel`] is derived.
///
/// # Geometry
///
/// Only the three letter rows are modelled (top, home, bottom). Every key is
/// one unit wide and rows are shifted by half keys: offsets of 0, 0.5 and 1
/// key widths for every layout here. That rounds the real stagger of an ANSI
/// board (0, 0.25 and 0.75) so that the derived relation equals the one QWERTY
/// has always had in this crate. Two keys are neighbours when
///
/// - they are next to each other in a row, or
/// - they are in consecutive rows and their centres are at most half a key
///   apart (a key touches two keys of the next row, or one at the end of a
///   row).
///
/// The relation is symmetric and irreflexive by construction.
///
/// # What is and is not covered
///
/// Every modelled key counts, letters `a` to `z` and others alike: `ç` on
/// ABNT2 (next to `l` and `p`, and to `.` and `;` below it), `ù` on AZERTY
/// (after `m`), `ü` on QWERTZ (after `p`) and `ö`, `ä` (after `l`), and the
/// punctuation keys of the ABNT2 bottom row, of Dvorak and of Colemak. The
/// search compares Unicode code points (`docs/design/unicode.md`), so a
/// query with `ç` for a term with `l` is a neighbouring-key substitution on
/// ABNT2. Keys are lowercase: an uppercase letter is a different symbol,
/// with no neighbours, unless the text is case folded (see
/// [`crate::text::Normalizer`]). With diacritic folding on (the default),
/// `ç`, `ù`, `ü`, `ö` and `ä` never reach the search (they become `c`, `u`,
/// `u`, `o` and `a`), so these neighbours only matter without it.
///
/// Not modelled at all: the number row, the extra ISO key beside the left
/// shift, the punctuation columns after the letter rows of QWERTY, QWERTZ and
/// AZERTY and after the top row of ABNT2, modifiers, dead keys and AltGr
/// layers. The unit costs are the same for every layout and remain
/// provisional.
///
/// The pairs of two letters `a`-`z` of ABNT2 are those of QWERTY; the layouts
/// differ by `ç` and the punctuation keys.
///
/// # Examples
///
/// ```
/// use keyhammer::cost::Layout;
///
/// assert!(Layout::Qwerty.are_neighbours('a', 's'));
/// assert!(!Layout::Qwerty.are_neighbours('a', 'd'));
/// // ç sits next to l and p on ABNT2.
/// assert!(Layout::Abnt2.are_neighbours('l', 'ç'));
/// assert!(Layout::Abnt2.are_neighbours('ç', 'p'));
/// // ù follows m on AZERTY, ö follows l on QWERTZ.
/// assert!(Layout::Azerty.are_neighbours('m', 'ù'));
/// assert!(Layout::Qwertz.are_neighbours('l', 'ö'));
/// // QWERTZ swaps y and z: z now sits beside t, u, g and h.
/// assert!(Layout::Qwertz.are_neighbours('z', 't'));
/// assert!(!Layout::Qwerty.are_neighbours('z', 't'));
/// // Dvorak: home row is a o e u i d h t n s.
/// assert!(Layout::Dvorak.are_neighbours('u', 'i'));
/// // Colemak: home row is a r s t d h n e i o.
/// assert!(Layout::Colemak.are_neighbours('r', 's'));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Layout {
    /// ANSI QWERTY, the default.
    Qwerty,
    /// QWERTZ (German): `y` and `z` swapped relative to QWERTY, `ü` after `p`
    /// and `ö`, `ä` after `l`.
    Qwertz,
    /// AZERTY (French): `a`/`q` and `z`/`w` swapped, `m` and `ù` on the home
    /// row.
    Azerty,
    /// Brazilian ABNT2: the QWERTY letters, plus `ç` after `l` and the
    /// punctuation keys `,` `.` `;` after `m`.
    Abnt2,
    /// Dvorak (US).
    Dvorak,
    /// Colemak (US).
    Colemak,
}

impl Layout {
    /// Every layout known to this version of the crate.
    pub const ALL: &'static [Layout] = &[
        Layout::Qwerty,
        Layout::Qwertz,
        Layout::Azerty,
        Layout::Abnt2,
        Layout::Dvorak,
        Layout::Colemak,
    ];

    /// Lowercase name of the layout.
    ///
    /// ```
    /// assert_eq!(keyhammer::cost::Layout::Abnt2.name(), "abnt2");
    /// ```
    pub fn name(self) -> &'static str {
        match self {
            Layout::Qwerty => "qwerty",
            Layout::Qwertz => "qwertz",
            Layout::Azerty => "azerty",
            Layout::Abnt2 => "abnt2",
            Layout::Dvorak => "dvorak",
            Layout::Colemak => "colemak",
        }
    }

    /// The letter rows of the layout, top to bottom.
    pub(crate) fn rows(self) -> &'static [Row] {
        match self {
            Layout::Qwerty => &QWERTY,
            Layout::Qwertz => &QWERTZ,
            Layout::Azerty => &AZERTY,
            Layout::Abnt2 => &ABNT2,
            Layout::Dvorak => &DVORAK,
            Layout::Colemak => &COLEMAK,
        }
    }

    /// Whether keys `a` and `b` are neighbours on this layout. `false` for a
    /// key that is not on the modelled rows and for `a == b`. The cost model
    /// of [`CostModel::for_layout`] uses exactly this relation.
    ///
    /// ```
    /// use keyhammer::cost::Layout;
    ///
    /// assert!(Layout::Azerty.are_neighbours('q', 'z')); // not so on QWERTY
    /// assert!(!Layout::Qwerty.are_neighbours('q', 'z'));
    /// assert!(!Layout::Qwerty.are_neighbours('q', 'q'));
    /// ```
    pub fn are_neighbours(self, a: char, b: char) -> bool {
        let mut found = false;
        self.for_each_pair(|x, y| found |= (x == a && y == b) || (x == b && y == a));
        found
    }

    /// Calls `f` once for every unordered pair of neighbouring keys.
    fn for_each_pair(self, mut f: impl FnMut(char, char)) {
        let rows = self.rows();
        for (r, row) in rows.iter().enumerate() {
            for (i, a) in row.keys.chars().enumerate() {
                let xa = usize::from(row.offset) + 2 * i;
                if let Some(b) = row.keys.chars().nth(i + 1) {
                    f(a, b);
                }
                if let Some(below) = rows.get(r + 1) {
                    for (j, b) in below.keys.chars().enumerate() {
                        if xa.abs_diff(usize::from(below.offset) + 2 * j) <= 1 {
                            f(a, b);
                        }
                    }
                }
            }
        }
    }
}

/// Costs of the four edit operations.
///
/// Built for a keyboard [`Layout`] with [`CostModel::for_layout`]
/// ([`CostModel::qwerty`] is the default). The layout only decides which
/// substitutions are cheap; the minimum costs the search bounds use
/// ([`c_min`](Self::c_min), [`c_indel_min`](Self::c_indel_min),
/// [`c_transpose_min`](Self::c_transpose_min)) do not depend on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CostModel {
    sub: Cost,
    sub_adjacent: Cost,
    indel: Cost,
    indel_double: Cost,
    transpose: Cost,
    /// Neighbour bit sets of the letters `a`-`z`.
    adjacent: [u32; 26],
    /// Neighbour pairs that involve a key outside `a`-`z`.
    extra: &'static [(u32, u32)],
}

/// Edits on the first query byte are less likely, so they cost 1.5x.
#[inline]
fn at_start(cost: Cost, qpos: usize) -> Cost {
    if qpos == 0 { cost + cost / 2 } else { cost }
}

impl CostModel {
    /// The default model with a QWERTY keyboard. Same as
    /// `CostModel::for_layout(Layout::Qwerty)`.
    pub fn qwerty() -> Self {
        Self::for_layout(Layout::Qwerty)
    }

    /// A model whose cheap substitutions are the neighbouring keys of
    /// `layout` ([`Layout::are_neighbours`]); see [`Layout`] for what the
    /// geometry leaves out. Every other pair of characters costs an ordinary
    /// substitution. Does not panic.
    ///
    /// # Examples
    ///
    /// ```
    /// use keyhammer::cost::{CostModel, Layout};
    ///
    /// let qwerty = CostModel::for_layout(Layout::Qwerty);
    /// let azerty = CostModel::for_layout(Layout::Azerty);
    /// // q and z touch on AZERTY, not on QWERTY.
    /// assert_eq!(azerty.sub_cost(b'q', b'z', 1), 8);
    /// assert_eq!(qwerty.sub_cost(b'q', b'z', 1), 16);
    /// // m is next to n on QWERTY only.
    /// assert_eq!(qwerty.sub_cost(b'm', b'n', 1), 8);
    /// assert_eq!(azerty.sub_cost(b'm', b'n', 1), 16);
    /// // ABNT2 has the QWERTY letters, and ç next to l and p.
    /// let abnt2 = CostModel::for_layout(Layout::Abnt2);
    /// assert_eq!(abnt2.sub_cost('ç', 'l', 1), 8);
    /// assert_eq!(qwerty.sub_cost('ç', 'l', 1), 16);
    /// assert_eq!(abnt2.sub_cost('a', 's', 1), qwerty.sub_cost('a', 's', 1));
    /// ```
    pub fn for_layout(layout: Layout) -> Self {
        let (adjacent, extra): ([u32; 26], &'static [(u32, u32)]) = match layout {
            Layout::Qwerty => (QWERTY_ADJ, &QWERTY_EXTRA),
            Layout::Qwertz => (QWERTZ_ADJ, &QWERTZ_EXTRA),
            Layout::Azerty => (AZERTY_ADJ, &AZERTY_EXTRA),
            Layout::Abnt2 => (ABNT2_ADJ, &ABNT2_EXTRA),
            Layout::Dvorak => (DVORAK_ADJ, &DVORAK_EXTRA),
            Layout::Colemak => (COLEMAK_ADJ, &COLEMAK_EXTRA),
        };
        Self {
            sub: 16,
            sub_adjacent: 8,
            indel: 16,
            indel_double: 8,
            transpose: 12,
            adjacent,
            extra,
        }
    }

    #[inline]
    fn is_adjacent(&self, a: u32, b: u32) -> bool {
        let (ia, ib) = (a.wrapping_sub(0x61), b.wrapping_sub(0x61));
        if ia < 26 && ib < 26 {
            return self.adjacent[ia as usize] & (1u32 << ib) != 0;
        }
        self.extra
            .iter()
            .any(|&(x, y)| (x == a && y == b) || (x == b && y == a))
    }

    /// Cost of typing `q` where the term has `t`; `qpos` is the index of `q` in the query.
    ///
    /// Symbols are Unicode scalar values: a `u8` is read as U+0000 to U+00FF,
    /// a `char` or a `u32` as itself.
    #[inline]
    pub fn sub_cost<S: Into<u32>>(&self, q: S, t: S, qpos: usize) -> Cost {
        let (q, t) = (q.into(), t.into());
        if q == t {
            return 0;
        }
        let base = if self.is_adjacent(q, t) {
            self.sub_adjacent
        } else {
            self.sub
        };
        at_start(base, qpos)
    }

    /// Cost of deleting the query symbol at `qpos` (the user typed an extra one).
    #[inline]
    pub fn del_cost<S: PartialEq>(&self, q: &[S], qpos: usize) -> Cost {
        let doubled =
            qpos > 0 && matches!((q.get(qpos), q.get(qpos - 1)), (Some(a), Some(b)) if a == b);
        at_start(
            if doubled {
                self.indel_double
            } else {
                self.indel
            },
            qpos,
        )
    }

    /// Cost of inserting term symbol `t` (the user skipped it) when `qpos` query symbols are consumed.
    #[inline]
    pub fn ins_cost<S: PartialEq>(&self, t: S, prev_t: Option<S>, qpos: usize) -> Cost {
        let base = if prev_t == Some(t) {
            self.indel_double
        } else {
            self.indel
        };
        at_start(base, qpos)
    }

    /// Cost of swapping two adjacent symbols, the first at `qpos` in the query.
    #[inline]
    pub fn transpose_cost(&self, qpos: usize) -> Cost {
        at_start(self.transpose, qpos)
    }

    /// Smallest cost of any single edit.
    pub fn c_min(&self) -> Cost {
        self.sub
            .min(self.sub_adjacent)
            .min(self.indel)
            .min(self.indel_double)
            .min(self.transpose)
    }

    /// Smallest cost of an insertion or deletion.
    pub fn c_indel_min(&self) -> Cost {
        self.indel.min(self.indel_double)
    }

    /// Smallest cost of a transposition.
    pub fn c_transpose_min(&self) -> Cost {
        self.transpose
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    /// Runtime neighbour set of a-z pairs, from the same geometry.
    fn runtime_table(layout: Layout) -> [u32; 26] {
        let mut adj = [0u32; 26];
        layout.for_each_pair(|a, b| {
            if a.is_ascii_lowercase() && b.is_ascii_lowercase() {
                let (ia, ib) = (a as usize - 'a' as usize, b as usize - 'a' as usize);
                adj[ia] |= 1 << ib;
                adj[ib] |= 1 << ia;
            }
        });
        adj
    }

    #[test]
    fn compile_time_tables_equal_the_runtime_geometry() {
        for &layout in Layout::ALL {
            let cm = CostModel::for_layout(layout);
            assert_eq!(cm.adjacent, runtime_table(layout), "{}", layout.name());
            let mut runtime = Vec::new();
            layout.for_each_pair(|a, b| {
                if !(a.is_ascii_lowercase() && b.is_ascii_lowercase()) {
                    runtime.push((u32::from(a), u32::from(b)));
                }
            });
            assert_eq!(cm.extra, runtime.as_slice(), "{}", layout.name());
        }
    }

    #[test]
    fn the_cost_model_uses_exactly_the_layout_relation() {
        for &layout in Layout::ALL {
            let cm = CostModel::for_layout(layout);
            let mut keys: Vec<char> = layout
                .rows()
                .iter()
                .flat_map(|r| r.keys().chars())
                .collect();
            // Characters on no modelled key, and case variants of keys.
            keys.extend(['A', 'Ç', 'é', '1', ' ', 'ж', '\u{0}']);
            for &a in &keys {
                for &b in &keys {
                    let want = if a == b {
                        0
                    } else if layout.are_neighbours(a, b) {
                        8
                    } else {
                        16
                    };
                    assert_eq!(cm.sub_cost(a, b, 1), want, "{} {a:?} {b:?}", layout.name());
                }
            }
        }
    }

    #[test]
    fn the_new_keys_have_their_neighbours() {
        let pairs = |layout: Layout, key: char| -> Vec<char> {
            let mut v: Vec<char> = layout
                .rows()
                .iter()
                .flat_map(|r| r.keys().chars())
                .filter(|&o| layout.are_neighbours(key, o))
                .collect();
            v.sort_unstable();
            v
        };
        assert_eq!(pairs(Layout::Abnt2, 'ç'), ['.', ';', 'l', 'p']);
        assert_eq!(pairs(Layout::Azerty, 'ù'), ['m']);
        assert_eq!(pairs(Layout::Qwertz, 'ü'), ['p', 'ä', 'ö']);
        assert_eq!(pairs(Layout::Qwertz, 'ö'), ['l', 'p', 'ä', 'ü']);
        assert_eq!(pairs(Layout::Qwertz, 'ä'), ['ö', 'ü']);
        assert!(CostModel::qwerty().extra.is_empty());
    }

    #[test]
    fn every_layout_has_each_letter_once_and_every_key_a_neighbour() {
        for &layout in Layout::ALL {
            let mut count = [0u8; 26];
            for r in layout.rows() {
                for c in r.keys().chars().filter(char::is_ascii_lowercase) {
                    count[(c as u8 - b'a') as usize] += 1;
                }
            }
            assert!(count.iter().all(|&n| n == 1), "{}", layout.name());
            for r in layout.rows() {
                for k in r.keys().chars() {
                    assert!(!layout.are_neighbours(k, k));
                    let any = layout
                        .rows()
                        .iter()
                        .flat_map(|r| r.keys().chars())
                        .any(|o| layout.are_neighbours(k, o));
                    assert!(any, "{} key {k}", layout.name());
                }
            }
        }
    }
}
