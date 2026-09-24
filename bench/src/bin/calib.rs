// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

//! Pre-registered calibration of the cost model (issue #21).
//!
//! Protocol: `docs/benchmarks/calibration-preregistration.md`, committed before
//! this harness was run. Quality only (MRR@10 and deterministic node counts),
//! no latency.
//!
//! The cost parameters are private in `CostModel`, so this binary needs the
//! experiment-only patch `bench/experiments/cost-knobs.patch` (it adds
//! `CostParams` and `CostModel::qwerty_with`; core is not changed in the
//! repository). It is built only with `--features calibration`:
//!
//! ```sh
//! git apply bench/experiments/cost-knobs.patch
//! cargo run --release -p keyhammer-bench --features calibration --bin calib -- bench/data
//! git checkout crates/keyhammer/src/cost.rs
//! ```

use std::collections::BTreeMap;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

use keyhammer::cost::{COST_UNIT, CostModel, CostParams, whole_units};
use keyhammer::search::{Hit, Ranking, SearchConfig, Searcher};
use keyhammer::trie::Trie;

const TOP: usize = 10;
const Z95: f64 = 1.96;
const SHORTLIST: usize = 5;
const MIN_CATEGORY: usize = 50;
const INCONCLUSIVE_UPPER: f64 = 0.005;
const SALT: &str = "keyhammer-calibration-v1:";
const BUDGET: u16 = 32;

// Caps (pairs per corpus and role), fixed in the protocol.
const CAP_BIRK: [usize; 3] = [2000, 1500, 2000];
const CAP_SLIPS: [usize; 3] = [3000, 2000, 3000];

fn die(msg: &str) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(2);
}

// ---------------------------------------------------------------- data

#[derive(Clone)]
struct Pair {
    typo: String,
    right: String,
}

struct Corpus {
    name: &'static str,
    pairs: Vec<Pair>,
}

fn splitmix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

fn hash(s: &str) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in SALT.bytes().chain(s.bytes()) {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    splitmix(h)
}

/// Bucket 0..99 of the intended word: all pairs of a word share a role.
fn bucket(word: &str) -> u64 {
    hash(word) % 100
}

fn read_tsv2(path: &str) -> Vec<Pair> {
    let text =
        fs::read_to_string(path).unwrap_or_else(|e| die(&format!("cannot read {path}: {e}")));
    text.lines()
        .filter(|l| !l.is_empty())
        .map(|l| match l.split_once('\t') {
            Some((a, b)) => Pair {
                typo: a.to_string(),
                right: b.split('\t').next().unwrap_or("").to_string(),
            },
            None => die(&format!("{path}: line without a tab: {l:?}")),
        })
        .collect()
}

fn read_slips(path: &str) -> Vec<Pair> {
    let text = fs::read_to_string(path).unwrap_or_else(|e| {
        die(&format!(
            "cannot read {path}: {e} (run fetch-finger-slips.mjs)"
        ))
    });
    text.lines()
        .skip(1)
        .filter(|l| !l.is_empty())
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            if f.len() != 6 {
                die(&format!("{path}: expected 6 columns"));
            }
            Pair {
                typo: f[0].to_string(),
                right: f[1].to_string(),
            }
        })
        .collect()
}

fn read_words(path: &str) -> Vec<(String, u16)> {
    let text =
        fs::read_to_string(path).unwrap_or_else(|e| die(&format!("cannot read {path}: {e}")));
    text.lines()
        .filter(|l| !l.is_empty())
        .map(
            |l| match l.split_once('\t').map(|(w, f)| (w, f.parse::<u16>())) {
                Some((w, Ok(f))) => (w.to_string(), f),
                _ => die(&format!("{path}: expected `word TAB u16`")),
            },
        )
        .collect()
}

/// Birkbeck pairs with the same filter as `prepare-m0-data.mjs` and
/// `compare.mjs`: `$correct` header lines, misspellings of at least 3 letters,
/// correct word in the dictionary, typo not in it, typo different from the word.
fn read_birkbeck(path: &str, dict: &std::collections::HashSet<&str>) -> Vec<Pair> {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|e| die(&format!("cannot read {path}: {e} (run fetch-data.mjs)")));
    let all_lower =
        |s: &str, min: usize| s.len() >= min && s.bytes().all(|b| b.is_ascii_alphabetic());
    let mut cur: Option<String> = None;
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(c) = line.strip_prefix('$') {
            cur = Some(c.to_ascii_lowercase());
        } else if let Some(c) = &cur {
            if all_lower(line, 3) {
                let t = line.to_ascii_lowercase();
                if all_lower(c, 3)
                    && dict.contains(c.as_str())
                    && !dict.contains(t.as_str())
                    && t != *c
                    && seen.insert((t.clone(), c.clone()))
                {
                    out.push(Pair {
                        typo: t,
                        right: c.clone(),
                    });
                }
            }
        }
    }
    out
}

