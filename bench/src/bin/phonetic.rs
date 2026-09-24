// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

//! Phonetic and spelling-rule candidate generation, prototyped outside the core
//! (issue #22). Protocol: `docs/benchmarks/recall-beyond-two-edits.md`.
//!
//! Three methods are compared with the default engine (budget 32) and the
//! budget-48 preset, all on the 274 137-word dictionary, `Coarse` ranking:
//!
//! - `rules-hw`: a handwritten list of spelling substitutions (fixed in the source
//!   before any corpus was looked at), each applied to the query as an extra
//!   operation of cost 8, 16 or 24; the query variant is searched with the
//!   unchanged engine and the variant's hit costs are shifted by the rule cost.
//! - `rules-learned`: the same mechanism with rules mined from the training
//!   pairs (residue between typo and right word after the longest common prefix
//!   and suffix, with 0 or 1 letter of context on each side).
//! - `phonetic`: candidates that share the Metaphone-like or Soundex key of the
//!   query, ranked by unit edit distance then weight, injected into `m` slots of
//!   the top 10 (optionally only when the best engine hit is poor).
//!
//! Everything (rules, costs, m, gate, key) is chosen on the TRAINING pairs only
//! (greedy selection with a paired z >= 2 admission test); the test pairs are
//! scored once per selected configuration. Splits are fixed by seed.
//!
//! Run: `cargo run --release -p keyhammer-bench --bin phonetic -- bench/data`

#[path = "../recall_common.rs"]
mod common;

use std::collections::{HashMap, HashSet};

use common::*;
use keyhammer::cost::{CostModel, whole_units};
use keyhammer::search::{SearchConfig, Searcher};
use keyhammer::trie::Trie;

const THREADS: usize = 4;
const BUDGET: u16 = 32;
const VARIANT_K: usize = 30;
const RULE_COSTS: [u16; 3] = [8, 16, 24];
const MAX_RULES: usize = 20;
const ADMIT_Z: f64 = 2.0;
const SPLIT_SEED: u64 = 20260925;
const SAMPLE_SEED: u64 = 20260926;
const BB_LARGE: usize = 3000;

type H = (u32, u16, u16); // id, cost, weight

fn par_map<T: Send, F: Fn(usize, &mut Searcher) -> T + Sync>(n: usize, f: F) -> Vec<T> {
    let mut out: Vec<Option<T>> = (0..n).map(|_| None).collect();
    let chunk = n.div_ceil(THREADS).max(1);
    std::thread::scope(|sc| {
        for (ci, slice) in out.chunks_mut(chunk).enumerate() {
            let f = &f;
            sc.spawn(move || {
                let mut s = Searcher::new();
                for (j, slot) in slice.iter_mut().enumerate() {
                    *slot = Some(f(ci * chunk + j, &mut s));
                }
            });
        }
    });
    out.into_iter().map(|x| x.unwrap()).collect()
}

struct Env {
    trie: Trie,
    cm: CostModel,
    ids: HashMap<String, u32>,
    key_mp: HashMap<Vec<u8>, Vec<u32>>,
    key_sx: HashMap<Vec<u8>, Vec<u32>>,
}

struct Q {
    typo: Vec<u8>,
    right: u32,
    base32: Vec<H>,
    nodes32: usize,
    base48_ids: Vec<u32>,
    nodes48: usize,
    phon: [Vec<u32>; 2],    // metaphone, soundex: top 10 by (osa, -weight, id)
    cand_recall: [bool; 2], // right word in the top 10 phonetic candidates
    in_bucket: [bool; 2],
    bucket_len: [usize; 2], // candidates scanned (unit edit distance evaluations)
    reach32: bool,          // right word within cost 32 of the query
}

struct Corpus {
    name: String,
    qs: Vec<Q>,
}

fn search_hits(env: &Env, s: &mut Searcher, q: &[u8], budget: u16, k: usize) -> (Vec<H>, usize) {
    let cfg = SearchConfig {
        k,
        budget,
        tsb: true,
        ..SearchConfig::default()
    };
    let out = s.search(&env.trie, &env.cm, q, &cfg).expect("search");
    (
        out.hits.iter().map(|h| (h.id, h.cost, h.weight)).collect(),
        out.stats.nodes_expanded,
    )
}

