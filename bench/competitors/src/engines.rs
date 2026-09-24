// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

//! The engines under test, behind one small trait. Every engine returns its
//! top [`TOP`] terms for a lower-cased query within two edits.

use std::borrow::Borrow;
use std::cmp::Reverse;
use std::rc::Rc;

use fst::automaton::Levenshtein;
use fst::{IntoStreamer, Map, MapBuilder, Streamer};
use keyhammer::cost::CostModel;
use keyhammer::search::{Ranking, SearchConfig, Searcher};
use keyhammer::trie::Trie;
use symspell::{AsciiStringStrategy, SymSpell, SymSpellBuilder, Verbosity};

/// Number of results every engine returns.
pub const TOP: usize = 10;
/// Edit radius used for candidate generation by every engine.
pub const RADIUS: usize = 2;

/// Names accepted by `--engines`, in report order.
pub const ALL: [&str; 6] = [
    "keyhammer",
    "symspell",
    "symspell/rerank",
    "fst",
    "strsim",
    "bk-tree",
];

pub trait Engine {
    /// Runs one query and keeps its ranked result inside the engine. This is
    /// the only call that is timed.
    fn run(&mut self, query: &str);
    /// The ranked terms of the last query (at most [`TOP`]).
    fn top(&self) -> Vec<&str>;
    /// Queries the engine could not answer (for example a size limit hit).
    fn failures(&self) -> usize {
        0
    }
}

/// Ranking key shared by the competitors that do not rank by themselves:
/// (distance, higher weight, term).
fn rank_and_cut<T: Ord>(v: &mut Vec<(usize, Reverse<u64>, T)>) {
    v.sort_unstable();
    v.truncate(TOP);
}

// --- keyhammer --------------------------------------------------------------

pub struct Keyhammer {
    trie: Trie,
    costs: CostModel,
    searcher: Searcher,
    cfg: SearchConfig,
    hits: Vec<u32>,
    truncated: usize,
}

impl Keyhammer {
    pub fn build(words: &[(String, u16)]) -> Self {
        let items: Vec<(&str, u16)> = words.iter().map(|(w, f)| (w.as_str(), *f)).collect();
        let trie = Trie::build(&items).expect("keyhammer: trie build failed");
        drop(items);
        Self {
            trie,
            costs: CostModel::qwerty(),
            searcher: Searcher::new(),
            cfg: SearchConfig {
                k: TOP,
                budget: 32,
                tsb: true,
                ranking: Ranking::Coarse,
                ..SearchConfig::default()
            },
            hits: Vec::with_capacity(TOP),
            truncated: 0,
        }
    }
}

impl Engine for Keyhammer {
    fn run(&mut self, query: &str) {
        self.hits.clear();
        if let Ok(out) = self
            .searcher
            .search(&self.trie, &self.costs, query.as_bytes(), &self.cfg)
        {
            self.truncated += usize::from(out.stats.truncated);
            self.hits.extend(out.hits.iter().map(|h| h.id));
        }
    }
    fn top(&self) -> Vec<&str> {
        self.hits.iter().map(|&id| self.trie.term(id)).collect()
    }
    fn failures(&self) -> usize {
        self.truncated
    }
}

// --- SymSpell ---------------------------------------------------------------

pub type SymIndex = SymSpell<AsciiStringStrategy>;

/// Builds the SymSpell index: max dictionary edit distance 2, prefix length 7
/// (the crate's default), count threshold 0 so that weight-0 words are kept.
/// Counts are the dictionary weights.
pub fn build_symspell(words: &[(String, u16)]) -> Rc<SymIndex> {
    let mut s: SymIndex = SymSpellBuilder::default()
        .max_dictionary_edit_distance(RADIUS as i64)
        .prefix_length(7)
        .count_threshold(0)
        .build()
        .expect("symspell: builder");
    let mut line = String::new();
    for (w, f) in words {
        line.clear();
        line.push_str(w);
        line.push('\t');
        line.push_str(&f.to_string());
        s.load_dictionary_line(&line, 0, 1, "\t");
    }
    Rc::new(s)
}

pub struct Sym {
    index: Rc<SymIndex>,
    /// When true, re-rank by (distance, higher count, term) instead of the
    /// crate's own order.
    rerank: bool,
    out: Vec<String>,
}

impl Sym {
    pub fn new(index: Rc<SymIndex>, rerank: bool) -> Self {
        Self {
            index,
            rerank,
            out: Vec::with_capacity(TOP),
        }
    }
}

impl Engine for Sym {
    fn run(&mut self, query: &str) {
        // Verbosity::All is needed to get more than the closest suggestions.
        let mut sugg = self.index.lookup(query, Verbosity::All, RADIUS as i64);
        if self.rerank {
            sugg.sort_unstable_by(|a, b| {
                (a.distance, Reverse(a.count), &a.term).cmp(&(
                    b.distance,
                    Reverse(b.count),
                    &b.term,
                ))
            });
        }
        sugg.truncate(TOP);
        self.out.clear();
        self.out.extend(sugg.into_iter().map(|s| s.term));
    }
    fn top(&self) -> Vec<&str> {
        self.out.iter().map(String::as_str).collect()
    }
}

// --- fst --------------------------------------------------------------------

pub struct Fst {
    map: Map<Vec<u8>>,
    out: Vec<(usize, Reverse<u64>, Vec<u8>)>,
    too_many_states: usize,
}

