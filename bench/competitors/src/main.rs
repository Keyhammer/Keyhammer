// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

//! Competitive benchmark: keyhammer against SymSpell, an `fst` Levenshtein
//! automaton, a `strsim` brute-force scan and a BK-tree, on the same
//! dictionaries and typo pairs. Prints Markdown tables.

mod alloc;
mod engines;

use std::collections::HashSet;
use std::fs;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use engines::{Engine, TOP};

#[global_allocator]
static COUNTING: alloc::Counting = alloc::Counting;

/// Two-sided 95% normal quantile.
const Z95: f64 = 1.96;

const HELP: &str = "\
keyhammer-competitors: compare keyhammer with Rust typo-tolerant search libraries

USAGE:
    cargo run --release -- [OPTIONS]

OPTIONS:
    --data DIR           directory with words-{10000,100000,full}.tsv and tests.tsv
                         (from bench/fetch-data.mjs and bench/prepare-m0-data.mjs)
                         [default: ../data]
    --corpus-dir DIR     directory with gtc.tsv and wiki.tsv (from
                         bench/fetch-typo-corpora.mjs) [default: the --data directory]
    --corpora LIST       comma-separated: birkbeck (tests.tsv), gtc, wiki
                         [default: birkbeck,gtc,wiki]
    --sizes LIST         comma-separated dictionary sizes: 10000, 100000, full
                         [default: 10000,100000,full]
    --engines LIST       comma-separated: keyhammer, symspell, symspell/rerank, fst,
                         strsim, bk-tree [default: all]
    --repetitions N      latency runs per configuration; the report takes the
                         median of the runs [default: 3]
    --min-queries N      timed queries per run at least (the query set is
                         repeated) [default: 1000]
    --skip-latency       quality, correctness and memory only
    --latency-only       latency only (skip quality, correctness and memory)
    -h, --help           print this help
";

struct Args {
    data: PathBuf,
    corpus_dir: PathBuf,
    corpora: Vec<String>,
    sizes: Vec<String>,
    engines: Vec<String>,
    repetitions: usize,
    min_queries: usize,
    quality: bool,
    latency: bool,
}

fn list(v: &str) -> Vec<String> {
    v.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn parse_args() -> Result<Option<Args>, String> {
    let mut a = Args {
        data: PathBuf::from("../data"),
        corpus_dir: PathBuf::new(),
        corpora: list("birkbeck,gtc,wiki"),
        sizes: list("10000,100000,full"),
        engines: engines::ALL.iter().map(|s| s.to_string()).collect(),
        repetitions: 3,
        min_queries: 1000,
        quality: true,
        latency: true,
    };
    let mut corpus_dir = None;
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "-h" | "--help" => return Ok(None),
            "--data" => a.data = PathBuf::from(value()?),
            "--corpus-dir" => corpus_dir = Some(PathBuf::from(value()?)),
            "--corpora" => a.corpora = list(&value()?),
            "--sizes" => a.sizes = list(&value()?),
            "--engines" => a.engines = list(&value()?),
            "--repetitions" => {
                a.repetitions = value()?.parse().map_err(|e| format!("{flag}: {e}"))?;
            }
            "--min-queries" => {
                a.min_queries = value()?.parse().map_err(|e| format!("{flag}: {e}"))?;
            }
            "--skip-latency" => a.latency = false,
            "--latency-only" => a.quality = false,
            other => return Err(format!("unknown option {other} (see --help)")),
        }
    }
    a.corpus_dir = corpus_dir.unwrap_or_else(|| a.data.clone());
    for (i, e) in a.engines.iter().enumerate() {
        if !engines::ALL.contains(&e.as_str()) {
            return Err(format!("unknown engine {e}"));
        }
        if a.engines[..i].contains(e) {
            return Err(format!("engine {e} listed twice"));
        }
    }
    for c in &a.corpora {
        if !["birkbeck", "gtc", "wiki"].contains(&c.as_str()) {
            return Err(format!("unknown corpus {c}"));
        }
    }
    if a.repetitions == 0 {
        return Err("--repetitions must be at least 1".into());
    }
    Ok(Some(a))
}

fn read_pairs(path: &Path) -> Result<Vec<(String, String)>, String> {
    let text =
        fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut out = Vec::new();
    let mut bad = 0usize;
    for l in text.lines().filter(|l| !l.trim().is_empty()) {
        match l.split_once('\t') {
            Some((a, b)) => out.push((
                a.to_string(),
                b.split('\t').next().unwrap_or("").to_string(),
            )),
            None => bad += 1,
        }
    }
    if bad > 0 {
        return Err(format!(
            "{}: {bad} non-empty lines without a tab",
            path.display()
        ));
    }
    Ok(out)
}