fn build_corpus(env: &Env, name: &str, pairs: &[(String, String)]) -> Corpus {
    let qs = par_map(pairs.len(), |i, s| {
        let (typo, right) = &pairs[i];
        let t = typo.as_bytes();
        let rid = env.ids[right];
        let (base32, nodes32) = search_hits(env, s, t, 32, VARIANT_K);
        let (h48, nodes48) = search_hits(env, s, t, 48, 10);
        // exact reachability at 32: a k=10 search may miss a reachable word, so ask for many
        let big = SearchConfig {
            k: 100_000,
            budget: 32,
            tsb: true,
            max_nodes: 100_000_000,
            ..SearchConfig::default()
        };
        let reach32 = s
            .search(&env.trie, &env.cm, t, &big)
            .expect("search")
            .hits
            .iter()
            .any(|h| h.id == rid);
        let mut phon: [Vec<u32>; 2] = [Vec::new(), Vec::new()];
        let mut cand_recall = [false; 2];
        let mut in_bucket = [false; 2];
        let mut bucket_len = [0usize; 2];
        for (ki, (key, idx)) in [(metaphone(t), &env.key_mp), (soundex(t), &env.key_sx)]
            .into_iter()
            .enumerate()
        {
            if let Some(b) = idx.get(&key) {
                in_bucket[ki] = b.contains(&rid);
                bucket_len[ki] = b.len();
                let mut v: Vec<(usize, u16, u32)> = b
                    .iter()
                    .map(|&id| {
                        (
                            osa(t, env.trie.term(id).as_bytes()),
                            u16::MAX - env.trie.weight(id),
                            id,
                        )
                    })
                    .collect();
                v.sort();
                v.truncate(10);
                phon[ki] = v.iter().map(|x| x.2).collect();
                cand_recall[ki] = phon[ki].contains(&rid);
            }
        }
        Q {
            typo: t.to_vec(),
            right: rid,
            base32,
            nodes32,
            base48_ids: h48.iter().map(|h| h.0).collect(),
            nodes48,
            phon,
            cand_recall,
            in_bucket,
            bucket_len,
            reach32,
        }
    });
    Corpus {
        name: name.to_string(),
        qs,
    }
}

// ---------------------------------------------------------------- rules

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Rule {
    from: Vec<u8>,
    to: Vec<u8>,
}

impl Rule {
    fn show(&self) -> String {
        format!(
            "{}->{}",
            String::from_utf8_lossy(&self.from),
            if self.to.is_empty() {
                "(empty)".to_string()
            } else {
                String::from_utf8_lossy(&self.to).to_string()
            }
        )
    }
}

/// The handwritten list, fixed before any corpus was inspected: common English
/// spelling alternations, each usable in both directions.
fn handwritten() -> Vec<Rule> {
    let pairs: &[(&str, &str)] = &[
        ("ie", "ei"),
        ("ph", "f"),
        ("ck", "k"),
        ("c", "k"),
        ("c", "s"),
        ("s", "z"),
        ("ss", "s"),
        ("ll", "l"),
        ("tt", "t"),
        ("rr", "r"),
        ("nn", "n"),
        ("mm", "m"),
        ("pp", "p"),
        ("cc", "c"),
        ("ff", "f"),
        ("dd", "d"),
        ("gg", "g"),
        ("bb", "b"),
        ("kn", "n"),
        ("gn", "n"),
        ("wr", "r"),
        ("mb", "m"),
        ("gh", ""),
        ("gh", "f"),
        ("h", ""),
        ("wh", "w"),
        ("a", "e"),
        ("e", "i"),
        ("i", "y"),
        ("o", "u"),
        ("a", "o"),
        ("u", "a"),
        ("ance", "ence"),
        ("ant", "ent"),
        ("able", "ible"),
        ("er", "or"),
        ("ar", "er"),
        ("ary", "ery"),
        ("tion", "sion"),
        ("cion", "tion"),
        ("ch", "sh"),
        ("tch", "ch"),
        ("dge", "ge"),
        ("x", "cks"),
        ("x", "ks"),
        ("qu", "kw"),
        ("j", "g"),
        ("ce", "se"),
        ("ci", "si"),
        ("sc", "s"),
        ("sc", "c"),
        ("ps", "s"),
        ("y", "ie"),
        ("ee", "ea"),
        ("oo", "ou"),
        ("ai", "ay"),
        ("ea", "e"),
        ("ou", "o"),
        ("au", "o"),
        ("ui", "u"),
        ("ure", "er"),
        ("eur", "er"),
    ];
    let mut v = Vec::new();
    for &(a, b) in pairs {
        v.push(Rule {
            from: a.as_bytes().to_vec(),
            to: b.as_bytes().to_vec(),
        });
        if !b.is_empty() {
            v.push(Rule {
                from: b.as_bytes().to_vec(),
                to: a.as_bytes().to_vec(),
            });
        }
    }
    v
}