/// Keeps the pairs whose word bucket is in `lo..hi`, ordered by the hash of the
/// pair, first `cap` of them.
fn take(pairs: &[Pair], lo: u64, hi: u64, cap: usize) -> Vec<Pair> {
    let mut v: Vec<&Pair> = pairs
        .iter()
        .filter(|p| (lo..hi).contains(&bucket(&p.right)))
        .collect();
    v.sort_by_key(|p| hash(&format!("{}\t{}", p.typo, p.right)));
    v.truncate(cap);
    v.into_iter().cloned().collect()
}

struct Data {
    train: Vec<Corpus>,
    val: Vec<Corpus>,
    test: Vec<Corpus>,
    slips_sample: Vec<Pair>,
}

fn load(dir: &str, dict: &std::collections::HashSet<&str>) -> Data {
    let birk = read_birkbeck(&format!("{dir}/missp.dat"), dict);
    let slips_all = read_slips(&format!("{dir}/slips.tsv"));
    let slips_sample = read_slips(&format!("{dir}/slips-sample.tsv"));
    let seen: std::collections::HashSet<(&str, &str)> = slips_sample
        .iter()
        .map(|p| (p.typo.as_str(), p.right.as_str()))
        .collect();
    let slips: Vec<Pair> = slips_all
        .into_iter()
        .filter(|p| !seen.contains(&(p.typo.as_str(), p.right.as_str())))
        .collect();
    let gtc = read_tsv2(&format!("{dir}/gtc.tsv"));
    let wiki = read_tsv2(&format!("{dir}/wiki.tsv"));
    let mut d = Data {
        train: Vec::new(),
        val: Vec::new(),
        test: Vec::new(),
        slips_sample: Vec::new(),
    };
    d.train.push(Corpus {
        name: "Birkbeck",
        pairs: take(&birk, 0, 50, CAP_BIRK[0]),
    });
    d.train.push(Corpus {
        name: "finger slips",
        pairs: take(&slips, 0, 50, CAP_SLIPS[0]),
    });
    d.train.push(Corpus {
        name: "GitHub Typo Corpus",
        pairs: take(&gtc, 0, 60, usize::MAX),
    });
    d.train.push(Corpus {
        name: "Wikipedia",
        pairs: take(&wiki, 0, 60, usize::MAX),
    });
    d.val.push(Corpus {
        name: "Birkbeck",
        pairs: take(&birk, 50, 70, CAP_BIRK[1]),
    });
    d.val.push(Corpus {
        name: "finger slips",
        pairs: take(&slips, 50, 70, CAP_SLIPS[1]),
    });
    d.val.push(Corpus {
        name: "GitHub Typo Corpus",
        pairs: take(&gtc, 60, 100, usize::MAX),
    });
    d.val.push(Corpus {
        name: "Wikipedia",
        pairs: take(&wiki, 60, 100, usize::MAX),
    });
    d.test.push(Corpus {
        name: "Birkbeck",
        pairs: take(&birk, 70, 100, CAP_BIRK[2]),
    });
    d.test.push(Corpus {
        name: "finger slips",
        pairs: take(&slips, 70, 100, CAP_SLIPS[2]),
    });
    d.test.push(Corpus {
        name: "GitHub Typo Corpus",
        pairs: read_tsv2(&format!("{dir}/gtc-holdout.tsv")),
    });
    d.test.push(Corpus {
        name: "Wikipedia",
        pairs: read_tsv2(&format!("{dir}/wiki-holdout.tsv")),
    });
    d.slips_sample = slips_sample;
    d
}

// ---------------------------------------------------------------- model grid

#[derive(Clone, Copy, PartialEq)]
struct Cfg {
    p: CostParams,
    /// The first-byte factor is applied after rounding (see the protocol).
    after: bool,
}

impl Cfg {
    fn name(&self) -> String {
        format!(
            "f{}{} t{} a{} i{}/{}",
            match self.p.first_quarters {
                4 => "1.0",
                5 => "1.25",
                _ => "1.5",
            },
            if self.after { "A" } else { "B" },
            self.p.transpose,
            self.p.sub_adjacent,
            self.p.indel,
            self.p.indel_double
        )
    }
    /// Number of parameters that differ from the shipped model.
    fn changes(&self) -> usize {
        let s = CostParams::SHIPPED;
        usize::from(self.p.first_quarters != s.first_quarters)
            + usize::from(self.after)
            + usize::from(self.p.transpose != s.transpose)
            + usize::from(self.p.sub_adjacent != s.sub_adjacent)
            + usize::from(self.p.indel != s.indel)
            + usize::from(self.p.indel_double != s.indel_double)
    }
}

fn shipped() -> Cfg {
    Cfg {
        p: CostParams::SHIPPED,
        after: false,
    }
}