fn read_dict(path: &Path) -> Result<Vec<(String, u16)>, String> {
    let mut out = Vec::new();
    let mut bad = 0usize;
    for (w, f) in read_pairs(path)? {
        match f.trim().parse() {
            Ok(v) => out.push((w.to_lowercase(), v)),
            Err(_) => bad += 1,
        }
    }
    if bad > 0 {
        return Err(format!(
            "{}: {bad} lines whose weight is not an integer in 0..=65535",
            path.display()
        ));
    }
    Ok(out)
}

fn corpus_path(a: &Args, name: &str) -> PathBuf {
    match name {
        "birkbeck" => a.data.join("tests.tsv"),
        other => a.corpus_dir.join(format!("{other}.tsv")),
    }
}

// --- statistics ----------------------------------------------------------------

fn mean(v: &[f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.iter().sum::<f64>() / v.len() as f64
}

/// Paired difference `a - b`: mean, standard error (sample sd / sqrt(n)).
fn paired(a: &[f64], b: &[f64]) -> (f64, f64) {
    let d: Vec<f64> = a.iter().zip(b).map(|(x, y)| x - y).collect();
    let n = d.len() as f64;
    let m = mean(&d);
    if d.len() < 2 {
        return (m, 0.0);
    }
    let var = d.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / (n - 1.0);
    (m, (var / n).sqrt())
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    sorted[((sorted.len() as f64 * p) as usize).min(sorted.len() - 1)]
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    v[v.len() / 2]
}

// --- engines --------------------------------------------------------------------

struct Built {
    name: String,
    engine: Box<dyn Engine>,
    build_ms: f64,
    /// Heap bytes of the index (see the report for what is counted).
    bytes: usize,
    /// Peak extra heap bytes during the build.
    peak: usize,
    note: String,
}

/// Builds every requested engine on `words`, measuring build time and the live
/// heap bytes it leaves behind. SymSpell is built once and shared by its two
/// rankings.
fn build_all(names: &[String], words: &[(String, u16)]) -> Vec<Built> {
    let mut out = Vec::new();
    let mut sym: Option<(std::rc::Rc<engines::SymIndex>, f64, usize, usize)> = None;
    for name in names {
        let before = alloc::live();
        alloc::reset_peak();
        let t0 = Instant::now();
        let mut note = String::new();
        let engine: Box<dyn Engine> = match name.as_str() {
            "keyhammer" => Box::new(engines::Keyhammer::build(words)),
            "symspell" | "symspell/rerank" => {
                if sym.is_none() {
                    let idx = engines::build_symspell(words);
                    let ms = t0.elapsed().as_secs_f64() * 1e3;
                    sym = Some((idx, ms, alloc::live() - before, alloc::peak() - before));
                }
                let (idx, _, _, _) = sym.as_ref().expect("built above");
                Box::new(engines::Sym::new(idx.clone(), name == "symspell/rerank"))
            }
            "fst" => {
                let f = engines::Fst::build(words);
                note = format!("fst size {} B", f.bytes());
                Box::new(f)
            }
            "strsim" => Box::new(engines::Brute::build(words)),
            "bk-tree" => Box::new(engines::BkTree::build(words)),
            _ => unreachable!("checked in parse_args"),
        };
        let (build_ms, bytes, peak) = match (name.as_str(), &sym) {
            ("symspell" | "symspell/rerank", Some((_, ms, b, p))) => {
                note = "one index shared by both SymSpell rows".into();
                (*ms, *b, *p)
            }
            _ => (
                t0.elapsed().as_secs_f64() * 1e3,
                alloc::live().saturating_sub(before),
                alloc::peak().saturating_sub(before),
            ),
        };
        out.push(Built {
            name: name.clone(),
            engine,
            build_ms,
            bytes,
            peak,
            note,
        });
    }
    out
}

/// The pairs usable with this dictionary: the correct word is in it and the
/// typo is not. Both are lower-cased.
fn usable(pairs: &[(String, String)], dict: &HashSet<&str>) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(t, c)| (t.to_lowercase(), c.to_lowercase()))
        .filter(|(t, c)| dict.contains(c.as_str()) && !dict.contains(t.as_str()) && t != c)
        .collect()
}

struct Quality {
    rr: Vec<f64>,
    r1: f64,
    r10: f64,
    tops: Vec<Vec<String>>,
}

