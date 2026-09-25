// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel
//! Tests for the serialized index format (`docs/design/index-format.md`):
//! round trips, byte stability, the exhaustive mutation test and the precise
//! errors of the validator.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod fuzz_props;
mod support;

use fuzz_props::{check_accepted, crc32_reference, fix_crc};
use keyhammer::cost::{CostModel, Layout};
use keyhammer::index::{FormatError, Index, MAGIC, PROFILE, VERSION};
use keyhammer::search::{Output, Ranking, SearchConfig, Searcher};
use keyhammer::text::Normalizer;
use keyhammer::trie::Trie;
use support::Rng;

fn build(items: &[(&str, u16)], n: Option<Normalizer>) -> Trie {
    match n {
        Some(n) => Trie::build_normalized(items, &n).unwrap(),
        None => Trie::build(items).unwrap(),
    }
}

/// Reads section `id` (1 to 11) from the section table: (offset, length).
fn section(bytes: &[u8], id: usize) -> (usize, usize) {
    let e = 64 + (id - 1) * 24;
    let u64_at = |o: usize| u64::from_le_bytes(bytes[o..o + 8].try_into().unwrap()) as usize;
    (u64_at(e + 8), u64_at(e + 16))
}

/// Every accessor of the view and of the loaded trie equals the original.
fn assert_same(trie: &Trie, ix: &Index<'_>) {
    let loaded = ix.to_trie();
    assert_eq!(ix.len(), trie.len());
    assert_eq!(ix.node_count(), trie.node_count());
    assert_eq!(ix.normalizer(), trie.normalizer());
    assert_eq!(loaded.normalizer(), trie.normalizer());
    for id in 0..trie.len() as u32 {
        assert_eq!(ix.term(id), trie.term(id));
        assert_eq!(ix.weight(id), trie.weight(id));
        assert_eq!(ix.input_index(id), trie.input_index(id));
        assert_eq!(loaded.term(id), trie.term(id));
        assert_eq!(loaded.weight(id), trie.weight(id));
        assert_eq!(loaded.input_index(id), trie.input_index(id));
    }
    for v in 0..trie.node_count() {
        let want = (
            trie.label(v),
            trie.children(v),
            trie.term_id(v),
            trie.max_weight(v),
            trie.len_min(v),
            trie.len_max(v),
            trie.below_mask(v),
        );
        let view = (
            ix.label(v),
            ix.children(v),
            ix.term_id(v),
            ix.max_weight(v),
            ix.len_min(v),
            ix.len_max(v),
            ix.below_mask(v),
        );
        let owned = (
            loaded.label(v),
            loaded.children(v),
            loaded.term_id(v),
            loaded.max_weight(v),
            loaded.len_min(v),
            loaded.len_max(v),
            loaded.below_mask(v),
        );
        assert_eq!(view, want, "node {v}");
        assert_eq!(owned, want, "node {v}");
    }
    // Out of range: defaults, no panic.
    let far = trie.node_count() + 5;
    assert_eq!(ix.term(u32::MAX), "");
    assert_eq!(ix.input_index(u32::MAX), u32::MAX);
    assert_eq!(ix.children(far), 0..0);
    assert_eq!(ix.label(usize::MAX), '\0');
    assert_eq!(ix.below_mask(usize::MAX), 0);
}

fn key(o: Output) -> (Vec<keyhammer::search::Hit>, keyhammer::search::Stats) {
    (o.hits, o.stats)
}

/// Search results (hits and work counters) on the view and on the loaded trie
/// equal those on the original trie, in every mode.
fn assert_same_results(trie: &Trie, ix: &Index<'_>, queries: &[String], cm: &CostModel) {
    let loaded = ix.to_trie();
    let mut s = Searcher::new();
    for q in queries {
        for ranking in [Ranking::Coarse, Ranking::Exact] {
            for (tsb, budget) in [(true, 32), (false, 32), (true, 48), (true, 16)] {
                let cfg = SearchConfig {
                    k: 7,
                    budget,
                    tsb,
                    ranking,
                    ..SearchConfig::default()
                };
                let b = q.as_bytes();
                let want = key(s.search(trie, cm, b, &cfg).unwrap());
                assert_eq!(
                    key(ix.search(&mut s, cm, b, &cfg).unwrap()),
                    want,
                    "q={q:?}"
                );
                assert_eq!(key(s.search(&loaded, cm, b, &cfg).unwrap()), want);
                let want = key(s.search_prefix(trie, cm, b, &cfg).unwrap());
                assert_eq!(key(ix.search_prefix(&mut s, cm, b, &cfg).unwrap()), want);
                assert_eq!(key(s.search_prefix(&loaded, cm, b, &cfg).unwrap()), want);
                let want = key(s.search_text(trie, cm, q, &cfg).unwrap());
                assert_eq!(key(ix.search_text(&mut s, cm, q, &cfg).unwrap()), want);
                assert_eq!(key(s.search_text(&loaded, cm, q, &cfg).unwrap()), want);
                let want = key(s.search_prefix_text(trie, cm, q, &cfg).unwrap());
                assert_eq!(
                    key(ix.search_prefix_text(&mut s, cm, q, &cfg).unwrap()),
                    want
                );
                assert_eq!(
                    key(s.search_prefix_text(&loaded, cm, q, &cfg).unwrap()),
                    want
                );
            }
        }
    }
}