fn grid() -> Vec<Cfg> {
    let mut v = Vec::new();
    for (fq, after) in [(4, false), (5, false), (5, true), (6, false), (6, true)] {
        for transpose in [8, 12, 16] {
            for sub_adjacent in [4, 8, 12, 16] {
                for (indel, indel_double) in [(16, 8), (16, 12), (16, 16), (12, 8), (12, 12)] {
                    v.push(Cfg {
                        p: CostParams {
                            sub: 16,
                            sub_adjacent,
                            indel,
                            indel_double,
                            transpose,
                            first_quarters: fq,
                        },
                        after,
                    });
                }
            }
        }
    }
    v
}

// ---------------------------------------------------------------- evaluation

/// Exact weighted OSA cost of `q` against `t` (the same recurrence as the
/// oracle in the core tests).
fn oracle(cm: &CostModel, q: &[u8], t: &[u8]) -> u32 {
    const BIG: u32 = 1_000_000;
    let (m, n) = (q.len(), t.len());
    let mut d = vec![vec![BIG; m + 1]; n + 1];
    d[0][0] = 0;
    for j in 0..=n {
        for i in 0..=m {
            let mut best = d[j][i];
            if i >= 1 && j >= 1 {
                best =
                    best.min(d[j - 1][i - 1] + u32::from(cm.sub_cost(q[i - 1], t[j - 1], i - 1)));
            }
            if j >= 1 {
                let prev_t = if j >= 2 { Some(t[j - 2]) } else { None };
                best = best.min(d[j - 1][i] + u32::from(cm.ins_cost(t[j - 1], prev_t, i)));
            }
            if i >= 1 {
                best = best.min(d[j][i - 1] + u32::from(cm.del_cost(q, i - 1)));
            }
            if i >= 2
                && j >= 2
                && q[i - 1] == t[j - 2]
                && q[i - 2] == t[j - 1]
                && q[i - 1] != q[i - 2]
            {
                best = best.min(d[j - 2][i - 2] + u32::from(cm.transpose_cost(i - 2)));
            }
            d[j][i] = best;
        }
    }
    d[n][m]
}

struct Model {
    cfg: Cfg,
    cm: CostModel,
    /// Same costs with the first-byte factor 1.0 (used by the "after" ranking).
    cm1: CostModel,
}

impl Model {
    fn new(cfg: Cfg) -> Self {
        let mut p1 = cfg.p;
        p1.first_quarters = 4;
        Model {
            cfg,
            cm: CostModel::qwerty_with(cfg.p),
            cm1: CostModel::qwerty_with(p1),
        }
    }
}

#[derive(Default, Clone, Copy)]
struct Counters {
    nodes: u64,
    truncated: u64,
    incomplete: u64,
    queries: u64,
}

/// Top-10 hits under the model's ranking. "Before": the core's `Coarse`
/// ranking. "After": the key is `whole_units(cost at factor 1.0) * 16 +
/// (exact cost - cost at factor 1.0)`, then higher weight, then lower id,
/// computed here from the `Exact` list (the list is extended until every term
/// that can reach the top 10 is in it).
fn top10(s: &mut Searcher, trie: &Trie, m: &Model, q: &[u8], c: &mut Counters) -> Vec<Hit> {
    c.queries += 1;
    if !m.cfg.after || m.cfg.p.first_quarters == 4 {
        let cfg = SearchConfig {
            k: TOP,
            budget: BUDGET,
            tsb: true,
            ..SearchConfig::default()
        };
        let out = s.search(trie, &m.cm, q, &cfg).expect("search");
        c.nodes += out.stats.nodes_expanded as u64;
        c.truncated += u64::from(out.stats.truncated);
        return out.hits;
    }
    let mut k = 64;
    loop {
        let cfg = SearchConfig {
            k,
            budget: BUDGET,
            tsb: true,
            ranking: Ranking::Exact,
            ..SearchConfig::default()
        };
        let out = s.search(trie, &m.cm, q, &cfg).expect("search");
        let mut keyed: Vec<(u32, std::cmp::Reverse<u16>, u32, Hit)> = out
            .hits
            .iter()
            .map(|h| {
                let c1 = oracle(&m.cm1, q, trie.term(h.id).as_bytes());
                let cf = u32::from(h.cost);
                assert!(c1 <= cf, "cost at factor 1.0 above the exact cost");
                let key = u32::from(whole_units(c1 as u16)) * u32::from(COST_UNIT) + (cf - c1);
                (key, std::cmp::Reverse(h.weight), h.id, *h)
            })
            .collect();
        keyed.sort_by_key(|x| (x.0, x.1, x.2));
        let complete = out.hits.len() < k
            || (keyed.len() >= TOP
                && u32::from(out.hits.last().expect("hits").cost) > keyed[TOP - 1].0);
        if complete || k >= 4096 {
            c.nodes += out.stats.nodes_expanded as u64;
            c.truncated += u64::from(out.stats.truncated);
            c.incomplete += u64::from(!complete);
            return keyed.into_iter().take(TOP).map(|x| x.3).collect();
        }
        k *= 2;
    }
}