/// Rules mined from training pairs: count of each (typo residue -> right residue).
fn mine(train: &[&(String, String)], top: usize, min_count: usize) -> Vec<Rule> {
    let mut cnt: HashMap<Rule, usize> = HashMap::new();
    for (typo, right) in train.iter().map(|p| (&p.0, &p.1)) {
        let (t, r) = (typo.as_bytes(), right.as_bytes());
        let mut p = 0;
        while p < t.len().min(r.len()) && t[p] == r[p] {
            p += 1;
        }
        let mut s = 0;
        while s < t.len().min(r.len()) - p && t[t.len() - 1 - s] == r[r.len() - 1 - s] {
            s += 1;
        }
        let (a, b) = (&t[p..t.len() - s], &r[p..r.len() - s]);
        if a.is_empty() || a.len() > 3 || b.len() > 3 {
            continue;
        }
        for (l, rr) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            if p < l || s < rr {
                continue;
            }
            let from = t[p - l..t.len() - s + rr].to_vec();
            let to = r[p - l..r.len() - s + rr].to_vec();
            if from != to && from.len() <= 5 {
                *cnt.entry(Rule { from, to }).or_default() += 1;
            }
        }
    }
    let mut v: Vec<(Rule, usize)> = cnt.into_iter().filter(|x| x.1 >= min_count).collect();
    v.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then(a.0.from.cmp(&b.0.from))
            .then(a.0.to.cmp(&b.0.to))
    });
    v.truncate(top);
    v.into_iter().map(|x| x.0).collect()
}

struct Var {
    hits: Vec<(u32, u16, u16)>,
    nodes: usize,
}

fn occurrences(q: &[u8], from: &[u8]) -> Vec<usize> {
    (0..=q.len().saturating_sub(from.len()))
        .filter(|&i| q.len() >= from.len() && &q[i..i + from.len()] == from)
        .collect()
}

fn variant(env: &Env, s: &mut Searcher, q: &[u8], rule: &Rule) -> Var {
    let mut best: HashMap<u32, (u16, u16)> = HashMap::new();
    let mut nodes = 0;
    for pos in occurrences(q, &rule.from) {
        let mut v = q[..pos].to_vec();
        v.extend_from_slice(&rule.to);
        v.extend_from_slice(&q[pos + rule.from.len()..]);
        if v.is_empty() || v.len() > 128 || v == q {
            continue;
        }
        let (h, n) = search_hits(env, s, &v, BUDGET, VARIANT_K);
        nodes += n;
        for (id, c, w) in h {
            let e = best.entry(id).or_insert((c, w));
            if c < e.0 {
                e.0 = c;
            }
        }
    }
    let mut hits: Vec<(u32, u16, u16)> = best.into_iter().map(|(i, (c, w))| (i, c, w)).collect();
    hits.sort();
    Var { hits, nodes }
}

/// Variants of one rule for every query of a corpus (memoized by the caller).
fn rule_cache(env: &Env, c: &Corpus, rule: &Rule) -> Vec<Var> {
    par_map(c.qs.len(), |i, s| variant(env, s, &c.qs[i].typo, rule))
}

struct Cache<'a> {
    env: &'a Env,
    store: Vec<Vec<Var>>,
    idx: HashMap<(String, Rule), usize>,
}

impl Cache<'_> {
    fn id(&mut self, c: &Corpus, r: &Rule) -> usize {
        let key = (c.name.clone(), r.clone());
        if let Some(&i) = self.idx.get(&key) {
            return i;
        }
        let v = rule_cache(self.env, c, r);
        self.store.push(v);
        self.idx.insert(key, self.store.len() - 1);
        self.store.len() - 1
    }
}

/// Top-10 ids after merging the base hits with the shifted hits of the selected rules.
fn merge(base: &[H], extra: &[(&Var, u16)]) -> Vec<u32> {
    let mut all: Vec<(u32, u16, u16)> = base.to_vec();
    for (v, r) in extra {
        for &(id, c, w) in &v.hits {
            if c + r <= BUDGET {
                all.push((id, c + r, w));
            }
        }
    }
    all.sort_by_key(|x| (x.0, x.1));
    all.dedup_by_key(|x| x.0);
    all.sort_by_key(|x| (whole_units(x.1), std::cmp::Reverse(x.2), x.0));
    all.iter().take(10).map(|x| x.0).collect()
}

fn base_top10(q: &Q) -> Vec<u32> {
    q.base32.iter().take(10).map(|h| h.0).collect()
}