fn quality(e: &mut dyn Engine, pairs: &[(String, String)]) -> Quality {
    let (mut rr, mut r1, mut r10, mut tops) = (Vec::new(), 0usize, 0usize, Vec::new());
    for (typo, right) in pairs {
        e.run(typo);
        let top: Vec<String> = e.top().iter().take(TOP).map(|s| s.to_string()).collect();
        let rank = top.iter().position(|t| t == right);
        rr.push(rank.map_or(0.0, |r| 1.0 / (r as f64 + 1.0)));
        r1 += usize::from(rank == Some(0));
        r10 += usize::from(rank.is_some());
        tops.push(top);
    }
    let n = pairs.len().max(1) as f64;
    Quality {
        rr,
        r1: r1 as f64 / n,
        r10: r10 as f64 / n,
        tops,
    }
}

/// Mean overlap of two top-10 lists (|A and B| / max(|A|, |B|), 1 when both
/// are empty) and the number of queries whose ordered lists are identical.
fn overlap(a: &[Vec<String>], b: &[Vec<String>]) -> (f64, usize) {
    let mut sum = 0.0;
    let mut same = 0;
    for (x, y) in a.iter().zip(b) {
        let m = x.len().max(y.len());
        sum += if m == 0 {
            1.0
        } else {
            x.iter().filter(|t| y.contains(t)).count() as f64 / m as f64
        };
        same += usize::from(x == y);
    }
    (sum / a.len().max(1) as f64, same)
}

fn run_quality(a: &Args, size: &str, words: &[(String, u16)]) -> Result<(), String> {
    let dict: HashSet<&str> = words.iter().map(|(w, _)| w.as_str()).collect();
    println!("\n## Dictionary {size} ({} terms)\n", words.len());
    let mut built = build_all(&a.engines, words);
    println!(
        "| Engine | Build (ms) | Index heap bytes | Bytes/term | Peak extra during build | Note |"
    );
    println!("|---|---|---|---|---|---|");
    for b in &built {
        println!(
            "| {} | {:.0} | {} | {:.1} | {} | {} |",
            b.name,
            b.build_ms,
            b.bytes,
            b.bytes as f64 / words.len() as f64,
            b.peak,
            b.note
        );
    }
    for corpus in &a.corpora {
        let all = read_pairs(&corpus_path(a, corpus))?;
        let pairs = usable(&all, &dict);
        // Reachable: the correct word is within optimal string alignment
        // distance 2 of the typo (an upper bound on R@10 under that model).
        let reach = pairs
            .iter()
            .filter(|(t, c)| strsim::osa_distance(t, c) <= engines::RADIUS)
            .count();
        println!(
            "\n### {size} / {corpus}: {} of {} pairs usable, {} ({:.3}) within OSA distance 2\n",
            pairs.len(),
            all.len(),
            reach,
            reach as f64 / pairs.len().max(1) as f64
        );
        let mut results = Vec::new();
        for b in &mut built {
            let (e0, t0) = (b.engine.errors(), b.engine.truncated());
            let q = quality(b.engine.as_mut(), &pairs);
            let counts = (b.engine.errors() - e0, b.engine.truncated() - t0);
            results.push((b.name.clone(), q, counts));
        }
        let brute = results.iter().position(|r| r.0 == "strsim");
        println!(
            "| Engine | MRR@10 | R@1 | R@10 | top-10 overlap with strsim | identical top-10 lists | errors | truncated |"
        );
        println!("|---|---|---|---|---|---|---|---|");
        for (name, q, (err, trunc)) in &results {
            let (ov, same) = brute.map_or((f64::NAN, 0), |i| overlap(&q.tops, &results[i].1.tops));
            println!(
                "| {name} | {:.3} | {:.3} | {:.3} | {:.3} | {} / {} | {err} | {trunc} |",
                mean(&q.rr),
                q.r1,
                q.r10,
                ov,
                same,
                pairs.len()
            );
        }
        if let Some(kh) = results.iter().find(|r| r.0 == "keyhammer") {
            println!(
                "\n| keyhammer minus | MRR difference | SE | 95% interval | abs(diff)/SE | queries that differ |"
            );
            println!("|---|---|---|---|---|---|");
            for (name, q, _) in results.iter().filter(|r| r.0 != "keyhammer") {
                let (m, se) = paired(&kh.1.rr, &q.rr);
                let differ = kh.1.rr.iter().zip(&q.rr).filter(|(x, y)| x != y).count();
                let ratio = if se > 0.0 { m.abs() / se } else { 0.0 };
                println!(
                    "| {name} | {m:+.4} | {se:.4} | [{:+.4}, {:+.4}] | {ratio:.1} | {differ} |",
                    m - Z95 * se,
                    m + Z95 * se
                );
            }
        }
    }
    Ok(())
}

