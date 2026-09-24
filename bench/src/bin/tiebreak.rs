// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

//! Pre-registered test of a ranking tie-break variant (issue #43).
//!
//! Protocol: `docs/benchmarks/tiebreak-preregistration.md`, committed before this
//! harness was run. Deterministic quantities only (MRR, ties); no latency.
//!
//! The variant (whole units, then higher weight, then exact cost, then term id)
//! is computed here from the public `Hit`s of the default search: the search is
//! repeated with a growing `k` until every term with at most the 10th hit's
//! whole units has been returned, and those are reordered. The core crate is
//! not touched. A self-check compares the result with `Ranking::Exact` and a
//! very large `k`, sorted by the variant key, on every query.
//!
//! Run: `cargo run --release -p keyhammer-bench --bin tiebreak -- bench/data`
//! after `node bench/fetch-typo-corpora.mjs --holdout`.

use std::collections::HashMap;
use std::fs;

use keyhammer::cost::{COST_UNIT, CostModel, whole_units};
use keyhammer::search::{Hit, Ranking, SearchConfig, Searcher};
use keyhammer::trie::Trie;

const TOP: usize = 10;
const Z95: f64 = 1.96;
/// Categories smaller than this do not enter the decision rule.
const MIN_CATEGORY: usize = 50;
/// A gain of this size is "not excluded" for the inconclusive verdict.
const INCONCLUSIVE_UPPER: f64 = 0.005;
const CORPORA: [&str; 2] = ["gtc-holdout", "wiki-holdout"];
const HUGE_NODES: usize = 100_000_000;

fn die(msg: &str) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(2);
}

fn read_tsv(path: &str) -> Vec<(String, String)> {
    let text =
        fs::read_to_string(path).unwrap_or_else(|e| die(&format!("cannot read {path}: {e}")));
    text.lines()
        .filter(|l| !l.is_empty())
        .map(|l| match l.split_once('\t') {
            Some((a, b)) => (a.to_string(), b.to_string()),
            None => die(&format!("{path}: line without a tab: {l:?}")),
        })
        .collect()
}

/// Unit optimal-string-alignment distance (adjacent transposition counts 1).
fn osa(a: &[u8], b: &[u8]) -> usize {
    let (m, n) = (a.len(), b.len());
    let mut d = vec![vec![0usize; n + 1]; m + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=m {
        for j in 1..=n {
            let c = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + c);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[m][n]
}

/// True when removing the letter at some index of `long` gives `short` and
/// that letter equals an adjacent letter of `long`.
fn removal(long: &[u8], short: &[u8]) -> (bool, bool) {
    let (mut any, mut doubled) = (false, false);
    for i in 0..long.len() {
        let mut t = long.to_vec();
        t.remove(i);
        if t == short {
            any = true;
            let prev = i > 0 && long[i] == long[i - 1];
            let next = i + 1 < long.len() && long[i] == long[i + 1];
            doubled |= prev || next;
        }
    }
    (any, doubled)
}

/// Error categories of a pair, as defined in the pre-registration.
fn classify(typo: &str, correct: &str, cm: &CostModel) -> Vec<&'static str> {
    let (t, c) = (typo.as_bytes(), correct.as_bytes());
    let mut cats = Vec::new();
    let d = osa(t, c);
    if d >= 2 {
        cats.push("two or more edits");
    } else if t.len() == c.len() {
        let diff: Vec<usize> = (0..t.len()).filter(|&i| t[i] != c[i]).collect();
        if diff.len() == 1 {
            let i = diff[0];
            if cm.sub_cost(t[i], c[i], 1) < COST_UNIT {
                cats.push("neighbouring-key substitution");
            } else {
                cats.push("other substitution");
            }
        } else {
            cats.push("transposition");
        }
    } else if t.len() + 1 == c.len() {
        let (ok, dbl) = removal(c, t);
        assert!(ok, "d = 1 but no deletion matches: {typo} {correct}");
        cats.push(if dbl {
            "missing doubled letter"
        } else {
            "missing letter"
        });
    } else {
        let (ok, dbl) = removal(t, c);
        assert!(ok, "d = 1 but no deletion matches: {typo} {correct}");
        cats.push(if dbl {
            "extra doubled letter"
        } else {
            "extra letter"
        });
    }
    if t[0] != c[0] {
        cats.push("first letter differs");
    }
    cats
}

const CATEGORY_ORDER: [&str; 8] = [
    "transposition",
    "missing letter",
    "missing doubled letter",
    "extra letter",
    "extra doubled letter",
    "neighbouring-key substitution",
    "other substitution",
    "two or more edits",
];

fn units(h: &Hit) -> u16 {
    whole_units(h.cost)
}

/// Default key: two hits with the same key are ordered by term id only.
fn key(h: &Hit) -> (u16, u16) {
    (units(h), h.weight)
}

/// Variant key without the term id.
fn vkey(h: &Hit) -> (u16, std::cmp::Reverse<u16>, u16) {
    (units(h), std::cmp::Reverse(h.weight), h.cost)
}

fn has_dup<K: PartialEq>(keys: &[K]) -> bool {
    keys.iter()
        .enumerate()
        .any(|(i, x)| keys[i + 1..].iter().any(|y| y == x))
}

struct Searchers<'a> {
    trie: &'a Trie,
    cm: &'a CostModel,
    s: Searcher,
    truncated_default: usize,
    truncated_expand: usize,
}

