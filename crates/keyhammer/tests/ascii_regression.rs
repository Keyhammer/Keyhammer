// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel
//! Pins the ASCII behaviour of the engine: trie shape, hits, costs and work
//! counters for fixed dictionaries and queries, as digests recorded before the
//! alphabet moved from bytes to code points (issue #19). Any change to an
//! ASCII result, to a node count or to the trie layout changes a digest.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use keyhammer::cost::{CostModel, Layout};
use keyhammer::search::{Output, Ranking, SearchConfig, Searcher};
use keyhammer::trie::Trie;
use support::Rng;

/// FNV-1a, 64 bits.
struct Digest(u64);

impl Digest {
    fn new() -> Self {
        Digest(0xcbf2_9ce4_8422_2325)
    }
    fn u64(&mut self, v: u64) {
        for b in v.to_le_bytes() {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}

fn word(rng: &mut Rng, alpha: &[u8]) -> String {
    let len = 1 + rng.below(9) as usize;
    (0..len)
        .map(|_| char::from(alpha[rng.below(alpha.len() as u64) as usize]))
        .collect()
}

fn dictionary(seed: u64, alpha: &[u8], n: usize) -> Vec<(String, u16)> {
    let mut rng = Rng::new(seed);
    (0..n)
        .map(|_| (word(&mut rng, alpha), rng.below(65_536) as u16))
        .collect()
}

fn trie_digest(trie: &Trie) -> u64 {
    let mut d = Digest::new();
    d.u64(trie.node_count() as u64);
    for v in 0..trie.node_count() {
        d.u64(u64::from(u32::from(trie.label(v))));
        d.u64(trie.children(v).start as u64);
        d.u64(trie.children(v).end as u64);
        d.u64(u64::from(trie.term_id(v)));
        d.u64(u64::from(trie.max_weight(v)));
        d.u64(u64::from(trie.len_min(v)));
        d.u64(u64::from(trie.len_max(v)));
        d.u64(trie.below_mask(v));
    }
    for id in 0..trie.len() as u32 {
        d.u64(u64::from(trie.input_index(id)));
    }
    d.0
}

fn out_digest(d: &mut Digest, out: &Output) {
    d.u64(out.hits.len() as u64);
    for h in &out.hits {
        d.u64(u64::from(h.id));
        d.u64(u64::from(h.cost));
        d.u64(u64::from(h.weight));
    }
    d.u64(out.stats.nodes_expanded as u64);
    d.u64(out.stats.nodes_pushed as u64);
    d.u64(out.stats.rows_computed as u64);
    d.u64(u64::from(out.stats.truncated));
}

/// Digest of the trie and of every search over `queries` in both modes, with
/// several configurations.
fn run(items: &[(String, u16)], cm: &CostModel, queries: &[Vec<u8>]) -> (u64, u64, usize) {
    let refs: Vec<(&str, u16)> = items.iter().map(|(s, w)| (s.as_str(), *w)).collect();
    let trie = Trie::build(&refs).unwrap();
    let mut s = Searcher::new();
    let mut d = Digest::new();
    let mut expanded = 0;
    for q in queries {
        for (k, budget, tsb, ranking, max_nodes) in [
            (10, 32, true, Ranking::Coarse, 100_000),
            (10, 32, false, Ranking::Coarse, 100_000),
            (5, 48, true, Ranking::Exact, 100_000),
            (20, 64, true, Ranking::Coarse, 100_000),
            (3, 16, false, Ranking::Exact, 100_000),
            (10, 32, true, Ranking::Coarse, 25),
        ] {
            let cfg = SearchConfig {
                k,
                budget,
                tsb,
                ranking,
                max_nodes,
            };
            let out = s.search(&trie, cm, q, &cfg).unwrap();
            expanded += out.stats.nodes_expanded;
            out_digest(&mut d, &out);
            let out = s.search_prefix(&trie, cm, q, &cfg).unwrap();
            expanded += out.stats.nodes_expanded;
            out_digest(&mut d, &out);
        }
    }
    (trie_digest(&trie), d.0, expanded)
}

fn queries(seed: u64, alpha: &[u8], n: usize) -> Vec<Vec<u8>> {
    let mut rng = Rng::new(seed);
    (0..n).map(|_| word(&mut rng, alpha).into_bytes()).collect()
}

#[test]
fn ascii_results_and_work_are_unchanged() {
    const LETTERS: &[u8] = b"abcdefghijklmnopqrstuvwxyz";
    // Every printable ASCII character plus tab and NUL: uppercase, digits and
    // punctuation are compared verbatim and must stay so.
    let printable: Vec<u8> = (0x20..0x7F).chain([b'\t', 0]).collect();
    let mut got = Vec::new();
    for (i, &layout) in Layout::ALL.iter().enumerate() {
        let cm = CostModel::for_layout(layout);
        let dict = dictionary(10 + i as u64, LETTERS, 800);
        let qs = queries(20 + i as u64, LETTERS, 40);
        got.push((layout.name(), run(&dict, &cm, &qs)));
    }
    let cm = CostModel::qwerty();
    let dict = dictionary(30, &printable, 800);
    let qs = queries(31, &printable, 40);
    got.push(("qwerty-printable", run(&dict, &cm, &qs)));
    // (trie digest, search digest, total nodes expanded), recorded on the byte
    // engine before issue #19.
    let want: &[(&str, (u64, u64, usize))] = &[
        (
            "qwerty",
            (0x4a63_27b1_3db1_bc5a, 0x9bd9_c576_c4f1_41a1, 120_838),
        ),
        (
            "qwertz",
            (0x4c6c_331d_09c5_4e92, 0x9728_82d7_cb72_6b63, 156_941),
        ),
        (
            "azerty",
            (0xe360_7799_9ee7_7a07, 0xbce8_490d_aed6_754f, 150_723),
        ),
        (
            "abnt2",
            (0xc7c6_d3fa_a24b_ec5b, 0x19fb_dbdf_ac9b_5157, 151_965),
        ),
        (
            "dvorak",
            (0x0c90_0336_3838_b267, 0x98c8_7a2c_741c_2b6a, 131_181),
        ),
        (
            "colemak",
            (0x3a45_df14_777b_fae9, 0x6874_a32f_a669_5c32, 154_208),
        ),
        (
            "qwerty-printable",
            (0x78f0_75da_980e_e104, 0x42e4_3fde_a38b_efbf, 165_698),
        ),
    ];
    assert_eq!(got, want);
}
