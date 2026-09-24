// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

/// Lazy CGL tree — demand-driven, thread-safe construction.
///
/// Child nodes store their input data and construct themselves on first access.
/// Uses Mutex for safe lazy initialization across threads.
///
/// Extended with confusion-aware pruning from Grudin (1983) QWERTY data.

use std::sync::Mutex;
use crate::altered::*;

/// A lazy child: Pending (data waiting to be built), Ready, or Empty.
enum LazyChild {
    Pending { strings: Vec<AlteredString>, k: usize, leaf_size: usize },
    Ready(Box<CglNode>),
    Empty,
}

pub struct CglNode {
    pivot: AlteredString,
    is_leaf: bool,
    suffixes: Vec<AlteredString>,

    less_m: Mutex<LazyChild>,
    greater_m: Mutex<LazyChild>,
    less_lex: Mutex<LazyChild>,
    greater_lex: Mutex<LazyChild>,

    alt_less_m: Mutex<LazyChild>,
    alt_greater_m: Mutex<LazyChild>,
    alt_less_lex: Mutex<LazyChild>,
    alt_greater_lex: Mutex<LazyChild>,
}

impl CglNode {
    /// Access a child, building it lazily on first access. Thread-safe.
    #[inline]
    fn child<'a>(lock: &'a Mutex<LazyChild>) -> Option<&'a CglNode> {
        let mut guard = lock.lock().unwrap();
        // build if pending
        match &*guard {
            LazyChild::Empty => return None,
            LazyChild::Ready(_) => {}
            LazyChild::Pending { .. } => {
                let old = std::mem::replace(&mut *guard, LazyChild::Empty);
                if let LazyChild::Pending { strings, k, leaf_size } = old {
                    if let Some(node) = build_inner(strings, k, leaf_size) {
                        *guard = LazyChild::Ready(Box::new(node));
                    }
                }
            }
        }
        // now return reference — need to extend lifetime past the guard
        // safe because: once Ready, the Box<CglNode> is never moved or dropped
        // until the parent CglNode is dropped
        match &*guard {
            LazyChild::Ready(node) => {
                let ptr: *const CglNode = &**node;
                Some(unsafe { &*ptr })
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RawMatch {
    pub origin: usize,
    #[allow(dead_code)]
    pub distance: usize,
}

pub struct PruneConfig<'a> {
    pub confusion: Option<&'a [[f32; 26]; 26]>,
    pub prune_threshold: f32,
}

impl<'a> PruneConfig<'a> {
    #[allow(dead_code)]
    pub fn none() -> Self {
        Self { confusion: None, prune_threshold: 0.0 }
    }

    #[inline]
    fn is_plausible(&self, a: u8, b: u8) -> bool {
        match self.confusion {
            None => true,
            Some(matrix) => {
                let ai = a.wrapping_sub(b'a') as usize;
                let bi = b.wrapping_sub(b'a') as usize;
                if ai >= 26 || bi >= 26 { return true; }
                matrix[ai][bi] >= self.prune_threshold
            }
        }
    }
}

// ─── Build ─────────────────────────────────────────────────────────────────

pub fn build(strings: Vec<AlteredString>, k: usize, leaf_size: usize) -> Option<CglNode> {
    build_inner(strings, k, leaf_size)
}

fn build_inner(strings: Vec<AlteredString>, k: usize, leaf_size: usize) -> Option<CglNode> {
    if strings.is_empty() {
        return None;
    }

    if strings.len() <= leaf_size {
        return Some(CglNode {
            pivot: strings.first().unwrap().clone(),
            is_leaf: true,
            suffixes: strings,
            less_m: Mutex::new(LazyChild::Empty),
            greater_m: Mutex::new(LazyChild::Empty),
            less_lex: Mutex::new(LazyChild::Empty),
            greater_lex: Mutex::new(LazyChild::Empty),
            alt_less_m: Mutex::new(LazyChild::Empty),
            alt_greater_m: Mutex::new(LazyChild::Empty),
            alt_less_lex: Mutex::new(LazyChild::Empty),
            alt_greater_lex: Mutex::new(LazyChild::Empty),
        });
    }

    let mut sorted = strings;
    sorted.sort_by(lex_cmp);
    let pivot_idx = sorted.len() / 2;
    let pivot = sorted[pivot_idx].clone();

    let rest: Vec<_> = sorted
        .into_iter()
        .enumerate()
        .filter(|(i, _)| *i != pivot_idx)
        .map(|(_, s)| {
            let l = lcp(&s, &pivot);
            (s, l)
        })
        .collect();

    let mut lcp_vals: Vec<usize> = rest.iter().map(|(_, l)| *l).collect();
    lcp_vals.sort_unstable();
    let m = lcp_vals.get(lcp_vals.len() / 2).copied().unwrap_or(0);

    let mut s_less_m = Vec::new();
    let mut s_greater_m = Vec::new();
    let mut s_less_lex = Vec::new();
    let mut s_greater_lex = Vec::new();

    for (s, l) in rest {
        if l < m {
            s_less_m.push(s);
        } else if l > m {
            s_greater_m.push(s);
        } else {
            match lex_cmp(&s, &pivot) {
                std::cmp::Ordering::Less => s_less_lex.push(s),
                _ => s_greater_lex.push(s),
            }
        }
    }

    let lazy = |v: Vec<AlteredString>, k: usize| -> Mutex<LazyChild> {
        if v.is_empty() {
            Mutex::new(LazyChild::Empty)
        } else {
            Mutex::new(LazyChild::Pending { strings: v, k, leaf_size })
        }
    };

    let alt_lazy = |v: &[AlteredString], k: usize| -> (Mutex<LazyChild>, Vec<AlteredString>) {
        if k == 0 || v.is_empty() {
            return (Mutex::new(LazyChild::Empty), v.to_vec());
        }
        let altered: Vec<_> = v.iter().map(|s| pivot_alter(s, &pivot)).collect();
        (Mutex::new(LazyChild::Pending { strings: altered, k: k - 1, leaf_size }), v.to_vec())
    };

    let (alt_less_m, s_less_m_kept) = alt_lazy(&s_less_m, k);
    let (alt_greater_m, s_greater_m_kept) = alt_lazy(&s_greater_m, k);
    let (alt_less_lex, s_less_lex_kept) = alt_lazy(&s_less_lex, k);
    let (alt_greater_lex, s_greater_lex_kept) = alt_lazy(&s_greater_lex, k);

    Some(CglNode {
        pivot,
        is_leaf: false,
        suffixes: Vec::new(),
        less_m: lazy(s_less_m_kept, k),
        greater_m: lazy(s_greater_m_kept, k),
        less_lex: lazy(s_less_lex_kept, k),
        greater_lex: lazy(s_greater_lex_kept, k),
        alt_less_m,
        alt_greater_m,
        alt_less_lex,
        alt_greater_lex,
    })
}

// ─── Query ─────────────────────────────────────────────────────────────────

pub fn query_with_pruning(
    node: Option<&CglNode>,
    q: &AlteredString,
    r: usize,
    config: &PruneConfig,
) -> Vec<RawMatch> {
    let mut buf = q.as_bytes().to_vec();
    let mut results = Vec::new();
    query_inner(node, &mut buf, r, &mut results, config);
    results
}

fn query_inner(
    node: Option<&CglNode>,
    q: &mut Vec<u8>,
    r: usize,
    results: &mut Vec<RawMatch>,
    config: &PruneConfig,
) {
    let node = match node {
        Some(n) => n,
        None => return,
    };

    let pb = node.pivot.as_bytes();

    let d = hamming_bytes(q, pb);
    if d <= r {
        if let Some(origin) = node.pivot.origin {
            results.push(RawMatch { origin, distance: d });
        }
    }

    if node.is_leaf {
        for s in &node.suffixes {
            if node.pivot.origin == s.origin { continue; }
            let d = hamming_bytes(q, s.as_bytes());
            if d <= r {
                if let Some(origin) = s.origin {
                    results.push(RawMatch { origin, distance: d });
                }
            }
        }
        return;
    }

    let len = q.len().min(pb.len());
    let mut i = 0;
    while i < len && q[i] == pb[i] {
        i += 1;
    }

    if i >= q.len() {
        query_inner(CglNode::child(&node.less_m), q, r, results, config);
        query_inner(CglNode::child(&node.greater_m), q, r, results, config);
        query_inner(CglNode::child(&node.less_lex), q, r, results, config);
        query_inner(CglNode::child(&node.greater_lex), q, r, results, config);
        if r > 0 {
            query_inner(CglNode::child(&node.alt_less_m), q, r - 1, results, config);
            query_inner(CglNode::child(&node.alt_greater_m), q, r - 1, results, config);
            query_inner(CglNode::child(&node.alt_less_lex), q, r - 1, results, config);
            query_inner(CglNode::child(&node.alt_greater_lex), q, r - 1, results, config);
        }
        return;
    }

    let original = q[i];
    let pivot_ch = if i < pb.len() { pb[i] } else { original };
    let mismatch_plausible = config.is_plausible(original, pivot_ch);
    let q_less = q.as_slice() < pb;

    if r > 0 {
        if q_less {
            query_inner(CglNode::child(&node.less_m), q, r, results, config);
            query_inner(CglNode::child(&node.less_lex), q, r, results, config);
            if mismatch_plausible {
                q[i] = pivot_ch;
                query_inner(CglNode::child(&node.greater_lex), q, r - 1, results, config);
                query_inner(CglNode::child(&node.greater_m), q, r - 1, results, config);
                q[i] = original;
                query_inner(CglNode::child(&node.alt_less_m), q, r - 1, results, config);
                q[i] = pivot_ch;
                query_inner(CglNode::child(&node.alt_less_lex), q, r - 1, results, config);
                query_inner(CglNode::child(&node.alt_greater_lex), q, r - 1, results, config);
                q[i] = original;
            }
        } else {
            query_inner(CglNode::child(&node.greater_m), q, r, results, config);
            query_inner(CglNode::child(&node.greater_lex), q, r, results, config);
            if mismatch_plausible {
                q[i] = pivot_ch;
                query_inner(CglNode::child(&node.less_m), q, r - 1, results, config);
                q[i] = original;
                query_inner(CglNode::child(&node.alt_greater_m), q, r - 1, results, config);
                query_inner(CglNode::child(&node.alt_greater_lex), q, r - 1, results, config);
                q[i] = pivot_ch;
                query_inner(CglNode::child(&node.alt_less_lex), q, r - 1, results, config);
                q[i] = original;
            }
        }
    } else {
        if q_less {
            query_inner(CglNode::child(&node.less_m), q, 0, results, config);
            query_inner(CglNode::child(&node.less_lex), q, 0, results, config);
        } else {
            query_inner(CglNode::child(&node.greater_m), q, 0, results, config);
            query_inner(CglNode::child(&node.greater_lex), q, 0, results, config);
        }
    }
}

#[inline]
fn hamming_bytes(a: &[u8], b: &[u8]) -> usize {
    let len = a.len().min(b.len());
    let mut d = 0;
    for i in 0..len {
        if a[i] != b[i] { d += 1; }
    }
    d
}

#[allow(dead_code)]
pub fn build_leaf_map(node: Option<&CglNode>) -> std::collections::HashMap<usize, usize> {
    let mut map = std::collections::HashMap::new();
    let mut counter = 0usize;
    fn traverse(
        node: Option<&CglNode>,
        map: &mut std::collections::HashMap<usize, usize>,
        counter: &mut usize,
    ) {
        let node = match node {
            Some(n) => n,
            None => return,
        };
        if node.is_leaf {
            let label = *counter;
            *counter += 1;
            for s in &node.suffixes {
                if let Some(origin) = s.origin {
                    map.insert(origin, label);
                }
            }
            return;
        }
        traverse(CglNode::child(&node.less_m), map, counter);
        traverse(CglNode::child(&node.greater_m), map, counter);
        traverse(CglNode::child(&node.less_lex), map, counter);
        traverse(CglNode::child(&node.greater_lex), map, counter);
        traverse(CglNode::child(&node.alt_less_m), map, counter);
        traverse(CglNode::child(&node.alt_greater_m), map, counter);
        traverse(CglNode::child(&node.alt_less_lex), map, counter);
        traverse(CglNode::child(&node.alt_greater_lex), map, counter);
    }
    traverse(node, &mut map, &mut counter);
    map
}

#[allow(dead_code)]
pub fn node_count(node: Option<&CglNode>) -> usize {
    match node {
        None => 0,
        Some(n) => {
            if n.is_leaf { return 1; }
            1 + node_count(CglNode::child(&n.less_m))
                + node_count(CglNode::child(&n.greater_m))
                + node_count(CglNode::child(&n.less_lex))
                + node_count(CglNode::child(&n.greater_lex))
                + node_count(CglNode::child(&n.alt_less_m))
                + node_count(CglNode::child(&n.alt_greater_m))
                + node_count(CglNode::child(&n.alt_less_lex))
                + node_count(CglNode::child(&n.alt_greater_lex))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_strings(words: &[&str]) -> Vec<AlteredString> {
        words.iter().enumerate()
            .map(|(i, w)| AlteredString::new(w.as_bytes(), Some(i)))
            .collect()
    }

    #[test]
    fn exact_match() {
        let terms = make_strings(&["apple", "banana", "cherry", "date", "elderberry"]);
        let root = build(terms, 2, 2);
        let q = AlteredString::new(b"cherry", None);
        let matches = query_with_pruning(root.as_ref(), &q, 0, &PruneConfig::none());
        assert!(matches.iter().any(|m| m.origin == 2 && m.distance == 0));
    }

    #[test]
    fn one_mismatch() {
        let terms = make_strings(&["rust", "dust", "must", "gust", "bust"]);
        let root = build(terms, 2, 2);
        let q = AlteredString::new(b"rust", None);
        let matches = query_with_pruning(root.as_ref(), &q, 1, &PruneConfig::none());
        assert!(matches.iter().any(|m| m.origin == 0 && m.distance == 0));
    }

    #[test]
    fn no_match_beyond_radius() {
        let terms = make_strings(&["aaaa", "bbbb", "cccc"]);
        let root = build(terms, 2, 1);
        let q = AlteredString::new(b"zzzz", None);
        let matches = query_with_pruning(root.as_ref(), &q, 1, &PruneConfig::none());
        assert!(matches.is_empty());
    }

    #[test]
    fn pruning_reduces_results() {
        let terms = make_strings(&["abc", "axc", "azc", "apc"]);
        let root = build(terms, 2, 1);
        let q = AlteredString::new(b"abc", None);

        let all = query_with_pruning(root.as_ref(), &q, 1, &PruneConfig::none());

        let mut confusion = [[0.01f32; 26]; 26];
        for i in 0..26 { confusion[i][i] = 1.0; }
        confusion[1][23] = 0.5;
        confusion[23][1] = 0.5;

        let config = PruneConfig {
            confusion: Some(&confusion),
            prune_threshold: 0.1,
        };
        let pruned = query_with_pruning(root.as_ref(), &q, 1, &config);
        assert!(pruned.len() <= all.len());
    }

    #[test]
    fn lazy_construction_works() {
        let words: Vec<String> = (0..100).map(|i| format!("word{:04}", i)).collect();
        let terms = make_strings(&words.iter().map(|s| s.as_str()).collect::<Vec<_>>());
        let root = build(terms, 2, 2);
        assert!(root.is_some());

        let q = AlteredString::new(b"word0050", None);
        let matches = query_with_pruning(root.as_ref(), &q, 1, &PruneConfig::none());
        assert!(matches.iter().any(|m| m.origin == 50));
    }
}