impl Searchers<'_> {
    /// Default top 10, and every term with at most the 10th hit's units in
    /// default order (a superset of the default top 10).
    fn run(&mut self, q: &[u8]) -> (Vec<Hit>, Vec<Hit>) {
        let cfg = SearchConfig {
            k: TOP,
            tsb: true,
            ..SearchConfig::default()
        };
        let out = self.s.search(self.trie, self.cm, q, &cfg).expect("search");
        self.truncated_default += usize::from(out.stats.truncated);
        let top = out.hits;
        if top.len() < TOP {
            return (top.clone(), top);
        }
        let u10 = units(&top[TOP - 1]);
        let mut k = 20;
        loop {
            let cfg = SearchConfig {
                k,
                tsb: true,
                max_nodes: HUGE_NODES,
                ..SearchConfig::default()
            };
            let out = self.s.search(self.trie, self.cm, q, &cfg).expect("search");
            self.truncated_expand += usize::from(out.stats.truncated);
            if out.hits.len() < k || units(out.hits.last().unwrap()) > u10 {
                return (top, out.hits);
            }
            k *= 2;
        }
    }

    /// Independent route: `Ranking::Exact`, all terms within budget, sorted by
    /// the variant key with the term id last.
    fn variant_by_exact(&mut self, q: &[u8]) -> Vec<u32> {
        let cfg = SearchConfig {
            k: 1 << 20,
            tsb: true,
            ranking: Ranking::Exact,
            max_nodes: HUGE_NODES,
            ..SearchConfig::default()
        };
        let out = self.s.search(self.trie, self.cm, q, &cfg).expect("search");
        assert!(out.hits.len() < 1 << 20 && !out.stats.truncated);
        let mut v = out.hits;
        v.sort_by_key(|h| (vkey(h), h.id));
        v.iter().take(TOP).map(|h| h.id).collect()
    }
}

/// Everything measured for one query at one dictionary size.
struct Q {
    corpus: usize,
    cats: Vec<&'static str>,
    rr_d: f64,
    rr_v: f64,
    in_dict: bool,
    weight0_correct: bool,
    t1: bool,
    t1_w0: bool,
    t1_wpos: bool,
    t2: bool,
    t2_w0: bool,
    t3: bool,
    t1_variant: bool,
}

