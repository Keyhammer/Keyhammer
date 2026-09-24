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

/// Bit that represents the character class of `b` (used by subtree signatures).
///
/// Only the low six bits of `b` count, so bytes that agree modulo 64 share a
/// class: `b'a'` (97) and `b'!'` (33), for instance. Only the letters a-z are guaranteed distinct from one another;
/// other bytes can share a class with them (digits with p-y, for instance). The
/// collisions are harmless: the
/// signatures only bound the search, and a collision can make a bound weaker
/// (a class looks present when it is not), never wrong.
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
    1u64 << (b & 63)
}

/// One row of a keyboard [`Layout`]: the keys left to right and where the
/// first one starts.
///
/// Positions are in half key widths, so key `i` is at `offset + 2 * i`. Only
/// letters `a` to `z` take part in the cost model; other keys (`ç`, `;`, ...)
/// are kept so that the geometry stays honest.
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
const QWERTZ: [Row; 3] = [row(0, "qwertzuiop"), row(1, "asdfghjkl"), row(2, "yxcvbnm")];
const AZERTY: [Row; 3] = [row(0, "azertyuiop"), row(1, "qsdfghjklm"), row(2, "wxcvbn")];
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

/// Neighbour bit sets (a-z only) of one layout, computed at compile time by
/// the rule described on [`Layout`].
const fn table(rows: &[Row; 3]) -> [u32; 26] {
    let mut adj = [0u32; 26];
    let mut r = 0;
    while r < 3 {
        let a_row = rows[r].keys.as_bytes();
        let mut i = 0; // key index (a character, not a byte)
        let mut ba = 0; // byte index of key i
        while ba < a_row.len() {
            let xa = rows[r].offset as usize + 2 * i;
            let ka = a_row[ba];
            // Next key in the same row.
            let bn = next_key(a_row, ba);
            if bn < a_row.len() {
                link(&mut adj, ka, a_row[bn]);
            }
            if r + 1 < 3 {
                let b_row = rows[r + 1].keys.as_bytes();
                let mut j = 0;
                let mut bb = 0;
                while bb < b_row.len() {
                    let xb = rows[r + 1].offset as usize + 2 * j;
                    if xa.abs_diff(xb) <= 1 {
                        link(&mut adj, ka, b_row[bb]);
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
    adj
}

/// Byte index of the key after the one starting at `b` (keys are UTF-8).
const fn next_key(bytes: &[u8], b: usize) -> usize {
    let mut n = b + 1;
    while n < bytes.len() && bytes[n] & 0xC0 == 0x80 {
        n += 1;
    }
    n
}

const fn link(adj: &mut [u32; 26], a: u8, b: u8) {
    // Any lead byte outside a-z (`ç`, punctuation) is dropped.
    if a.is_ascii_lowercase() && b.is_ascii_lowercase() {
        let (ia, ib) = ((a - b'a') as usize, (b - b'a') as usize);
        adj[ia] |= 1 << ib;
        adj[ib] |= 1 << ia;
    }
}

const QWERTY_ADJ: [u32; 26] = table(&QWERTY);
const QWERTZ_ADJ: [u32; 26] = table(&QWERTZ);
const AZERTY_ADJ: [u32; 26] = table(&AZERTY);
const ABNT2_ADJ: [u32; 26] = table(&ABNT2);
const DVORAK_ADJ: [u32; 26] = table(&DVORAK);
const COLEMAK_ADJ: [u32; 26] = table(&COLEMAK);

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
/// The search alphabet is the bytes `a` to `z` (lowercase; other characters
/// are issue #19). The cost model keeps only neighbour pairs where **both**
/// keys are letters `a` to `z`. A key outside that set (`ç` on ABNT2, the
/// punctuation keys on Dvorak and Colemak) takes part in the geometry
/// ([`Layout::are_neighbours`] reports it) but every pair that involves it is
/// dropped from the cost model, so a letter next to such a key loses that
/// neighbour until #19 lands. Not modelled at all: the number row, the extra
/// ISO key beside the left shift, punctuation columns after the letter rows,
/// modifiers, dead keys and AltGr layers. The unit costs are the same for every
/// layout and remain provisional.
///
/// In particular the a-z part of ABNT2 is identical to QWERTY: the only
/// difference is `ç` (next to `l` and `p`, and to `.` and `;` below it), which
/// the a-z alphabet cannot use yet.
///
/// # Examples
///
/// ```
/// use keyhammer::cost::Layout;
///
/// assert!(Layout::Qwerty.are_neighbours('a', 's'));
/// assert!(!Layout::Qwerty.are_neighbours('a', 'd'));
/// // ç sits next to l on ABNT2, but ç is outside a-z (issue #19).
/// assert!(Layout::Abnt2.are_neighbours('l', 'ç'));
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
    /// QWERTZ (German): `y` and `z` swapped relative to QWERTY.
    Qwertz,
    /// AZERTY (French): `a`/`q` and `z`/`w` swapped, `m` on the home row.
    Azerty,
    /// Brazilian ABNT2. Its cost model is currently identical to
    /// [`Layout::Qwerty`]: `ç` is outside the a-z alphabet until #19.
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

    /// Whether keys `a` and `b` are neighbours on this layout, counting every
    /// modelled key (not only a-z). `false` for a key that is not on the
    /// modelled rows and for `a == b`.
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
    adjacent: [u32; 26],
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
    /// `layout`. Only pairs of letters `a` to `z` count; see [`Layout`] for
    /// what that leaves out. Does not panic.
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
    /// // ABNT2 has the QWERTY letters (ç is outside a-z).
    /// assert_eq!(CostModel::for_layout(Layout::Abnt2), qwerty);
    /// ```
    pub fn for_layout(layout: Layout) -> Self {
        let adjacent = match layout {
            Layout::Qwerty => QWERTY_ADJ,
            Layout::Qwertz => QWERTZ_ADJ,
            Layout::Azerty => AZERTY_ADJ,
            Layout::Abnt2 => ABNT2_ADJ,
            Layout::Dvorak => DVORAK_ADJ,
            Layout::Colemak => COLEMAK_ADJ,
        };
        Self {
            sub: 16,
            sub_adjacent: 8,
            indel: 16,
            indel_double: 8,
            transpose: 12,
            adjacent,
        }
    }

    #[inline]
    fn is_adjacent(&self, a: u8, b: u8) -> bool {
        let (ia, ib) = (a.wrapping_sub(b'a'), b.wrapping_sub(b'a'));
        ia < 26 && ib < 26 && self.adjacent[ia as usize] & (1u32 << ib) != 0
    }

    /// Cost of typing `q` where the term has `t`; `qpos` is the index of `q` in the query.
    #[inline]
    pub fn sub_cost(&self, q: u8, t: u8, qpos: usize) -> Cost {
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

    /// Cost of deleting the query byte at `qpos` (the user typed an extra byte).
    #[inline]
    pub fn del_cost(&self, q: &[u8], qpos: usize) -> Cost {
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

    /// Cost of inserting term byte `t` (the user skipped it) when `qpos` query bytes are consumed.
    #[inline]
    pub fn ins_cost(&self, t: u8, prev_t: Option<u8>, qpos: usize) -> Cost {
        let base = if prev_t == Some(t) {
            self.indel_double
        } else {
            self.indel
        };
        at_start(base, qpos)
    }

    /// Cost of swapping two adjacent bytes, the first at `qpos` in the query.
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
            assert_eq!(
                CostModel::for_layout(layout).adjacent,
                runtime_table(layout),
                "{}",
                layout.name()
            );
        }
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