#[derive(Clone)]
struct RuleSet {
    sel: Vec<(Rule, u16, usize)>, // rule, cost, cache id
}

/// Greedy forward selection on `train` (indices into `c.qs`). Admission: paired
/// train gain in MRR@10 with z >= ADMIT_Z.
fn select_rules(cache: &mut Cache, c: &Corpus, train: &[usize], cands: &[Rule]) -> RuleSet {
    let cids: Vec<usize> = cands.iter().map(|r| cache.id(c, r)).collect();
    let store = &cache.store;
    let mut sel: Vec<(Rule, u16, usize)> = Vec::new();
    let mut cur: Vec<f64> = train
        .iter()
        .map(|&i| rr_of(&base_top10(&c.qs[i]), c.qs[i].right))
        .collect();
    for _ in 0..MAX_RULES {
        let mut best: Option<(f64, usize, u16, Vec<f64>)> = None;
        for (ci, cand) in cands.iter().enumerate() {
            if sel.iter().any(|(r, _, _)| r == cand) {
                continue;
            }
            for &rc in &RULE_COSTS {
                let mut rr = Vec::with_capacity(train.len());
                for (ti, &qi) in train.iter().enumerate() {
                    let q = &c.qs[qi];
                    let v = &store[cids[ci]][qi];
                    if v.hits.is_empty() {
                        rr.push(cur[ti]);
                        continue;
                    }
                    let mut extra: Vec<(&Var, u16)> = sel
                        .iter()
                        .map(|(_, cost, id)| (&store[*id][qi], *cost))
                        .collect();
                    extra.push((v, rc));
                    rr.push(rr_of(&merge(&q.base32, &extra), q.right));
                }
                let (d, se) = paired(&rr, &cur);
                let z = if se > 0.0 { d / se } else { 0.0 };
                if z >= ADMIT_Z && best.as_ref().is_none_or(|b| d > b.0) {
                    best = Some((d, ci, rc, rr));
                }
            }
        }
        match best {
            Some((_, ci, rc, rr)) => {
                sel.push((cands[ci].clone(), rc, cids[ci]));
                cur = rr;
            }
            None => break,
        }
    }
    RuleSet { sel }
}

/// (rr per query, nodes per query) of the selected rule set on `idx` of `c`.
fn score_rules(
    cache: &mut Cache,
    c: &Corpus,
    idx: &[usize],
    rs: &RuleSet,
) -> (Vec<f64>, Vec<usize>) {
    let ids: Vec<usize> = rs.sel.iter().map(|(r, _, _)| cache.id(c, r)).collect();
    let store = &cache.store;
    let mut rr = Vec::new();
    let mut nodes = Vec::new();
    for &qi in idx {
        let q = &c.qs[qi];
        let extra: Vec<(&Var, u16)> = rs
            .sel
            .iter()
            .zip(&ids)
            .map(|((_, cost, _), id)| (&store[*id][qi], *cost))
            .collect();
        rr.push(if extra.is_empty() {
            rr_of(&base_top10(q), q.right)
        } else {
            rr_of(&merge(&q.base32, &extra), q.right)
        });
        nodes.push(q.nodes32 + extra.iter().map(|e| e.0.nodes).sum::<usize>());
    }
    (rr, nodes)
}

// ------------------------------------------------------------- phonetic

#[derive(Clone, Copy, Debug)]
struct PhonCfg {
    key: usize, // 0 metaphone, 1 soundex
    m: usize,
    gate: i32, // inject only when the best engine cost is above this (-1: always)
}

fn phon_ids(q: &Q, cfg: Option<PhonCfg>) -> Vec<u32> {
    let e = base_top10(q);
    let Some(cfg) = cfg else { return e };
    let best = q
        .base32
        .iter()
        .map(|h| i32::from(h.1))
        .min()
        .unwrap_or(i32::MAX);
    if best <= cfg.gate {
        return e;
    }
    let keep = 10 - cfg.m;
    let mut out: Vec<u32> = e.iter().take(keep).copied().collect();
    let mut added = 0;
    for &p in &q.phon[cfg.key] {
        if added == cfg.m {
            break;
        }
        if !out.contains(&p) {
            out.push(p);
            added += 1;
        }
    }
    for &x in &e {
        if out.len() >= 10 {
            break;
        }
        if !out.contains(&x) {
            out.push(x);
        }
    }
    out
}