fn rr(hits: &[Hit], target: Option<u32>) -> f64 {
    match target.and_then(|t| hits.iter().position(|h| h.id == t)) {
        Some(p) => 1.0 / (p as f64 + 1.0),
        None => 0.0,
    }
}

fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len() as f64
}

/// Paired difference `a - b`: mean, SE, low, high.
fn paired(a: &[f64], b: &[f64]) -> (f64, f64, f64, f64) {
    let d: Vec<f64> = a.iter().zip(b).map(|(x, y)| x - y).collect();
    let n = d.len() as f64;
    let m = mean(&d);
    let se = if d.len() > 1 {
        (d.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / (n - 1.0) / n).sqrt()
    } else {
        f64::NAN
    };
    (m, se, m - Z95 * se, m + Z95 * se)
}

fn diff_of(qs: &[&Q]) -> (usize, f64, f64, f64, f64, usize) {
    let a: Vec<f64> = qs.iter().map(|q| q.rr_v).collect();
    let b: Vec<f64> = qs.iter().map(|q| q.rr_d).collect();
    let (m, se, lo, hi) = paired(&a, &b);
    let differ = qs.iter().filter(|q| q.rr_v != q.rr_d).count();
    (qs.len(), m, se, lo, hi, differ)
}

fn pct(x: usize, n: usize) -> String {
    if n == 0 {
        "-".into()
    } else {
        format!("{:.1}% ({x})", 100.0 * x as f64 / n as f64)
    }
}