struct Lat {
    p50: f64,
    p95: f64,
    p99: f64,
    qps: f64,
}

fn time_once(e: &mut dyn Engine, queries: &[String], min_queries: usize) -> Lat {
    for q in queries {
        e.run(black_box(q));
    }
    let reps = min_queries.div_ceil(queries.len().max(1)).max(1);
    let mut lat = Vec::with_capacity(reps * queries.len());
    let wall = Instant::now();
    for _ in 0..reps {
        for q in queries {
            let t = Instant::now();
            e.run(black_box(q));
            lat.push(t.elapsed().as_secs_f64() * 1e6);
        }
    }
    let total = wall.elapsed().as_secs_f64();
    lat.sort_by(|a, b| a.total_cmp(b));
    Lat {
        p50: percentile(&lat, 0.50),
        p95: percentile(&lat, 0.95),
        p99: percentile(&lat, 0.99),
        qps: lat.len() as f64 / total,
    }
}

fn run_latency(a: &Args, size: &str, words: &[(String, u16)]) -> Result<(), String> {
    let dict: HashSet<&str> = words.iter().map(|(w, _)| w.as_str()).collect();
    let mut built = build_all(&a.engines, words);
    for corpus in &a.corpora {
        let pairs = usable(&read_pairs(&corpus_path(a, corpus))?, &dict);
        let queries: Vec<String> = pairs.into_iter().map(|(t, _)| t).collect();
        if queries.is_empty() {
            println!(
                "
### Latency {size} / {corpus}: no usable pairs, skipped"
            );
            continue;
        }
        let reps = a.min_queries.div_ceil(queries.len()).max(1);
        println!(
            "\n### Latency {size} / {corpus}: {} queries x {reps} = {} timed per run, {} runs\n",
            queries.len(),
            queries.len() * reps,
            a.repetitions
        );
        // runs[engine][run]
        let mut runs: Vec<Vec<Lat>> = built.iter().map(|_| Vec::new()).collect();
        for _ in 0..a.repetitions {
            for (i, b) in built.iter_mut().enumerate() {
                runs[i].push(time_once(b.engine.as_mut(), &queries, a.min_queries));
            }
        }
        println!("| Engine | p50 (us) | p95 (us) | p99 (us) | queries/s | p50 range | p95 range |");
        println!("|---|---|---|---|---|---|---|");
        for (b, r) in built.iter().zip(&runs) {
            let col = |f: fn(&Lat) -> f64| r.iter().map(f).collect::<Vec<f64>>();
            let (mut p50, mut p95, mut p99, mut qps) = (
                col(|l| l.p50),
                col(|l| l.p95),
                col(|l| l.p99),
                col(|l| l.qps),
            );
            let range = |v: &[f64]| {
                let lo = v.iter().copied().fold(f64::INFINITY, f64::min);
                let hi = v.iter().copied().fold(0.0, f64::max);
                format!("{lo:.1}-{hi:.1}")
            };
            let (r50, r95) = (range(&p50), range(&p95));
            println!(
                "| {} | {:.1} | {:.1} | {:.1} | {:.0} | {r50} | {r95} |",
                b.name,
                median(&mut p50),
                median(&mut p95),
                median(&mut p99),
                median(&mut qps)
            );
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(Some(a)) => a,
        Ok(None) => {
            print!("{HELP}");
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    let result = (|| -> Result<(), String> {
        println!(
            "# keyhammer competitive benchmark\n\nEngines: {}. Top {TOP}, edit radius {}.",
            args.engines.join(", "),
            engines::RADIUS
        );
        if args.quality {
            println!("\n# Quality, correctness, build time and memory");
            for size in &args.sizes {
                let words = read_dict(&args.data.join(format!("words-{size}.tsv")))?;
                run_quality(&args, size, &words)?;
            }
        }
        if args.latency {
            println!(
                "\n# Latency (single thread; warm-up pass, then every query timed; median of {} runs)",
                args.repetitions
            );
            for size in &args.sizes {
                let words = read_dict(&args.data.join(format!("words-{size}.tsv")))?;
                println!("\n## Dictionary {size} ({} terms)", words.len());
                run_latency(&args, size, &words)?;
            }
        }
        Ok(())
    })();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