fn select_phon(c: &Corpus, train: &[usize]) -> Option<PhonCfg> {
    let base: Vec<f64> = train
        .iter()
        .map(|&i| rr_of(&base_top10(&c.qs[i]), c.qs[i].right))
        .collect();
    let mut best: Option<(f64, PhonCfg)> = None;
    for key in 0..2 {
        for m in [1, 2, 3, 5] {
            for gate in [-1, 16, 24] {
                let cfg = PhonCfg { key, m, gate };
                let rr: Vec<f64> = train
                    .iter()
                    .map(|&i| rr_of(&phon_ids(&c.qs[i], Some(cfg)), c.qs[i].right))
                    .collect();
                let (d, se) = paired(&rr, &base);
                let z = if se > 0.0 { d / se } else { 0.0 };
                if z >= ADMIT_Z && best.as_ref().is_none_or(|b| d > b.0) {
                    best = Some((d, cfg));
                }
            }
        }
    }
    best.map(|b| b.1)
}

// -------------------------------------------------------------- reporting

struct Row {
    name: String,
    rr: Vec<f64>,
    hit: Vec<f64>,
    r1: Vec<f64>,
    nodes: Vec<usize>,
}

fn mk(name: &str, rr: Vec<f64>, nodes: Vec<usize>) -> Row {
    Row {
        name: name.to_string(),
        hit: rr.iter().map(|&x| f64::from(x > 0.0)).collect(),
        r1: rr.iter().map(|&x| f64::from(x == 1.0)).collect(),
        rr,
        nodes,
    }
}

fn print_rows(rows: &[Row]) {
    println!(
        "| method | MRR@10 | R@1 | R@10 | nodes/query | dMRR vs default (SE) | dR@10 vs default (SE) | dMRR vs preset (SE) |"
    );
    println!("|---|---|---|---|---|---|---|---|");
    for (i, r) in rows.iter().enumerate() {
        let (dm, dh) = if i == 0 {
            (String::new(), String::new())
        } else {
            let (a, sa) = paired(&r.rr, &rows[0].rr);
            let (h, sh) = paired(&r.hit, &rows[0].hit);
            (format!("{a:+.4} ({sa:.4})"), format!("{h:+.4} ({sh:.4})"))
        };
        let dp = if i < 2 {
            String::new()
        } else {
            let (a, sa) = paired(&r.rr, &rows[1].rr);
            format!("{a:+.4} ({sa:.4})")
        };
        println!(
            "| {} | {:.3} | {:.3} | {:.3} | {:.0} | {dm} | {dh} | {dp} |",
            r.name,
            mean(&r.rr),
            mean(&r.r1),
            mean(&r.hit),
            r.nodes.iter().sum::<usize>() as f64 / r.nodes.len() as f64
        );
    }
}

fn concat(a: Row, b: Row) -> Row {
    Row {
        name: a.name,
        rr: [a.rr, b.rr].concat(),
        hit: [a.hit, b.hit].concat(),
        r1: [a.r1, b.r1].concat(),
        nodes: [a.nodes, b.nodes].concat(),
    }
}

struct Diag {
    def_rr: f64,
    ph_rr: f64,
    nhits: usize,
    seen: bool, // the right word is the right word of some training pair
}

struct Fold {
    rows: Vec<Row>, // default, hr48, hw, learned, phonetic
    diag: Vec<Diag>,
}

/// Where the phonetic gain comes from: by the engine's hit count and by the rank of the right word.
fn print_diag(d: &[Diag]) {
    let n = d.len() as f64;
    let cnt = |f: &dyn Fn(&Diag) -> bool| d.iter().filter(|x| f(x)).count();
    println!(
        "Diagnostic (n = {}): the default returns 0 hits for {} queries and fewer than 5 for {}; the right word is the right word of a training pair for {} ({} pairs are split by pair, not by intended word).
",
        d.len(),
        cnt(&|x| x.nhits == 0),
        cnt(&|x| x.nhits < 5),
        cnt(&|x| x.seen),
        d.len()
    );
    println!(
        "| phonetic rank of the right word | queries gained | share of the MRR gain (sum / n) |
|---|---|---|"
    );
    for (lab, lo, hi) in [
        ("1", 1.0, 1.0),
        ("2 to 5", 0.2, 0.5),
        ("6 to 10", 0.1, 0.1667),
    ] {
        let g: Vec<f64> = d
            .iter()
            .filter(|x| x.ph_rr > x.def_rr && x.ph_rr >= lo - 1e-9 && x.ph_rr <= hi + 1e-9)
            .map(|x| x.ph_rr - x.def_rr)
            .collect();
        println!(
            "| {lab} | {} | {:+.4} |",
            g.len(),
            g.iter().sum::<f64>() / n
        );
    }
    let l: Vec<f64> = d
        .iter()
        .filter(|x| x.ph_rr < x.def_rr)
        .map(|x| x.ph_rr - x.def_rr)
        .collect();
    println!(
        "| queries that got worse | {} | {:+.4} |
",
        l.len(),
        l.iter().sum::<f64>() / n
    );
}