fn rr(trie: &Trie, hits: &[Hit], right: &str) -> f64 {
    hits.iter()
        .position(|h| trie.term(h.id) == right)
        .map_or(0.0, |r| 1.0 / (r as f64 + 1.0))
}

/// Reciprocal ranks of every pair of a corpus under the model.
fn eval(trie: &Trie, m: &Model, pairs: &[Pair], c: &mut Counters) -> Vec<f64> {
    let mut s = Searcher::new();
    pairs
        .iter()
        .map(|p| {
            let hits = top10(&mut s, trie, m, p.typo.as_bytes(), c);
            rr(trie, &hits, &p.right)
        })
        .collect()
}

fn eval_split(trie: &Trie, m: &Model, split: &[Corpus]) -> (Vec<Vec<f64>>, Counters) {
    let mut c = Counters::default();
    let v = split
        .iter()
        .map(|k| eval(trie, m, &k.pairs, &mut c))
        .collect();
    (v, c)
}

/// Runs `f` over `0..n` on `threads` threads, results in index order.
fn par_map<T: Send, F: Fn(usize) -> T + Sync>(n: usize, threads: usize, f: F) -> Vec<T> {
    let next = AtomicUsize::new(0);
    let mut out: Vec<Option<T>> = (0..n).map(|_| None).collect();
    let parts: Vec<Vec<(usize, T)>> = std::thread::scope(|sc| {
        let hs: Vec<_> = (0..threads.max(1))
            .map(|_| {
                sc.spawn(|| {
                    let mut mine = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        if i >= n {
                            break;
                        }
                        mine.push((i, f(i)));
                    }
                    mine
                })
            })
            .collect();
        hs.into_iter().map(|h| h.join().expect("thread")).collect()
    });
    for part in parts {
        for (i, t) in part {
            out[i] = Some(t);
        }
    }
    out.into_iter().map(|x| x.expect("result")).collect()
}

// ---------------------------------------------------------------- statistics

fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len() as f64
}

/// Paired difference `a - b`: (mean, SE).
fn paired(a: &[f64], b: &[f64]) -> (f64, f64) {
    let d: Vec<f64> = a.iter().zip(b).map(|(x, y)| x - y).collect();
    let n = d.len();
    if n < 2 {
        return (mean(&d), f64::NAN);
    }
    let m = mean(&d);
    let var = d.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / (n as f64 - 1.0);
    (m, (var / n as f64).sqrt())
}

fn paired_idx(a: &[f64], b: &[f64], idx: &[usize]) -> (f64, f64) {
    let aa: Vec<f64> = idx.iter().map(|&i| a[i]).collect();
    let bb: Vec<f64> = idx.iter().map(|&i| b[i]).collect();
    paired(&aa, &bb)
}

/// Per-corpus paired differences (candidate - reference) and their macro average.
struct Diff {
    per: Vec<(f64, f64)>,
    macro_d: f64,
    macro_se: f64,
}

fn diff(a: &[Vec<f64>], b: &[Vec<f64>]) -> Diff {
    let per: Vec<(f64, f64)> = a.iter().zip(b).map(|(x, y)| paired(x, y)).collect();
    let c = per.len() as f64;
    let macro_d = per.iter().map(|p| p.0).sum::<f64>() / c;
    let macro_se = per.iter().map(|p| p.1 * p.1).sum::<f64>().sqrt() / c;
    Diff {
        per,
        macro_d,
        macro_se,
    }
}

impl Diff {
    fn lb(&self) -> f64 {
        self.macro_d - Z95 * self.macro_se
    }
    fn ub(&self) -> f64 {
        self.macro_d + Z95 * self.macro_se
    }
    fn vetoed(&self) -> bool {
        self.per.iter().any(|&(d, se)| d + Z95 * se < 0.0)
    }
}

fn macro_mrr(rrs: &[Vec<f64>]) -> f64 {
    rrs.iter().map(|v| mean(v)).sum::<f64>() / rrs.len() as f64
}