fn main() {
    let dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "bench/data".into());
    let cm = CostModel::qwerty();
    let mut pairs: Vec<(usize, String, String)> = Vec::new();
    for (ci, name) in CORPORA.iter().enumerate() {
        for (t, c) in read_tsv(&format!("{dir}/{name}.tsv")) {
            pairs.push((ci, t, c));
        }
    }
    println!(
        "pairs: {} = {}",
        pairs.len(),
        CORPORA
            .iter()
            .enumerate()
            .map(|(i, n)| format!("{} {}", n, pairs.iter().filter(|p| p.0 == i).count()))
            .collect::<Vec<_>>()
            .join(" + ")
    );
    let cats: Vec<Vec<&'static str>> = pairs.iter().map(|(_, t, c)| classify(t, c, &cm)).collect();

    let mut final_rule: Option<[String; 4]> = None;
    for size in ["10000", "100000", "full"] {
        let words: Vec<(String, u16)> = read_tsv(&format!("{dir}/words-{size}.tsv"))
            .into_iter()
            .map(|(w, f)| (w, f.parse().unwrap_or_else(|_| die("bad weight"))))
            .collect();
        let wmap: HashMap<&str, u16> = words.iter().map(|(w, f)| (w.as_str(), *f)).collect();
        let items: Vec<(&str, u16)> = words.iter().map(|(w, f)| (w.as_str(), *f)).collect();
        let trie = Trie::build(&items).expect("trie");
        let mut se = Searchers {
            trie: &trie,
            cm: &cm,
            s: Searcher::new(),
            truncated_default: 0,
            truncated_expand: 0,
        };
        let idmap: HashMap<String, u32> = (0..words.len() as u32)
            .map(|i| (trie.term(i).to_string(), i))
            .collect();
        println!("\n## dictionary: {} words", words.len());
        let mut qs: Vec<Q> = Vec::new();
        let mut mismatches = 0usize;
        for (i, (ci, typo, right)) in pairs.iter().enumerate() {
            let target = idmap.get(right).copied();
            let (top, all) = se.run(typo.as_bytes());
            // variant: reorder the complete set of terms with units <= u10
            let mut v = all.clone();
            v.sort_by_key(|h| (vkey(h), h.id));
            v.truncate(TOP);
            // self-check against the independent Ranking::Exact route
            let ids: Vec<u32> = v.iter().map(|h| h.id).collect();
            if ids != se.variant_by_exact(typo.as_bytes()) {
                mismatches += 1;
            }
            let keys: Vec<(u16, u16)> = top.iter().map(key).collect();
            let vkeys: Vec<_> = v.iter().map(vkey).collect();
            let in_top = target.is_some_and(|t| top.iter().any(|h| h.id == t));
            let tied_hits: Vec<&Hit> = top
                .iter()
                .filter(|h| keys.iter().filter(|k| **k == key(h)).count() > 1)
                .collect();
            let t2_hit = target
                .filter(|_| in_top)
                .and_then(|t| top.iter().find(|h| h.id == t))
                .filter(|h| keys.iter().filter(|k| **k == key(h)).count() > 1);
            // T3: correct word absent from the top 10 but with the same key as
            // the 10th hit (cut off by the id tie-break at the boundary).
            let t3 = !in_top
                && top.len() == TOP
                && target
                    .and_then(|t| all.iter().find(|h| h.id == t))
                    .is_some_and(|h| key(h) == key(&top[TOP - 1]));
            qs.push(Q {
                corpus: *ci,
                cats: cats[i].clone(),
                rr_d: rr(&top, target),
                rr_v: rr(&v, target),
                in_dict: target.is_some(),
                weight0_correct: wmap.get(right.as_str()) == Some(&0),
                t1: has_dup(&keys),
                t1_w0: tied_hits.iter().any(|h| h.weight == 0),
                t1_wpos: tied_hits.iter().any(|h| h.weight > 0),
                t2: t2_hit.is_some(),
                t2_w0: t2_hit.is_some_and(|h| h.weight == 0),
                t3,
                t1_variant: has_dup(&vkeys),
            });
        }
        println!(
            "self-check (variant vs Ranking::Exact route, every query): {mismatches} mismatches; searches truncated at max_nodes: default k=10 {}, expansion {}",
            se.truncated_default, se.truncated_expand
        );
        if mismatches > 0 {
            die("self-check failed");
        }

        // primary
        println!("\nPrimary: MRR@10 of variant minus default (paired)");
        println!(
            "| sample | n | MRR default | MRR variant | diff | SE | 95% interval | queries that differ |"
        );
        println!("|---|---|---|---|---|---|---|---|");
        let groups: [(&str, Vec<&Q>); 3] = [
            ("pooled", qs.iter().collect()),
            (CORPORA[0], qs.iter().filter(|q| q.corpus == 0).collect()),
            (CORPORA[1], qs.iter().filter(|q| q.corpus == 1).collect()),
        ];
        let mut prim = Vec::new();
        for (name, g) in &groups {
            let (n, m, s, lo, hi, differ) = diff_of(g);
            let md = mean(&g.iter().map(|q| q.rr_d).collect::<Vec<_>>());
            let mv = mean(&g.iter().map(|q| q.rr_v).collect::<Vec<_>>());
            println!(
                "| {name} | {n} | {md:.4} | {mv:.4} | {m:+.4} | {s:.4} | [{lo:+.4}, {hi:+.4}] | {differ} |"
            );
            prim.push((m, s, lo, hi));
        }
        println!(
            "correct word not in this dictionary: {} of {} queries (score 0 for both)",
            qs.iter().filter(|q| !q.in_dict).count(),
            qs.len()
        );

        // categories (pooled)
        println!("\nCategories (pooled): diff variant - default");
        println!(
            "| category | n | diff | SE | 95% interval | significant loss | in rule (n >= {MIN_CATEGORY}) |"
        );
        println!("|---|---|---|---|---|---|---|");
        let mut cat_names: Vec<&str> = CATEGORY_ORDER.to_vec();
        cat_names.push("first letter differs");
        let mut cat_loss = Vec::new();
        for c in &cat_names {
            let g: Vec<&Q> = qs.iter().filter(|q| q.cats.contains(c)).collect();
            if g.is_empty() {
                continue;
            }
            let (n, m, s, lo, hi, _) = diff_of(&g);
            let loss = hi < 0.0;
            let inrule = n >= MIN_CATEGORY;
            if loss && inrule {
                cat_loss.push(*c);
            }
            println!(
                "| {c} | {n} | {m:+.4} | {s:.4} | [{lo:+.4}, {hi:+.4}] | {} | {} |",
                if loss { "yes" } else { "no" },
                if inrule { "yes" } else { "no" }
            );
        }

        // ties
        println!(
            "\nTies under the default ranking, top 10 (key = whole units, weight; equal keys are ordered by term id only)"
        );
        println!(
            "| sample | n | T1 any id-only tie | T1 with a weight-0 tie | T1 with a weight>0 tie | T2 correct in a tie | T2 correct in a weight-0 tie | T3 correct cut off by the tie at rank 10 | correct word has weight 0 | T1 left by the variant (exact cost also equal) |"
        );
        println!("|---|---|---|---|---|---|---|---|---|---|");
        for (name, g) in &groups {
            let n = g.len();
            let c = |f: &dyn Fn(&Q) -> bool| g.iter().filter(|q| f(q)).count();
            println!(
                "| {name} | {n} | {} | {} | {} | {} | {} | {} | {} | {} |",
                pct(c(&|q| q.t1), n),
                pct(c(&|q| q.t1_w0), n),
                pct(c(&|q| q.t1_wpos), n),
                pct(c(&|q| q.t2), n),
                pct(c(&|q| q.t2_w0), n),
                pct(c(&|q| q.t3), n),
                pct(c(&|q| q.weight0_correct), n),
                pct(c(&|q| q.t1_variant), n),
            );
        }
        let all: Vec<&Q> = qs.iter().collect();
        for (label, f) in [
            ("T2 queries", (&|q: &Q| q.t2) as &dyn Fn(&Q) -> bool),
            ("T3 queries", &|q: &Q| q.t3),
        ] {
            let g: Vec<&Q> = all.iter().copied().filter(|q| f(q)).collect();
            if g.len() > 1 {
                let (n, m, s, lo, hi, differ) = diff_of(&g);
                println!(
                    "MRR diff on {label} (pooled): n={n} diff={m:+.4} SE={s:.4} [{lo:+.4}, {hi:+.4}] differ={differ}"
                );
            } else {
                println!("MRR diff on {label} (pooled): n={}", g.len());
            }
        }

        if size == "full" {
            let cond1 = prim[0].2 > 0.0;
            let cond2 = cat_loss.is_empty();
            let cond3 = prim[1].3 >= 0.0 && prim[2].3 >= 0.0;
            let verdict = if cond1 && cond2 && cond3 {
                "ADOPT"
            } else if !cond2 || !cond3 || prim[0].3 < INCONCLUSIVE_UPPER {
                "DO NOT ADOPT"
            } else {
                "INCONCLUSIVE"
            };
            final_rule = Some([
                format!(
                    "1. pooled interval lower end > 0: {} (diff {:+.4}, SE {:.4}, [{:+.4}, {:+.4}])",
                    if cond1 { "PASS" } else { "FAIL" },
                    prim[0].0,
                    prim[0].1,
                    prim[0].2,
                    prim[0].3
                ),
                format!(
                    "2. no category with n >= {MIN_CATEGORY} significantly negative: {} {:?}",
                    if cond2 { "PASS" } else { "FAIL" },
                    cat_loss
                ),
                format!(
                    "3. neither corpus significantly negative: {} (upper ends {:+.4}, {:+.4})",
                    if cond3 { "PASS" } else { "FAIL" },
                    prim[1].3,
                    prim[2].3
                ),
                format!("verdict: {verdict}"),
            ]);
        }
    }
    println!("\n## Decision rule at 274137 words");
    for l in final_rule.expect("full dictionary ran") {
        println!("{l}");
    }
}