impl Fst {
    /// Builds an `fst::Map` from term to weight. The builder needs the keys in
    /// byte order, so the terms are sorted first (included in the build time).
    pub fn build(words: &[(String, u16)]) -> Self {
        let mut sorted: Vec<(&str, u16)> = words.iter().map(|(w, f)| (w.as_str(), *f)).collect();
        sorted.sort_unstable();
        sorted.dedup_by(|a, b| a.0 == b.0);
        let mut b = MapBuilder::memory();
        for (w, f) in &sorted {
            b.insert(w, u64::from(*f)).expect("fst: insert");
        }
        drop(sorted);
        Self {
            map: b.into_map(),
            out: Vec::new(),
            too_many_states: 0,
        }
    }

    /// Size of the serialised automaton in bytes.
    pub fn bytes(&self) -> usize {
        self.map.as_fst().size()
    }
}

impl Engine for Fst {
    fn run(&mut self, query: &str) {
        self.out.clear();
        let lev = match Levenshtein::new(query, RADIUS as u32) {
            Ok(l) => l,
            Err(_) => {
                self.too_many_states += 1;
                return;
            }
        };
        let mut stream = self.map.search(&lev).into_stream();
        while let Some((k, v)) = stream.next() {
            // The automaton only says "within 2"; the distance for ranking is
            // recomputed (plain Levenshtein, the automaton's edit model).
            let term = std::str::from_utf8(k).unwrap_or("");
            let d = strsim::levenshtein(query, term);
            self.out.push((d, Reverse(v), k.to_vec()));
        }
        rank_and_cut(&mut self.out);
    }
    fn top(&self) -> Vec<&str> {
        self.out
            .iter()
            .map(|(_, _, k)| std::str::from_utf8(k).unwrap_or(""))
            .collect()
    }
    fn failures(&self) -> usize {
        self.too_many_states
    }
}

// --- strsim brute force ------------------------------------------------------

pub struct Brute {
    terms: Vec<(Box<str>, u16)>,
    out: Vec<(usize, Reverse<u64>, u32)>,
}

impl Brute {
    /// The "index" is the term list itself, sorted by term so that the
    /// position breaks ties in term order.
    pub fn build(words: &[(String, u16)]) -> Self {
        let mut terms: Vec<(Box<str>, u16)> =
            words.iter().map(|(w, f)| (w.as_str().into(), *f)).collect();
        terms.sort_unstable();
        Self {
            terms,
            out: Vec::new(),
        }
    }
}

impl Engine for Brute {
    fn run(&mut self, query: &str) {
        self.out.clear();
        for (i, (t, f)) in self.terms.iter().enumerate() {
            // A length difference above the radius already rules the term out;
            // this skip does not change the result.
            if t.len().abs_diff(query.len()) > RADIUS {
                continue;
            }
            let d = strsim::osa_distance(query, t);
            if d <= RADIUS {
                self.out.push((d, Reverse(u64::from(*f)), i as u32));
            }
        }
        rank_and_cut(&mut self.out);
    }
    fn top(&self) -> Vec<&str> {
        self.out
            .iter()
            .map(|&(_, _, i)| &*self.terms[i as usize].0)
            .collect()
    }
}

// --- BK-tree -----------------------------------------------------------------

pub struct Entry {
    term: Box<str>,
    weight: u16,
}

impl Borrow<str> for Entry {
    fn borrow(&self) -> &str {
        &self.term
    }
}

/// Unrestricted Damerau-Levenshtein distance (a true metric, which a BK-tree
/// needs; optimal string alignment is not one).
pub struct DamerauMetric;

impl bk_tree::Metric<str> for DamerauMetric {
    fn distance(&self, a: &str, b: &str) -> u32 {
        strsim::damerau_levenshtein(a, b) as u32
    }
    fn threshold_distance(&self, a: &str, b: &str, threshold: u32) -> Option<u32> {
        if a.len().abs_diff(b.len()) > threshold as usize {
            return None;
        }
        let d = strsim::damerau_levenshtein(a, b) as u32;
        (d <= threshold).then_some(d)
    }
}

impl bk_tree::Metric<Entry> for DamerauMetric {
    fn distance(&self, a: &Entry, b: &Entry) -> u32 {
        bk_tree::Metric::<str>::distance(self, &a.term, &b.term)
    }
    fn threshold_distance(&self, a: &Entry, b: &Entry, threshold: u32) -> Option<u32> {
        bk_tree::Metric::<str>::threshold_distance(self, &a.term, &b.term, threshold)
    }
}

pub struct BkTree {
    tree: bk_tree::BKTree<Entry, DamerauMetric>,
    out: Vec<(usize, Reverse<u64>, Box<str>)>,
}

impl BkTree {
    /// Inserts the terms in dictionary-file order (the files are shuffled).
    pub fn build(words: &[(String, u16)]) -> Self {
        let mut tree: bk_tree::BKTree<Entry, DamerauMetric> = bk_tree::BKTree::new(DamerauMetric);
        for (w, f) in words {
            tree.add(Entry {
                term: w.as_str().into(),
                weight: *f,
            });
        }
        Self {
            tree,
            out: Vec::new(),
        }
    }
}

impl Engine for BkTree {
    fn run(&mut self, query: &str) {
        self.out.clear();
        for (d, e) in self.tree.find(query, RADIUS as u32) {
            self.out
                .push((d as usize, Reverse(u64::from(e.weight)), e.term.clone()));
        }
        rank_and_cut(&mut self.out);
    }
    fn top(&self) -> Vec<&str> {
        self.out.iter().map(|(_, _, t)| &**t).collect()
    }
}