// ---------------------------------------------------------------- categories

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
        let (_, dbl) = removal(c, t);
        cats.push(if dbl {
            "missing doubled letter"
        } else {
            "missing letter"
        });
    } else {
        let (_, dbl) = removal(t, c);
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

// ---------------------------------------------------------------- baseline

fn osa_within(a: &[u8], b: &[u8], k: usize) -> Option<usize> {
    if a.len().abs_diff(b.len()) > k {
        return None;
    }
    let d = osa(a, b);
    (d <= k).then_some(d)
}

/// Unit-cost baseline (reference only): OSA distance at most 2, then higher
/// weight, then lower id, over a full scan; ids are positions in `words`.
fn baseline_rr(words: &[(String, u16)], p: &Pair) -> f64 {
    let mut cand: Vec<(usize, u32, usize)> = Vec::new();
    for (id, (w, f)) in words.iter().enumerate() {
        if let Some(d) = osa_within(p.typo.as_bytes(), w.as_bytes(), 2) {
            cand.push((d, 65_535 - u32::from(*f), id));
        }
    }
    cand.sort();
    cand.truncate(TOP);
    cand.iter()
        .position(|c| words[c.2].0 == p.right)
        .map_or(0.0, |r| 1.0 / (r as f64 + 1.0))
}

// ---------------------------------------------------------------- report

fn fmt_d(d: f64, se: f64) -> String {
    format!("{d:+.4} ({se:.4})")
}

fn print_diff_row(label: &str, d: &Diff) {
    let cells: Vec<String> = d.per.iter().map(|&(x, s)| fmt_d(x, s)).collect();
    println!(
        "| {label} | {} | {} | [{:+.4}, {:+.4}] |",
        cells.join(" | "),
        fmt_d(d.macro_d, d.macro_se),
        d.lb(),
        d.ub()
    );
}

fn header(split: &[Corpus]) {
    let names: Vec<&str> = split.iter().map(|c| c.name).collect();
    println!(
        "| model | {} | macro | 95% interval |\n|---|{}---|---|",
        names.join(" | "),
        "---|".repeat(names.len())
    );
}

fn sizes(label: &str, split: &[Corpus]) {
    let s: Vec<String> = split
        .iter()
        .map(|c| format!("{} {}", c.name, c.pairs.len()))
        .collect();
    println!(
        "{label}: {} (total {})",
        s.join(", "),
        split.iter().map(|c| c.pairs.len()).sum::<usize>()
    );
}

const HELP: &str = "\
Pre-registered calibration of the cost model (issue #21). Needs the patch
bench/experiments/cost-knobs.patch applied to crates/keyhammer/src/cost.rs.

USAGE:
    calib [DATA_DIR] [--threads N] [--counts] [--selfcheck]

    --counts     print the size of every split and stop (no model is run)
    --selfcheck  run internal consistency checks on 300 train queries and stop
";

fn main() {
    let mut dir = "bench/data".to_string();
    let mut threads = 6usize;
    let (mut counts_only, mut selfcheck) = (false, false);
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => {
                print!("{HELP}");
                return;
            }
            "--threads" => {
                threads = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or_else(|| die("--threads N"));
            }
            "--counts" => counts_only = true,
            "--selfcheck" => selfcheck = true,
            _ if a.starts_with('-') => die(&format!("unknown option {a:?}")),
            _ => dir = a,
        }
    }
    let words = read_words(&format!("{dir}/words-full.tsv"));
    let dict: std::collections::HashSet<&str> = words.iter().map(|(w, _)| w.as_str()).collect();
    let data = load(&dir, &dict);
    println!("dictionary {} words", words.len());
    sizes("train", &data.train);
    sizes("validation", &data.val);
    sizes("test", &data.test);
    println!("slips-sample (reference only) {}", data.slips_sample.len());
    if counts_only {
        return;
    }
    let items: Vec<(&str, u16)> = words.iter().map(|(w, f)| (w.as_str(), *f)).collect();
    let trie = Trie::build(&items).expect("trie");

    if selfcheck {
        run_selfcheck(&trie, &data);
        return;
    }

    let grid = grid();
    println!("\ngrid: {} models\n", grid.len());
    let shipped_cfg = shipped();

    // ---- phase 1: train
    let ship = Model::new(shipped_cfg);
    let (ship_tr, _) = eval_split(&trie, &ship, &data.train);
    let results: Vec<(Vec<Vec<f64>>, Counters)> = par_map(grid.len(), threads, |i| {
        eval_split(&trie, &Model::new(grid[i]), &data.train)
    });
    let mut scored: Vec<(usize, Diff)> = results
        .iter()
        .enumerate()
        .map(|(i, (r, _))| (i, diff(r, &ship_tr)))
        .collect();
    let mut tsv = String::from("model\tmacro_diff\tmacro_se\tincomplete\ttruncated\n");
    for (i, d) in &scored {
        tsv.push_str(&format!(
            "{}\t{:+.5}\t{:.5}\t{}\t{}\n",
            grid[*i].name(),
            d.macro_d,
            d.macro_se,
            results[*i].1.incomplete,
            results[*i].1.truncated
        ));
    }
    let _ = fs::write(format!("{dir}/calib-train-grid.tsv"), tsv);
    // order: highest train macro difference, then fewer changes, then grid order
    scored.sort_by(|a, b| {
        b.1.macro_d
            .partial_cmp(&a.1.macro_d)
            .expect("finite")
            .then(grid[a.0].changes().cmp(&grid[b.0].changes()))
            .then(a.0.cmp(&b.0))
    });
    println!("### Phase 1: train (macro MRR@10 difference to the shipped model)\n");
    println!(
        "shipped model on train: macro MRR@10 {:.4}\n",
        macro_mrr(&ship_tr)
    );
    header(&data.train);
    for (i, d) in scored.iter().take(10) {
        print_diff_row(&grid[*i].name(), d);
    }
    println!();
    let neg = scored.iter().filter(|(_, d)| d.macro_d < 0.0).count();
    let pos = scored.iter().filter(|(_, d)| d.macro_d > 0.0).count();
    println!(
        "{} of {} models above the shipped one on train, {} below, {} equal; all rows in calib-train-grid.tsv\n",
        pos,
        grid.len(),
        neg,
        grid.len() - pos - neg
    );

    // ---- shortlist
    let mut short: Vec<(String, Cfg)> = scored
        .iter()
        .take(SHORTLIST)
        .map(|(i, _)| (format!("train rank {}", 0), grid[*i]))
        .collect();
    for (k, s) in short.iter_mut().enumerate() {
        s.0 = format!("train rank {}", k + 1);
    }
    let mut f1 = shipped_cfg;
    f1.p.first_quarters = 4;
    let mut fa = shipped_cfg;
    fa.after = true;
    for (label, c) in [
        ("reference F1 (factor 1.0)", f1),
        ("reference FA (1.5 after rounding)", fa),
    ] {
        if !short.iter().any(|(_, s)| *s == c) {
            short.push((label.to_string(), c));
        } else {
            for s in short.iter_mut().filter(|(_, s)| *s == c) {
                s.0 = format!("{} = {}", s.0, label);
            }
        }
    }

    // ---- phase 2: validation
    let (ship_va, _) = eval_split(&trie, &ship, &data.val);
    let va: Vec<(Vec<Vec<f64>>, Counters)> = par_map(short.len(), threads, |i| {
        eval_split(&trie, &Model::new(short[i].1), &data.val)
    });
    println!("### Phase 2: validation\n");
    println!(
        "shipped model on validation: macro MRR@10 {:.4}\n",
        macro_mrr(&ship_va)
    );
    header(&data.val);
    let mut val_d: Vec<Diff> = Vec::new();
    for (k, (label, cfg)) in short.iter().enumerate() {
        let d = diff(&va[k].0, &ship_va);
        print_diff_row(&format!("{} ({})", cfg.name(), label), &d);
        val_d.push(d);
    }
    println!();
    // qualifying: lower bound above 0 and no corpus with a significant loss
    let mut best: Option<usize> = None;
    for (k, d) in val_d.iter().enumerate() {
        let ok = d.lb() > 0.0 && !d.vetoed();
        println!(
            "- {} ({}): lower bound {:+.4}, vetoed by a corpus loss: {}, qualifies: {}",
            short[k].1.name(),
            short[k].0,
            d.lb(),
            d.vetoed(),
            ok
        );
        if ok {
            let better = match best {
                None => true,
                Some(b) => {
                    d.macro_d > val_d[b].macro_d
                        || (d.macro_d == val_d[b].macro_d
                            && short[k].1.changes() < short[b].1.changes())
                }
            };
            if better {
                best = Some(k);
            }
        }
    }
    let by_val = (0..short.len())
        .max_by(|&a, &b| {
            val_d[a]
                .macro_d
                .partial_cmp(&val_d[b].macro_d)
                .expect("finite")
        })
        .expect("shortlist");
    let selected = best.map(|k| short[k].1);
    match &selected {
        Some(c) => println!("\nSelected: {}\n", c.name()),
        None => println!(
            "\nSelected: none qualifies, the shipped model stays (highest validation macro difference was {})\n",
            short[by_val].1.name()
        ),
    }

    // ---- phase 3: test, once
    println!("### Phase 3: test (untouched until now)\n");
    let cand_cfg = selected.unwrap_or(short[by_val].1);
    let cand = Model::new(cand_cfg);
    let (ship_te, ship_c) = eval_split(&trie, &ship, &data.test);
    let (cand_te, cand_c) = eval_split(&trie, &cand, &data.test);
    println!(
        "{} {} (selected: {}); shipped macro MRR@10 {:.4}, candidate {:.4}\n",
        if selected.is_some() {
            "Candidate"
        } else {
            "Exploratory (not selected by the rule)"
        },
        cand_cfg.name(),
        selected.is_some(),
        macro_mrr(&ship_te),
        macro_mrr(&cand_te)
    );
    let d = diff(&cand_te, &ship_te);
    header(&data.test);
    print_diff_row(&format!("{} - shipped", cand_cfg.name()), &d);
    println!(
        "\nper corpus MRR@10 (shipped / candidate): {}\n",
        data.test
            .iter()
            .enumerate()
            .map(|(i, c)| format!(
                "{} {:.4} / {:.4}",
                c.name,
                mean(&ship_te[i]),
                mean(&cand_te[i])
            ))
            .collect::<Vec<_>>()
            .join("; ")
    );
    println!(
        "counters: candidate incomplete top-10 lists {}, truncated searches {}; shipped truncated {}\n",
        cand_c.incomplete, cand_c.truncated, ship_c.truncated
    );

    // categories, pooled over the test corpora
    let cm = CostModel::qwerty();
    let mut all_c: Vec<f64> = Vec::new();
    let mut all_s: Vec<f64> = Vec::new();
    let mut cats: BTreeMap<&'static str, Vec<usize>> = BTreeMap::new();
    let mut flat = 0usize;
    for (ci, corpus) in data.test.iter().enumerate() {
        for (pi, p) in corpus.pairs.iter().enumerate() {
            for cat in classify(&p.typo, &p.right, &cm) {
                cats.entry(cat).or_default().push(flat);
            }
            all_c.push(cand_te[ci][pi]);
            all_s.push(ship_te[ci][pi]);
            flat += 1;
        }
    }
    println!("categories on the pooled test pairs (candidate - shipped, MRR@10):\n");
    println!(
        "| category | n | shipped | candidate | difference (SE) | significantly negative |\n|---|---|---|---|---|---|"
    );
    let mut cat_loss = false;
    for (cat, idx) in &cats {
        let (dd, se) = paired_idx(&all_c, &all_s, idx);
        let neg = dd + Z95 * se < 0.0;
        if neg && idx.len() >= MIN_CATEGORY {
            cat_loss = true;
        }
        println!(
            "| {cat} | {} | {:.4} | {:.4} | {} | {}{} |",
            idx.len(),
            idx.iter().map(|&i| all_s[i]).sum::<f64>() / idx.len() as f64,
            idx.iter().map(|&i| all_c[i]).sum::<f64>() / idx.len() as f64,
            fmt_d(dd, se),
            neg,
            if idx.len() < MIN_CATEGORY {
                " (n < 50, not in the rule)"
            } else {
                ""
            }
        );
    }
    // verdict
    println!("\n### Verdict against the pre-registered rule\n");
    let corpus_loss = d.vetoed();
    match selected {
        None => println!(
            "No candidate qualified on validation, so no change is proposed. (The test row above is exploratory.)"
        ),
        Some(_) => {
            let verdict = if d.lb() > 0.0 && !corpus_loss && !cat_loss {
                "PROPOSE the candidate as a follow-up"
            } else if d.lb() <= 0.0 && d.ub() >= INCONCLUSIVE_UPPER && !corpus_loss && !cat_loss {
                "INCONCLUSIVE"
            } else {
                "DO NOT propose"
            };
            println!(
                "test macro lower bound {:+.4} (> 0: {}), test corpus with significant loss: {}, category (n >= 50) with significant loss: {}, upper end {:+.4}. Verdict: {verdict}.",
                d.lb(),
                d.lb() > 0.0,
                corpus_loss,
                cat_loss,
                d.ub()
            );
        }
    }

    // reference: unit-cost baseline on the test set
    println!("\n### Reference: unit-cost baseline on the test set (not part of the rule)\n");
    let base: Vec<Vec<f64>> = data
        .test
        .iter()
        .map(|c| par_map(c.pairs.len(), threads, |i| baseline_rr(&words, &c.pairs[i])))
        .collect();
    header(&data.test);
    print_diff_row("shipped - baseline", &diff(&ship_te, &base));
    print_diff_row(
        &format!("{} - baseline", cand_cfg.name()),
        &diff(&cand_te, &base),
    );
    println!("\nbaseline macro MRR@10 {:.4}\n", macro_mrr(&base));

    // reference: the seen slips sample
    println!("### Reference: the finger-slip sample of #60 (seen before, not used by any rule)\n");
    let sample = [Corpus {
        name: "slips-sample",
        pairs: data.slips_sample.clone(),
    }];
    let (a, _) = eval_split(&trie, &ship, &sample);
    let (b, _) = eval_split(&trie, &cand, &sample);
    println!(
        "shipped MRR@10 {:.4} (the finger-slips report has 0.841), candidate {:.4}, difference {}\n",
        macro_mrr(&a),
        macro_mrr(&b),
        {
            let x = diff(&b, &a);
            fmt_d(x.macro_d, x.macro_se)
        }
    );

    // ---- nodes expanded by the first-byte factor (core Coarse ranking, k = 10, budget 32)
    println!(
        "### Nodes expanded by the first-byte factor (other costs shipped, `Coarse`, k = 10, budget 32, tsb on)\n"
    );
    println!(
        "| first-byte factor | split | corpus | queries | nodes expanded | per query | truncated |\n|---|---|---|---|---|---|---|"
    );
    for fq in [4u16, 5, 6] {
        let mut cfg = shipped_cfg;
        cfg.p.first_quarters = fq;
        let m = Model::new(cfg);
        for (label, split) in [
            ("train", &data.train),
            ("validation", &data.val),
            ("test", &data.test),
        ] {
            for corpus in split {
                let mut c = Counters::default();
                let mut s = Searcher::new();
                for p in &corpus.pairs {
                    top10(&mut s, &trie, &m, p.typo.as_bytes(), &mut c);
                }
                println!(
                    "| x{} | {label} | {} | {} | {} | {:.1} | {} |",
                    f64::from(fq) / 4.0,
                    corpus.name,
                    c.queries,
                    c.nodes,
                    c.nodes as f64 / c.queries as f64,
                    c.truncated
                );
            }
        }
    }
    if selected.is_some() {
        let mut c = Counters::default();
        let mut s = Searcher::new();
        let mut nodes_ship = 0u64;
        let mut n = 0u64;
        for corpus in &data.test {
            for p in &corpus.pairs {
                top10(&mut s, &trie, &cand, p.typo.as_bytes(), &mut c);
                n += 1;
            }
        }
        let mut c2 = Counters::default();
        for corpus in &data.test {
            for p in &corpus.pairs {
                top10(&mut s, &trie, &ship, p.typo.as_bytes(), &mut c2);
            }
        }
        nodes_ship += c2.nodes;
        println!(
            "\ncandidate on the test set: {} nodes expanded over {} queries ({:.1} per query; the \"after\" ranking searches with `Exact` and a larger k, so this is not comparable when the candidate uses it); shipped {} ({:.1})",
            c.nodes,
            n,
            c.nodes as f64 / n as f64,
            nodes_ship,
            nodes_ship as f64 / n as f64
        );
    }
}

