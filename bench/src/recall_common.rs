// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

//! Shared helpers of the recall research harnesses (`recall`, `phonetic`): data
//! loading, dictionaries, unit edit distance, paired statistics, and a
//! Metaphone-like and a Soundex phonetic key. Bench-only; the core crate is not
//! touched.

#![allow(dead_code)]

use std::fs;

pub const Z95: f64 = 1.96;

pub fn die(msg: &str) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(2);
}

pub fn read_pairs(path: &str) -> Vec<(String, String)> {
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

pub fn read_words(path: &str) -> Vec<(String, u16)> {
    read_pairs(path)
        .into_iter()
        .map(|(w, f)| {
            let f = f
                .parse::<u16>()
                .unwrap_or_else(|_| die(&format!("{path}: bad frequency {f:?}")));
            (w, f)
        })
        .collect()
}

/// Deterministic LCG shuffle.
pub fn shuffle<T>(v: &mut [T], seed: u64) {
    let mut s = seed;
    for i in (1..v.len()).rev() {
        s = s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let j = ((s >> 33) as usize) % (i + 1);
        v.swap(i, j);
    }
}

/// A dictionary of `size` words holding every target: the targets first, then
/// a deterministic random fill from `full`. A `size` at least as large as
/// `full` gives `full`.
pub fn dictionary(full: &[(String, u16)], targets: &[&str], size: usize) -> Vec<(String, u16)> {
    if size >= full.len() {
        return full.to_vec();
    }
    let tset: std::collections::HashSet<&str> = targets.iter().copied().collect();
    let mut out: Vec<(String, u16)> = full
        .iter()
        .filter(|(w, _)| tset.contains(w.as_str()))
        .cloned()
        .collect();
    let mut rest: Vec<&(String, u16)> = full
        .iter()
        .filter(|(w, _)| !tset.contains(w.as_str()))
        .collect();
    shuffle(&mut rest, 20260924);
    let need = size.saturating_sub(out.len());
    out.extend(rest.into_iter().take(need).cloned());
    out
}

/// Unit optimal-string-alignment distance.
pub fn osa(a: &[u8], b: &[u8]) -> usize {
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

pub fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len() as f64
}

/// Paired difference `a - b` over all indices: (mean, SE).
pub fn paired(a: &[f64], b: &[f64]) -> (f64, f64) {
    let d: Vec<f64> = a.iter().zip(b).map(|(x, y)| x - y).collect();
    let n = d.len();
    if n < 2 {
        return (mean(&d), f64::NAN);
    }
    let m = mean(&d);
    let var = d.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / (n as f64 - 1.0);
    (m, (var / n as f64).sqrt())
}

pub fn rr_of(ids: &[u32], right: u32) -> f64 {
    ids.iter()
        .position(|&i| i == right)
        .map_or(0.0, |r| 1.0 / (r as f64 + 1.0))
}

fn is_vowel(c: u8) -> bool {
    matches!(c, b'a' | b'e' | b'i' | b'o' | b'u')
}

/// A Metaphone-like key (a simplified Lawrence Philips Metaphone; not
/// truncated). Input: lowercase a-z bytes. Handwritten from the published
/// rules of the algorithm before any of the data was looked at.
pub fn metaphone(w: &[u8]) -> Vec<u8> {
    let mut s: Vec<u8> = w.to_vec();
    if s.len() >= 2 {
        match (s[0], s[1]) {
            (b'k', b'n') | (b'g', b'n') | (b'p', b'n') | (b'w', b'r') | (b'p', b's') => {
                s.remove(0);
            }
            (b'a', b'e') => {
                s.remove(0);
            }
            (b'w', b'h') => {
                s.remove(1);
            }
            _ => {}
        }
    }
    if s.first() == Some(&b'x') {
        s[0] = b's';
    }
    let n = s.len();
    let at = |i: isize| -> u8 {
        if i < 0 || i as usize >= n {
            0
        } else {
            s[i as usize]
        }
    };
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < n {
        let c = s[i];
        let ii = i as isize;
        if c == at(ii - 1) && c != b'c' {
            i += 1;
            continue;
        }
        let (p, nx, nx2) = (at(ii - 1), at(ii + 1), at(ii + 2));
        match c {
            b'a' | b'e' | b'i' | b'o' | b'u' => {
                if i == 0 {
                    out.push(b'A');
                }
            }
            b'b' => {
                if !(p == b'm' && i + 1 == n) {
                    out.push(b'B');
                }
            }
            b'c' => {
                if nx == b'i' && nx2 == b'a' {
                    out.push(b'X');
                } else if nx == b'h' {
                    out.push(b'X');
                    i += 1;
                } else if matches!(nx, b'i' | b'e' | b'y') {
                    if p != b's' {
                        out.push(b'S');
                    }
                } else {
                    out.push(b'K');
                }
            }
            b'd' => {
                if nx == b'g' && matches!(nx2, b'e' | b'y' | b'i') {
                    out.push(b'J');
                    i += 1;
                } else {
                    out.push(b'T');
                }
            }
            b'g' => {
                if nx == b'h' && !(i + 2 < n && is_vowel(nx2)) {
                    // silent gh (not before a vowel)
                    i += 1;
                } else if nx == b'n'
                    && (i + 2 == n || (nx2 == b'e' && at(ii + 3) == b'd' && i + 4 == n))
                {
                    // silent g in ...gn / ...gned
                } else if matches!(nx, b'i' | b'e' | b'y') && p != b'g' {
                    out.push(b'J');
                } else {
                    out.push(b'K');
                }
            }
            b'h' => {
                if is_vowel(nx) && !matches!(p, b'c' | b's' | b'p' | b't' | b'g') {
                    out.push(b'H');
                }
            }
            b'k' => {
                if p != b'c' {
                    out.push(b'K');
                }
            }
            b'p' => {
                if nx == b'h' {
                    out.push(b'F');
                    i += 1;
                } else {
                    out.push(b'P');
                }
            }
            b'q' => out.push(b'K'),
            b's' => {
                if nx == b'h' {
                    out.push(b'X');
                    i += 1;
                } else if nx == b'i' && matches!(nx2, b'o' | b'a') {
                    out.push(b'X');
                } else {
                    out.push(b'S');
                }
            }
            b't' => {
                if nx == b'i' && matches!(nx2, b'o' | b'a') {
                    out.push(b'X');
                } else if nx == b'h' {
                    out.push(b'0');
                    i += 1;
                } else if !(nx == b'c' && nx2 == b'h') {
                    out.push(b'T');
                }
            }
            b'v' => out.push(b'F'),
            b'w' | b'y' => {
                if is_vowel(nx) {
                    out.push(c.to_ascii_uppercase());
                }
            }
            b'x' => {
                out.push(b'K');
                out.push(b'S');
            }
            b'z' => out.push(b'S'),
            b'f' | b'j' | b'l' | b'm' | b'n' | b'r' => out.push(c.to_ascii_uppercase()),
            _ => {}
        }
        i += 1;
    }
    out
}

/// Standard American Soundex (letter + 3 digits).
pub fn soundex(w: &[u8]) -> Vec<u8> {
    let code = |c: u8| -> u8 {
        match c {
            b'b' | b'f' | b'p' | b'v' => b'1',
            b'c' | b'g' | b'j' | b'k' | b'q' | b's' | b'x' | b'z' => b'2',
            b'd' | b't' => b'3',
            b'l' => b'4',
            b'm' | b'n' => b'5',
            b'r' => b'6',
            _ => 0,
        }
    };
    let mut out = vec![w[0].to_ascii_uppercase()];
    let mut last = code(w[0]);
    for &c in &w[1..] {
        let k = code(c);
        if k != 0 && k != last {
            out.push(k);
        }
        if c != b'h' && c != b'w' {
            last = k;
        }
        if out.len() == 4 {
            break;
        }
    }
    while out.len() < 4 {
        out.push(b'0');
    }
    out
}
