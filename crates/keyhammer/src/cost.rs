// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

//! Fixed-point edit costs (unit = 16) and QWERTY adjacency.
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
/// class: `b'a'` (97) and `b'!'` (33), for instance. Outside a-z the mapping is
/// therefore lossy, but harmless: the
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

/// Costs of the four edit operations.
#[derive(Clone, Debug)]
pub struct CostModel {
    sub: Cost,
    sub_adjacent: Cost,
    indel: Cost,
    indel_double: Cost,
    transpose: Cost,
    adjacent: [u32; 26],
}

fn link(adjacent: &mut [u32; 26], a: u8, b: u8) {
    let (ia, ib) = ((a - b'a') as usize, (b - b'a') as usize);
    adjacent[ia] |= 1 << ib;
    adjacent[ib] |= 1 << ia;
}

/// Edits on the first query byte are less likely, so they cost 1.5x.
#[inline]
fn at_start(cost: Cost, qpos: usize) -> Cost {
    if qpos == 0 { cost + cost / 2 } else { cost }
}

impl CostModel {
    /// The default model with a QWERTY keyboard.
    pub fn qwerty() -> Self {
        let rows: [&[u8]; 3] = [b"qwertyuiop", b"asdfghjkl", b"zxcvbnm"];
        let mut adjacent = [0u32; 26];
        for (r, row) in rows.iter().enumerate() {
            for (c, &key) in row.iter().enumerate() {
                if let Some(&next) = row.get(c + 1) {
                    link(&mut adjacent, key, next);
                }
                if let Some(below) = rows.get(r + 1) {
                    if let Some(&k) = below.get(c) {
                        link(&mut adjacent, key, k);
                    }
                    if let Some(&k) = c.checked_sub(1).and_then(|p| below.get(p)) {
                        link(&mut adjacent, key, k);
                    }
                }
            }
        }
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