fn random_word(rng: &mut Rng, alpha: &[char]) -> String {
    (0..1 + rng.below(9))
        .map(|_| alpha[rng.below(alpha.len() as u64) as usize])
        .collect()
}

/// Up to two random edits of `w`.
fn typo(rng: &mut Rng, w: &str, alpha: &[char]) -> String {
    let mut c: Vec<char> = w.chars().collect();
    for _ in 0..rng.below(3) {
        let letter = alpha[rng.below(alpha.len() as u64) as usize];
        let n = c.len() as u64;
        match rng.below(4) {
            0 if n > 0 => c[rng.below(n) as usize] = letter,
            1 => c.insert(rng.below(n + 1) as usize, letter),
            2 if n > 1 => {
                c.remove(rng.below(n) as usize);
            }
            3 if n > 1 => c.swap(0, 1),
            _ => {}
        }
    }
    c.into_iter().collect()
}

const ALPHABETS: [&str; 4] = ["abcdefghij", "aeéèçßAÉ", "жзaЖ😀ä", "cafeCAFÉ\u{301}ﬁ "];

fn scale(n: usize) -> usize {
    if cfg!(miri) { n.div_ceil(40) } else { n }
}

#[test]
fn round_trip_on_random_dictionaries_gives_identical_tries_and_results() {
    let normalizers = [
        None,
        Some(Normalizer::new()),
        Some(Normalizer::new().with_case_folding(false)),
        Some(Normalizer::new().with_diacritic_folding(false)),
    ];
    for (a, alpha) in ALPHABETS.iter().enumerate() {
        let alpha: Vec<char> = alpha.chars().collect();
        for d in 0..scale(12) {
            let mut rng = Rng::new(a as u64 * 100 + d as u64);
            let words: Vec<(String, u16)> = (0..1 + rng.below(scale(120) as u64))
                .map(|_| (random_word(&mut rng, &alpha), rng.below(4) as u16 * 1000))
                .collect();
            let items: Vec<(&str, u16)> = words.iter().map(|(w, x)| (w.as_str(), *x)).collect();
            let cm = CostModel::for_layout(Layout::ALL[d % Layout::ALL.len()]);
            let mut queries: Vec<String> = vec![String::new()];
            for _ in 0..scale(6) {
                let base = &words[rng.below(words.len() as u64) as usize].0;
                queries.push(typo(&mut rng, base, &alpha));
            }
            let n = normalizers[d % normalizers.len()];
            let Ok(trie) = (match n {
                Some(n) => Trie::build_normalized(&items, &n),
                None => Trie::build(&items),
            }) else {
                continue; // a term made only of combining marks
            };
            let bytes = trie.to_bytes().unwrap();
            assert_eq!(bytes.len() % 8, 0);
            assert_eq!(trie.to_bytes().unwrap(), bytes, "not deterministic");
            let ix = Index::from_bytes(&bytes).unwrap();
            assert_same(&trie, &ix);
            assert_eq!(ix.to_trie().to_bytes().unwrap(), bytes);
            assert_eq!(Trie::from_bytes(&bytes).unwrap().to_bytes().unwrap(), bytes);
            assert_same_results(&trie, &ix, &queries, &cm);
        }
    }
}

#[test]
fn an_unaligned_input_is_read_the_same() {
    let trie = build(&[("héllo", 3), ("hello", 5), ("help", 2)], None);
    let bytes = trie.to_bytes().unwrap();
    for shift in 1..8 {
        let mut buf = vec![0u8; shift];
        buf.extend_from_slice(&bytes);
        let ix = Index::from_bytes(&buf[shift..]).unwrap();
        assert_same(&trie, &ix);
        assert_same_results(
            &trie,
            &ix,
            &["helo".into(), "hélp".into()],
            &CostModel::qwerty(),
        );
    }
}