/// Select on `train`, score on `test`; both index `c` (same corpus) or `train_c` / `test_c`.
#[allow(clippy::too_many_arguments)]
fn run_fold(
    cache: &mut Cache,
    train_c: &Corpus,
    train: &[usize],
    train_pairs: &[&(String, String)],
    test_c: &Corpus,
    test: &[usize],
    log: &mut Vec<String>,
) -> Fold {
    let hw = handwritten();
    let hw_sel = select_rules(cache, train_c, train, &hw);
    let learned_cands = mine(train_pairs, 80, 4);
    let ln_sel = select_rules(cache, train_c, train, &learned_cands);
    let ph = select_phon(train_c, train);
    log.push(format!(
        "selected on {} training pairs of {}:\n  - handwritten ({} of {} candidates): {}\n  - learned ({} of {} candidates): {}\n  - phonetic: {}",
        train.len(),
        train_c.name,
        hw_sel.sel.len(),
        hw.len(),
        hw_sel.sel.iter().map(|(r, c, _)| format!("{} @{c}", r.show())).collect::<Vec<_>>().join(", "),
        ln_sel.sel.len(),
        learned_cands.len(),
        ln_sel.sel.iter().map(|(r, c, _)| format!("{} @{c}", r.show())).collect::<Vec<_>>().join(", "),
        ph.map_or("none admitted".into(), |p| format!(
            "{} key, m = {}, gate: best cost > {}",
            ["Metaphone-like", "Soundex"][p.key],
            p.m,
            p.gate
        )),
    ));
    let d_rr: Vec<f64> = test
        .iter()
        .map(|&i| rr_of(&base_top10(&test_c.qs[i]), test_c.qs[i].right))
        .collect();
    let d_n: Vec<usize> = test.iter().map(|&i| test_c.qs[i].nodes32).collect();
    let h_rr: Vec<f64> = test
        .iter()
        .map(|&i| rr_of(&test_c.qs[i].base48_ids, test_c.qs[i].right))
        .collect();
    let h_n: Vec<usize> = test.iter().map(|&i| test_c.qs[i].nodes48).collect();
    let (hw_rr, hw_n) = score_rules(cache, test_c, test, &hw_sel);
    let (ln_rr, ln_n) = score_rules(cache, test_c, test, &ln_sel);
    let p_rr: Vec<f64> = test
        .iter()
        .map(|&i| rr_of(&phon_ids(&test_c.qs[i], ph), test_c.qs[i].right))
        .collect();
    let trained: HashSet<u32> = train.iter().map(|&i| train_c.qs[i].right).collect();
    let diag: Vec<Diag> = test
        .iter()
        .zip(p_rr.iter().zip(d_rr.iter()))
        .map(|(&i, (&p, &d))| Diag {
            def_rr: d,
            ph_rr: p,
            nhits: test_c.qs[i].base32.len().min(10),
            seen: trained.contains(&test_c.qs[i].right),
        })
        .collect();
    Fold {
        diag,
        rows: vec![
            mk("default (budget 32)", d_rr, d_n.clone()),
            mk("preset (budget 48)", h_rr, h_n),
            mk("rules, handwritten list", hw_rr, hw_n),
            mk("rules, mined from training pairs", ln_rr, ln_n),
            mk("phonetic key injection", p_rr, d_n),
        ],
    }
}