/// Consistency checks that print pass or fail only (no quality numbers).
fn run_selfcheck(trie: &Trie, data: &Data) {
    let pairs: Vec<&Pair> = data
        .train
        .iter()
        .flat_map(|c| c.pairs.iter())
        .step_by(37)
        .take(300)
        .collect();
    let mut s = Searcher::new();
    let mut c = Counters::default();
    // (a) exact cost read from the search equals the oracle under every factor
    let mut bad = 0;
    for fq in [4u16, 5, 6] {
        let mut cfg = shipped();
        cfg.p.first_quarters = fq;
        let m = Model::new(cfg);
        for p in &pairs {
            for h in top10(&mut s, trie, &m, p.typo.as_bytes(), &mut c) {
                if u32::from(h.cost) != oracle(&m.cm, p.typo.as_bytes(), trie.term(h.id).as_bytes())
                {
                    bad += 1;
                }
            }
        }
    }
    println!(
        "check a (Hit::cost equals the independent DP): {}",
        if bad == 0 { "pass" } else { "FAIL" }
    );
    // (b) "after" with factor 1.0 is the same list as "before"
    let mut f1 = shipped();
    f1.p.first_quarters = 4;
    let mb = Model::new(f1);
    let mut diffs = 0;
    for p in &pairs {
        let before = top10(&mut s, trie, &mb, p.typo.as_bytes(), &mut c);
        let cfg = SearchConfig {
            k: 4096,
            budget: BUDGET,
            tsb: true,
            ranking: Ranking::Exact,
            ..SearchConfig::default()
        };
        let out = s
            .search(trie, &mb.cm, p.typo.as_bytes(), &cfg)
            .expect("search");
        let mut keyed: Vec<(u32, std::cmp::Reverse<u16>, u32)> = out
            .hits
            .iter()
            .map(|h| {
                (
                    u32::from(whole_units(h.cost)) * 16,
                    std::cmp::Reverse(h.weight),
                    h.id,
                )
            })
            .collect();
        keyed.sort();
        let after: Vec<u32> = keyed.iter().take(TOP).map(|x| x.2).collect();
        let b: Vec<u32> = before.iter().map(|h| h.id).collect();
        if after != b {
            diffs += 1;
        }
    }
    println!(
        "check b (Coarse equals units-then-weight-then-id over the Exact list at factor 1.0): {}",
        if diffs == 0 { "pass" } else { "FAIL" }
    );
    // (c) the "after" list always contains only terms that the exact list holds and is complete
    let mut m5 = shipped();
    m5.after = true;
    let m5 = Model::new(m5);
    let mut n_incomplete = 0;
    for p in &pairs {
        let mut cc = Counters::default();
        let _ = top10(&mut s, trie, &m5, p.typo.as_bytes(), &mut cc);
        n_incomplete += cc.incomplete;
    }
    println!(
        "check c (\"after\" lists complete): {}",
        if n_incomplete == 0 { "pass" } else { "FAIL" }
    );
}