#[test]
fn the_same_input_gives_the_same_bytes() {
    let items = [
        ("zeta", 1),
        ("alpha", 9),
        ("beta", 3),
        ("alpha", 2),
        ("über", 4),
    ];
    let a = build(&items, None).to_bytes().unwrap();
    let b = build(&items, None).to_bytes().unwrap();
    assert_eq!(a, b);
    // The input order changes only the input indices (section 9).
    let mut rev = items;
    rev.reverse();
    let c = build(&rev, None).to_bytes().unwrap();
    let (off, len) = section(&a, 9);
    assert_ne!(a[off..off + len], c[off..off + len]);
    let strip = |x: &[u8]| {
        let mut x = x.to_vec();
        x[off..off + len].fill(0);
        x[24..28].fill(0);
        x
    };
    assert_eq!(strip(&a), strip(&c));
}

/// Hex of `bytes`, 32 bytes per line.
fn hex(bytes: &[u8]) -> String {
    bytes
        .chunks(32)
        .map(|c| c.iter().map(|b| format!("{b:02x}")).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The exact bytes of two tiny indexes, pinned: any change to the writer that
/// changes them is a format change (a new version, see the design note).
#[test]
fn byte_stability_golden() {
    let plain = build(&[("car", 9), ("cat", 4), ("né", 1)], None);
    let got = hex(&plain.to_bytes().unwrap());
    assert_eq!(
        got,
        GOLDEN_PLAIN.trim(),
        "plain index bytes changed:\n{got}"
    );
    let norm = build(&[("Café", 5)], Some(Normalizer::new()));
    let got = hex(&norm.to_bytes().unwrap());
    assert_eq!(
        got,
        GOLDEN_NORMALIZED.trim(),
        "normalised index bytes changed:\n{got}"
    );
}

const GOLDEN_PLAIN: &str = "
4b48494e4445580001000100000000004802000000000000eb1307840b000000
0700000003000000090000000000000000000000000000000000000000000000
010000000400000048010000000000001c000000000000000200000004000000
6801000000000000200000000000000003000000040000008801000000000000
1c000000000000000400000002000000a8010000000000000e00000000000000
0500000002000000b8010000000000000e000000000000000600000002000000
c8010000000000000e000000000000000700000008000000d801000000000000
3800000000000000080000000200000010020000000000000600000000000000
090000000400000018020000000000000c000000000000000a00000004000000
280200000000000010000000000000000b000000010000003802000000000000
090000000000000000000000630000006e00000061000000e900000072000000
7400000000000000010000000300000004000000050000000700000007000000
0700000007000000ffffffffffffffffffffffffffffffff0200000000000000
0100000000000000020003000200030002000300030000000300030002000300
020003000300000009000900010009000100090004000000200000000a401400
0000000002001400200000000000000000000000000014000000000000000000
0000000000000000000000000000000009000400010000000000000001000000
0200000000000000000000000300000006000000090000006361726361746ec3
a900000000000000
";

const GOLDEN_NORMALIZED: &str = "
4b48494e4445580001000100010000000802000000000000696592790b000000
050000000100000004000000030001000f000000000000000000000000000000
0100000004000000480100000000000014000000000000000200000004000000
6001000000000000180000000000000003000000040000007801000000000000
1400000000000000040000000200000090010000000000000a00000000000000
0500000002000000a0010000000000000a000000000000000600000002000000
b0010000000000000a000000000000000700000008000000c001000000000000
28000000000000000800000002000000e8010000000000000200000000000000
0900000004000000f00100000000000004000000000000000a00000004000000
f80100000000000008000000000000000b000000010000000002000000000000
0400000000000000000000006300000061000000660000006500000000000000
010000000200000003000000040000000500000005000000ffffffffffffffff
ffffffffffffffff000000000000000004000400040004000400000000000000
0400040004000400040000000000000005000500050005000500000000000000
000000006a000000000000006200000000000000600000000000000020000000
0000000000000000050000000000000000000000000000000000000004000000
6361666500000000
";

#[test]
fn the_header_holds_what_the_design_note_says() {
    let trie = build(
        &[("Straße", 2)],
        Some(Normalizer::new().with_case_folding(false)),
    );
    let b = trie.to_bytes().unwrap();
    assert_eq!(b[..8], MAGIC);
    assert_eq!(u16::from_le_bytes([b[8], b[9]]), VERSION);
    assert_eq!(u16::from_le_bytes([b[10], b[11]]), PROFILE);
    assert_eq!(b[12..16], [1, 0, 0, 0]); // a normaliser is recorded
    assert_eq!(
        u64::from_le_bytes(b[16..24].try_into().unwrap()),
        b.len() as u64
    );
    let mut z = b.clone();
    z[24..28].fill(0);
    assert_eq!(b[24..28], crc32_reference(&z).to_le_bytes());
    assert_eq!(b[28..32], [11, 0, 0, 0]);
    assert_eq!(b[44], 2); // diacritics only
    assert_eq!(b[46..48], [1, 0]);
    assert_eq!(b[48..51], [15, 0, 0]);
    let mut last = 0;
    for id in 1..=11 {
        let (off, _) = section(&b, id);
        assert_eq!(off % 8, 0);
        assert!(off > last);
        last = off;
    }
}

/// Payload fields (design note, section 5.4): a weight, an input index, the
/// normaliser mode byte.
fn is_payload(bytes: &[u8], pos: usize) -> bool {
    let (w, wl) = section(bytes, 8);
    let (i, il) = section(bytes, 9);
    (w..w + wl).contains(&pos) || (i..i + il).contains(&pos) || pos == 44
}

fn queries_of(trie: &Trie) -> Vec<String> {
    let mut q: Vec<String> = vec![String::new(), "x".into()];
    for id in 0..trie.len() as u32 {
        let t = trie.term(id);
        q.push(t.to_string());
        let mut c: Vec<char> = t.chars().collect();
        c.pop();
        q.push(c.iter().collect());
    }
    q
}

/// Flips every byte of `bytes` with each single-bit mask and 0xFF, and cuts
/// it at every length. Without a CRC fix every change must be rejected; with
/// the CRC recomputed it must be rejected or give a consistent, canonical
/// index whose flipped byte was in a payload field.
fn every_byte_flip(bytes: &[u8]) -> (usize, usize) {
    assert!(Index::from_bytes(bytes).is_ok());
    let masks: &[u8] = if cfg!(miri) {
        &[0x01, 0xFF]
    } else {
        &[0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0xFF]
    };
    let step = if cfg!(miri) { 29 } else { 1 };
    let trie = Trie::from_bytes(bytes).unwrap();
    let queries = queries_of(&trie);
    let queries: Vec<&str> = queries.iter().map(String::as_str).collect();
    let cm = CostModel::qwerty();
    let (mut accepted, mut rejected) = (0, 0);
    for pos in (0..bytes.len()).step_by(step) {
        for &mask in masks {
            let mut m = bytes.to_vec();
            m[pos] ^= mask;
            let e = Index::from_bytes(&m).expect_err("a flipped byte must be rejected");
            assert!(!e.to_string().is_empty());
            if (24..28).contains(&pos) {
                continue; // fixing the CRC field would restore the original
            }
            fix_crc(&mut m);
            match Index::from_bytes(&m) {
                Ok(_) => {
                    assert!(
                        is_payload(bytes, pos),
                        "accepted a flip at {pos} (mask {mask:#x})"
                    );
                    check_accepted(&m, &queries, &cm, 5, 32);
                    accepted += 1;
                }
                Err(_) => rejected += 1,
            }
        }
    }
    for len in (0..bytes.len()).step_by(step) {
        let cut = &bytes[..len];
        assert!(Index::from_bytes(cut).is_err(), "accepted a cut at {len}");
        // Even with its length and CRC made consistent, a cut file is refused.
        if len >= 28 {
            let mut c = cut.to_vec();
            if len >= 24 {
                c[16..24].copy_from_slice(&(len as u64).to_le_bytes());
            }
            fix_crc(&mut c);
            assert!(
                Index::from_bytes(&c).is_err(),
                "accepted a fixed-up cut at {len}"
            );
        }
    }
    let mut longer = bytes.to_vec();
    longer.extend_from_slice(&[0; 8]);
    assert!(Index::from_bytes(&longer).is_err());
    let n = longer.len() as u64;
    longer[16..24].copy_from_slice(&n.to_le_bytes());
    fix_crc(&mut longer);
    assert_eq!(
        Index::from_bytes(&longer).unwrap_err(),
        FormatError::BadCounts
    );
    (accepted, rejected)
}

#[test]
fn every_byte_flip_and_cut_of_an_ascii_index_is_rejected_or_harmless() {
    let trie = build(
        &[("car", 9), ("cat", 4), ("cart", 7), ("dog", 2), ("do", 3)],
        None,
    );
    let (accepted, rejected) = every_byte_flip(&trie.to_bytes().unwrap());
    eprintln!("ascii: {accepted} consistent flips accepted, {rejected} rejected");
    assert!(accepted > 0 && rejected > 0);
}

#[test]
fn every_byte_flip_and_cut_of_a_normalised_index_is_rejected_or_harmless() {
    let items = [
        ("Café", 5),
        ("cafe", 9),
        ("Straße", 3),
        ("жук", 1),
        ("😀x", 2),
    ];
    let trie = build(&items, Some(Normalizer::new()));
    let (accepted, rejected) = every_byte_flip(&trie.to_bytes().unwrap());
    eprintln!("normalised: {accepted} consistent flips accepted, {rejected} rejected");
    assert!(accepted > 0 && rejected > 0);
}

/// Applies `f` to a valid index, recomputes the CRC and returns the error.
fn corrupt(trie: &Trie, f: impl FnOnce(&mut Vec<u8>)) -> FormatError {
    let mut b = trie.to_bytes().unwrap();
    f(&mut b);
    fix_crc(&mut b);
    Index::from_bytes(&b).unwrap_err()
}

fn put_u16(b: &mut [u8], at: usize, v: u16) {
    b[at..at + 2].copy_from_slice(&v.to_le_bytes());
}
fn put_u32(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

#[test]
fn each_check_reports_its_own_error() {
    use FormatError as E;
    let trie = build(&[("car", 9), ("cat", 4), ("né", 1)], None);
    let norm = build(&[("café", 9), ("the", 4)], Some(Normalizer::new()));
    // root, c, n, ca, é, car, cat
    assert_eq!(trie.node_count(), 7);
    let good = trie.to_bytes().unwrap();

    assert_eq!(
        Index::from_bytes(&good[..63]).unwrap_err(),
        E::TooShort { len: 63 }
    );
    assert_eq!(corrupt(&trie, |b| b[0] = b'X'), E::BadMagic);
    assert_eq!(
        corrupt(&trie, |b| put_u16(b, 8, 2)),
        E::UnsupportedVersion { found: 2 }
    );
    assert_eq!(
        corrupt(&trie, |b| put_u16(b, 8, 0)),
        E::UnsupportedVersion { found: 0 }
    );
    assert_eq!(
        corrupt(&trie, |b| put_u16(b, 10, 2)),
        E::UnknownProfile { found: 2 }
    );
    assert_eq!(corrupt(&trie, |b| b[12] = 3), E::UnknownFlags { flags: 3 });
    assert!(matches!(
        corrupt(&trie, |b| b[16] ^= 8),
        E::LengthMismatch { .. }
    ));
    let mut bad = good.clone();
    bad[200] ^= 1;
    assert!(matches!(
        Index::from_bytes(&bad).unwrap_err(),
        E::ChecksumMismatch { .. }
    ));
    assert_eq!(
        corrupt(&trie, |b| b[60] = 1),
        E::NonZeroReserved { offset: 60 }
    );
    assert_eq!(
        corrupt(&trie, |b| b[46] = 1),
        E::NonZeroReserved { offset: 46 }
    );
    assert_eq!(corrupt(&norm, |b| b[44] = 4), E::BadNormalizer { modes: 4 });
    assert_eq!(
        corrupt(&norm, |b| b[46] = 2),
        E::NormalizerMismatch {
            algorithm: 2,
            unicode: [15, 0, 0]
        }
    );
    assert_eq!(
        corrupt(&norm, |b| b[48] = 16),
        E::NormalizerMismatch {
            algorithm: 1,
            unicode: [16, 0, 0]
        }
    );
    assert_eq!(
        corrupt(&trie, |b| b[28] = 12),
        E::SectionCount { found: 12 }
    );
    assert_eq!(corrupt(&trie, |b| put_u32(b, 36, 0)), E::BadCounts);
    assert_eq!(corrupt(&trie, |b| put_u32(b, 36, 7)), E::BadCounts);
    assert_eq!(
        corrupt(&trie, |b| b[64 + 4] = 2),
        E::BadSectionTable { section: 1 }
    );
    assert_eq!(
        corrupt(&trie, |b| b[64 + 24 * 3 + 8] ^= 8),
        E::BadSectionTable { section: 4 }
    );
    // Padding after the pool ("carcatné" is 9 bytes, then 7 zero bytes).
    let (pool, plen) = section(&good, 11);
    assert_eq!(
        corrupt(&trie, |b| b[pool + plen] = 1),
        E::NonZeroReserved {
            offset: pool + plen
        }
    );

    let (labels, _) = section(&good, 1);
    let (children, _) = section(&good, 2);
    let (tids, _) = section(&good, 3);
    let (lmin, _) = section(&good, 4);
    let (lmax, _) = section(&good, 5);
    let (maxw, _) = section(&good, 6);
    let (mask, _) = section(&good, 7);
    let (weights, _) = section(&good, 8);
    let (input, _) = section(&good, 9);
    let (toff, _) = section(&good, 10);

    // A child range before its parent (a cycle), and a non-contiguous one.
    assert_eq!(
        corrupt(&trie, |b| put_u32(b, children + 4 * 3, 3)),
        E::BadChildren { node: 2 }
    );
    assert_eq!(
        corrupt(&trie, |b| put_u32(b, children, 0)),
        E::BadChildren { node: 0 }
    );
    assert_eq!(
        corrupt(&trie, |b| put_u32(b, labels, 7)),
        E::BadLabel { node: 0 }
    );
    // A surrogate, and two siblings out of order ('c' and 'n' swapped).
    assert_eq!(
        corrupt(&trie, |b| put_u32(b, labels + 4 * 4, 0xD800)),
        E::BadLabel { node: 4 }
    );
    assert_eq!(
        corrupt(&trie, |b| put_u32(b, labels + 4 * 2, u32::from('a'))),
        E::BadLabel { node: 2 }
    );
    assert_eq!(
        corrupt(&trie, |b| put_u32(b, tids, 0)),
        E::BadTermId { node: 0 }
    );
    assert_eq!(
        corrupt(&trie, |b| put_u32(b, tids + 4 * 5, 3)),
        E::BadTermId { node: 5 }
    );
    assert_eq!(
        corrupt(&trie, |b| put_u32(b, tids + 4 * 5, u32::MAX)),
        E::BadTermId { node: 5 }
    );
    // Bounds too loose or too tight are both refused: a len_min too high (or
    // a len_max too low) would make the subtree bound drop results.
    assert_eq!(
        corrupt(&trie, |b| put_u16(b, lmin, 3)),
        E::BadLenBounds { node: 0 }
    );
    assert_eq!(
        corrupt(&trie, |b| put_u16(b, lmax, 2)),
        E::BadLenBounds { node: 0 }
    );
    assert_eq!(
        corrupt(&trie, |b| put_u16(b, lmax, 9)),
        E::BadLenBounds { node: 0 }
    );
    // Node 6 ("cat") claims a longer term: its parent still agrees (min and
    // max over its children are unchanged), the node itself does not.
    assert_eq!(
        corrupt(&trie, |b| put_u16(b, lmin + 2 * 6, 4)),
        E::BadLenBounds { node: 6 }
    );
    assert_eq!(
        corrupt(&trie, |b| b[mask] ^= 1),
        E::BadBelowMask { node: 0 }
    );
    assert_eq!(
        corrupt(&trie, |b| put_u16(b, maxw, 8)),
        E::BadMaxWeight { node: 0 }
    );
    // The heaviest term's weight: its path's maxima no longer match.
    assert_eq!(
        corrupt(&trie, |b| put_u16(b, weights, 1)),
        E::BadMaxWeight { node: 5 }
    );
    assert_eq!(
        corrupt(&trie, |b| put_u32(b, toff + 4, 0)),
        E::BadTermOffsets { term: 0 }
    );
    assert_eq!(
        corrupt(&trie, |b| put_u32(b, toff, 1)),
        E::BadTermOffsets { term: 0 }
    );
    // "né" is bytes 6..9 of the pool: break the UTF-8 of "é".
    assert_eq!(
        corrupt(&trie, |b| b[pool + 8] = b'x'),
        E::InvalidUtf8 { term: 2 }
    );
    // "car" -> "cas": sorted still, but not in the trie.
    assert_eq!(
        corrupt(&trie, |b| b[pool + 2] = b's'),
        E::TermNotInTrie { term: 0 }
    );
    // "cat" -> "caa": now before "car".
    assert_eq!(
        corrupt(&trie, |b| b[pool + 5] = b'a'),
        E::TermsNotSorted { term: 1 }
    );
    // "the" -> "tHe" in the pool and in the trie, with the masks updated:
    // consistent, but not in the normaliser's normal form.
    let e = corrupt(&norm, |b| {
        let (nl, _) = section(b, 1);
        let (nm, _) = section(b, 7);
        let (np, _) = section(b, 11);
        let t = norm.children(0).find(|&v| norm.label(v) == 't').unwrap();
        let h = norm.children(t).start;
        assert_eq!(norm.label(h), 'h');
        put_u32(b, nl + 4 * h, u32::from('H'));
        assert_eq!(b[np + 5], b'h');
        b[np + 5] = b'H';
        let class = |c: char| keyhammer::cost::symbol_class(c.into());
        for v in [0, t] {
            let at = nm + 8 * v;
            let m = u64::from_le_bytes(b[at..at + 8].try_into().unwrap());
            let m = (m & !class('h')) | class('H');
            b[at..at + 8].copy_from_slice(&m.to_le_bytes());
        }
    });
    assert_eq!(e, E::TermNotNormalized { term: 1 });
    assert_eq!(
        corrupt(&trie, |b| put_u32(b, input + 4, u32::MAX)),
        E::BadInputIndex { term: 1 }
    );
    // Every variant has a message.
    for e in [E::BadCounts, E::TooLarge, E::BadMagic] {
        assert!(!e.to_string().is_empty());
    }
}

#[test]
fn payload_changes_are_accepted_and_consistent() {
    let trie = build(&[("car", 1), ("cat", 4), ("cart", 7)], None);
    let mut b = trie.to_bytes().unwrap();
    // "car" (id 0) weighs 1, below "cart" under it: raising it to 5 leaves
    // every maximum unchanged, so it is a consistent index of other content.
    // (The weight of a leaf is its node's maximum, so changing it is refused.)
    let (weights, _) = section(&b, 8);
    assert_eq!(trie.term(0), "car");
    put_u16(&mut b, weights, 5);
    fix_crc(&mut b);
    let ix = Index::from_bytes(&b).unwrap();
    assert_eq!(ix.weight(0), 5);
    check_accepted(&b, &["cat", "car", "ca", ""], &CostModel::qwerty(), 5, 32);
}

/// The arrays of an index, for [`assemble`].
struct Raw {
    labels: Vec<u32>,
    children: Vec<u32>,
    tids: Vec<u32>,
    lmin: Vec<u16>,
    lmax: Vec<u16>,
    maxw: Vec<u16>,
    mask: Vec<u64>,
    weights: Vec<u16>,
    input: Vec<u32>,
    terms: Vec<String>,
    /// Mode byte of the normaliser, if any.
    norm: Option<u8>,
}

fn raw_of(t: &Trie) -> Raw {
    let n = t.node_count();
    let ids = 0..t.len() as u32;
    let mut children: Vec<u32> = (0..n).map(|v| t.children(v).start as u32).collect();
    children.push(n as u32);
    Raw {
        labels: (0..n).map(|v| u32::from(t.label(v))).collect(),
        children,
        tids: (0..n).map(|v| t.term_id(v)).collect(),
        lmin: (0..n).map(|v| t.len_min(v)).collect(),
        lmax: (0..n).map(|v| t.len_max(v)).collect(),
        maxw: (0..n).map(|v| t.max_weight(v)).collect(),
        mask: (0..n).map(|v| t.below_mask(v)).collect(),
        weights: ids.clone().map(|id| t.weight(id)).collect(),
        input: ids.clone().map(|id| t.input_index(id)).collect(),
        terms: ids.map(|id| t.term(id).to_string()).collect(),
        norm: t
            .normalizer()
            .map(|n| u8::from(n.folds_case()) | (u8::from(n.folds_diacritics()) << 1)),
    }
}

/// An independent writer, straight from the tables of the design note.
fn assemble(r: &Raw) -> Vec<u8> {
    let pad = |b: &mut Vec<u8>| {
        while b.len() % 8 != 0 {
            b.push(0);
        }
    };
    let mut pool = Vec::new();
    let mut offsets = vec![0u32];
    for t in &r.terms {
        pool.extend_from_slice(t.as_bytes());
        offsets.push(pool.len() as u32);
    }
    let le16 = |v: &[u16]| v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
    let le32 = |v: &[u32]| v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
    let le64 = |v: &[u64]| v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
    let sections: Vec<(u32, Vec<u8>)> = vec![
        (4, le32(&r.labels)),
        (4, le32(&r.children)),
        (4, le32(&r.tids)),
        (2, le16(&r.lmin)),
        (2, le16(&r.lmax)),
        (2, le16(&r.maxw)),
        (8, le64(&r.mask)),
        (2, le16(&r.weights)),
        (4, le32(&r.input)),
        (4, le32(&offsets)),
        (1, pool.clone()),
    ];
    let mut body = Vec::new();
    let mut table = Vec::new();
    let start = 64 + 24 * sections.len();
    for (i, (elem, data)) in sections.iter().enumerate() {
        let off = start + body.len();
        table.extend_from_slice(&(i as u32 + 1).to_le_bytes());
        table.extend_from_slice(&elem.to_le_bytes());
        table.extend_from_slice(&(off as u64).to_le_bytes());
        table.extend_from_slice(&(data.len() as u64).to_le_bytes());
        body.extend_from_slice(data);
        pad(&mut body);
    }
    let total = start + body.len();
    let mut h = Vec::new();
    h.extend_from_slice(b"KHINDEX\0");
    h.extend_from_slice(&1u16.to_le_bytes());
    h.extend_from_slice(&1u16.to_le_bytes());
    h.extend_from_slice(&u32::from(r.norm.is_some()).to_le_bytes());
    h.extend_from_slice(&(total as u64).to_le_bytes());
    h.extend_from_slice(&[0; 4]);
    h.extend_from_slice(&11u32.to_le_bytes());
    h.extend_from_slice(&(r.labels.len() as u32).to_le_bytes());
    h.extend_from_slice(&(r.terms.len() as u32).to_le_bytes());
    h.extend_from_slice(&(pool.len() as u32).to_le_bytes());
    match r.norm {
        Some(m) => h.extend_from_slice(&[m, 0, 1, 0, 15, 0, 0, 0]),
        None => h.extend_from_slice(&[0; 8]),
    }
    h.extend_from_slice(&[0; 12]);
    assert_eq!(h.len(), 64);
    h.extend(table);
    h.extend(body);
    fix_crc(&mut h);
    h
}

#[test]
fn an_independent_writer_following_the_design_note_gives_the_same_bytes() {
    for (items, n) in [
        (vec![("car", 9), ("cat", 4), ("né", 1)], None),
        (
            vec![("Café", 5), ("naïve", 2), ("😀", 1)],
            Some(Normalizer::new()),
        ),
        (
            vec![("a", 1)],
            Some(Normalizer::new().with_case_folding(false)),
        ),
    ] {
        let trie = build(&items, n);
        assert_eq!(assemble(&raw_of(&trie)), trie.to_bytes().unwrap());
    }
}

#[test]
fn a_terminal_node_without_a_term_of_its_own_is_counted() {
    // Three leaves, two terms: the leaf "ad" reuses the id of "ac". Every
    // other check passes (equal weights), only the count differs.
    let trie = build(&[("ab", 1), ("ac", 1), ("ad", 1)], None);
    let mut r = raw_of(&trie);
    let ad = trie.term_id(trie.children(trie.children(0).start).end - 1);
    assert_eq!(ad, 2);
    for t in &mut r.tids {
        if *t == 2 {
            *t = 1;
        }
    }
    r.terms.pop();
    r.weights.pop();
    r.input.pop();
    assert_eq!(
        Index::from_bytes(&assemble(&r)).unwrap_err(),
        FormatError::TerminalCount {
            found: 3,
            expected: 2
        }
    );
}

/// The normaliser record is payload (design note, section 5.4): a changed
/// mode byte is accepted and changes what `search_text` finds, and removing
/// the record is accepted too. Callers compare `normalizer()` themselves.
#[test]
fn the_normaliser_is_the_files_and_callers_must_check_it() {
    let trie = build(&[("cafe", 1)], Some(Normalizer::new()));
    let good = trie.to_bytes().unwrap();
    let cm = CostModel::qwerty();
    let cfg = SearchConfig {
        budget: 0,
        ..SearchConfig::default()
    };
    let hits = |b: &[u8]| {
        let ix = Index::from_bytes(b).unwrap();
        ix.search_text(&mut Searcher::new(), &cm, "café", &cfg)
            .unwrap()
            .hits
            .len()
    };
    assert_eq!(hits(&good), 1);
    // Mode byte 3 (case and diacritics) -> 1 (case only).
    let mut case_only = good.clone();
    case_only[44] = 1;
    fix_crc(&mut case_only);
    let ix = Index::from_bytes(&case_only).unwrap();
    assert_ne!(ix.normalizer(), Some(Normalizer::new()));
    assert_eq!(hits(&case_only), 0);
    // No normaliser at all.
    let mut plain = good.clone();
    plain[12] = 0;
    plain[44..51].fill(0);
    fix_crc(&mut plain);
    assert_eq!(Index::from_bytes(&plain).unwrap().normalizer(), None);
}