fn main() {
    let dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "bench/data".to_string());
    let full = read_words(&format!("{dir}/words-full.tsv"));
    let items: Vec<(&str, u16)> = full.iter().map(|(w, f)| (w.as_str(), *f)).collect();
    let trie = Trie::build(&items).expect("trie");
    let ids: HashMap<String, u32> = (0..trie.len() as u32)
        .map(|i| (trie.term(i).to_string(), i))
        .collect();
    let mut key_mp: HashMap<Vec<u8>, Vec<u32>> = HashMap::new();
    let mut key_sx: HashMap<Vec<u8>, Vec<u32>> = HashMap::new();
    for id in 0..trie.len() as u32 {
        let t = trie.term(id).as_bytes();
        if t.iter().all(u8::is_ascii_lowercase) && !t.is_empty() {
            key_mp.entry(metaphone(t)).or_default().push(id);
            key_sx.entry(soundex(t)).or_default().push(id);
        }
    }
    let env = Env {
        trie,
        cm: CostModel::qwerty(),
        ids,
        key_mp,
        key_sx,
    };

    // corpora
    let bb300 = read_pairs(&format!("{dir}/tests.tsv"));
    let wiki = [
        read_pairs(&format!("{dir}/wiki.tsv")),
        read_pairs(&format!("{dir}/wiki-holdout.tsv")),
    ]
    .concat();
    let gtc = [
        read_pairs(&format!("{dir}/gtc.tsv")),
        read_pairs(&format!("{dir}/gtc-holdout.tsv")),
    ]
    .concat();
    let m0: HashSet<(String, String)> = bb300.iter().cloned().collect();
    let dict: HashSet<&str> = full.iter().map(|w| w.0.as_str()).collect();
    let mut bb_all: Vec<(String, String)> = Vec::new();
    let mut seen = HashSet::new();
    let mut cur = String::new();
    let text = std::fs::read_to_string(format!("{dir}/missp.dat"))
        .unwrap_or_else(|e| die(&format!("missp.dat: {e}")));
    let az = |s: &str, min: usize| s.len() >= min && s.bytes().all(|b| b.is_ascii_lowercase());
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(c) = line.strip_prefix('$') {
            cur = c.to_lowercase();
        } else {
            let t = line.to_lowercase();
            if az(&t, 3)
                && az(&cur, 3)
                && dict.contains(cur.as_str())
                && !dict.contains(t.as_str())
                && t != cur
            {
                let p = (t, cur.clone());
                if !m0.contains(&p) && seen.insert(p.clone()) {
                    bb_all.push(p);
                }
            }
        }
    }
    let usable = bb_all.len();
    shuffle(&mut bb_all, SAMPLE_SEED);
    bb_all.truncate(BB_LARGE);
    println!(
        "Birkbeck full list: {usable} usable pairs outside the 300 M0 pairs; sample of {} used.",
        bb_all.len()
    );

    let c_bb300 = build_corpus(&env, "Birkbeck M0 300", &bb300);
    let c_bb = build_corpus(&env, "Birkbeck sample 3000", &bb_all);
    let c_wiki = build_corpus(&env, "Wikipedia all 3754", &wiki);
    let c_gtc = build_corpus(&env, "GitHub Typo Corpus 3000", &gtc);
    let mut cache = Cache {
        env: &env,
        store: Vec::new(),
        idx: HashMap::new(),
    };

    // question 1b: how phonetic are the pairs the default cannot reach
    println!("\n## Unreachable at budget 32 and phonetic keys (274 137 words)\n");
    println!(
        "| corpus | pairs | unreachable at 32 | of those: right word in query's Metaphone bucket | in top 10 of bucket | in Soundex bucket | in top 10 of bucket | reachable but not in top 10 (default) |"
    );
    println!("|---|---|---|---|---|---|---|---|");
    for c in [&c_bb300, &c_bb, &c_wiki, &c_gtc] {
        let un: Vec<&Q> = c.qs.iter().filter(|q| !q.reach32).collect();
        let cnt = |f: &dyn Fn(&Q) -> bool| un.iter().filter(|q| f(q)).count();
        let missed =
            c.qs.iter()
                .filter(|q| q.reach32 && !base_top10(q).contains(&q.right))
                .count();
        println!(
            "| {} | {} | {} | {} | {} | {} | {} | {missed} |",
            c.name,
            c.qs.len(),
            un.len(),
            cnt(&|q| q.in_bucket[0]),
            cnt(&|q| q.cand_recall[0]),
            cnt(&|q| q.in_bucket[1]),
            cnt(&|q| q.cand_recall[1]),
        );
    }

    println!(
        "\n## Cost of the phonetic candidates: bucket sizes (unit edit distance evaluations per query)\n"
    );
    println!("| corpus | key | mean | median | p95 | max |\n|---|---|---|---|---|---|");
    for c in [&c_bb300, &c_bb, &c_wiki, &c_gtc] {
        for (ki, kn) in ["Metaphone-like", "Soundex"].into_iter().enumerate() {
            let mut v: Vec<usize> = c.qs.iter().map(|q| q.bucket_len[ki]).collect();
            v.sort();
            let p = |f: f64| {
                v[((v.len() as f64 * f).ceil() as usize)
                    .saturating_sub(1)
                    .min(v.len() - 1)]
            };
            println!(
                "| {} | {kn} | {:.0} | {} | {} | {} |",
                c.name,
                v.iter().sum::<usize>() as f64 / v.len() as f64,
                p(0.5),
                p(0.95),
                v[v.len() - 1]
            );
        }
    }

    println!(
        "\n## Rule coverage: unreachable-at-32 pairs whose right word one handwritten rule (applied once, at any position) would bring within the budget\n"
    );
    println!("(Lower bound: variant searches return at most {VARIANT_K} hits.)\n");
    println!(
        "| corpus | unreachable at 32 | rule cost 8 | rule cost 16 | rule cost 24 |\n|---|---|---|---|---|"
    );
    {
        let hw = handwritten();
        for c in [&c_bb300, &c_bb, &c_wiki, &c_gtc] {
            let cids: Vec<usize> = hw.iter().map(|r| cache.id(c, r)).collect();
            let store = &cache.store;
            let mut n = [0usize; 3];
            let mut un = 0;
            for (qi, q) in c.qs.iter().enumerate() {
                if q.reach32 {
                    continue;
                }
                un += 1;
                let minc = cids
                    .iter()
                    .filter_map(|&cid| {
                        store[cid][qi]
                            .hits
                            .iter()
                            .find(|h| h.0 == q.right)
                            .map(|h| h.1)
                    })
                    .min();
                if let Some(m) = minc {
                    for (k, rc) in RULE_COSTS.iter().enumerate() {
                        if m + rc <= BUDGET {
                            n[k] += 1;
                        }
                    }
                }
            }
            println!("| {} | {un} | {} | {} | {} |", c.name, n[0], n[1], n[2]);
        }
    }

    let split = |n: usize| -> (Vec<usize>, Vec<usize>) {
        let mut v: Vec<usize> = (0..n).collect();
        shuffle(&mut v, SPLIT_SEED);
        let h = n / 2;
        let (a, b) = v.split_at(h);
        (a.to_vec(), b.to_vec())
    };

    println!(
        "\n## Within-corpus two-fold test (rules chosen on one half, scored on the other; every query is a test query exactly once)\n"
    );
    let named: [(&Corpus, &[(String, String)]); 3] =
        [(&c_wiki, &wiki), (&c_bb, &bb_all), (&c_gtc, &gtc)];
    for (c, pairs) in named {
        let (h0, h1) = split(c.qs.len());
        let mut log = Vec::new();
        let mut folds = Vec::new();
        for (tr, te) in [(&h0, &h1), (&h1, &h0)] {
            let tp: Vec<&(String, String)> = tr.iter().map(|&i| &pairs[i]).collect();
            folds.push(run_fold(&mut cache, c, tr, &tp, c, te, &mut log));
        }
        println!("### {}\n", c.name);
        for l in &log {
            println!("{l}\n");
        }
        let mut it = folds.into_iter();
        let (mut f0, mut f1) = (it.next().unwrap(), it.next().unwrap());
        let diag: Vec<Diag> = f0.diag.drain(..).chain(f1.diag.drain(..)).collect();
        let rows: Vec<Row> = f0
            .rows
            .into_iter()
            .zip(f1.rows)
            .map(|(a, b)| concat(a, b))
            .collect();
        println!("Pooled test scores over all {} pairs:\n", c.qs.len());
        print_rows(&rows);
        println!();
        print_diag(&diag);
    }

    println!(
        "\n## Cross-corpus test (selected on the whole training corpus, scored on a different corpus)\n"
    );
    let train_sets: [(&Corpus, &[(String, String)]); 2] = [(&c_wiki, &wiki), (&c_bb, &bb_all)];
    for (trc, trp) in train_sets {
        let idx: Vec<usize> = (0..trc.qs.len()).collect();
        let tp: Vec<&(String, String)> = trp.iter().collect();
        // selection is done once per test corpus only to reuse run_fold; the selection is identical
        let mut first = true;
        for tec in [&c_bb300, &c_bb, &c_wiki, &c_gtc] {
            if std::ptr::eq(tec, trc) {
                continue;
            }
            let te: Vec<usize> = (0..tec.qs.len()).collect();
            let mut l2 = Vec::new();
            let f = run_fold(&mut cache, trc, &idx, &tp, tec, &te, &mut l2);
            if first {
                first = false;
                println!("### trained on {}\n", trc.name);
                println!("{}\n", l2[0]);
            }
            println!("Test corpus: {} (n = {})\n", tec.name, tec.qs.len());
            print_rows(&f.rows);
            println!();
            print_diag(&f.diag);
        }
    }
}
