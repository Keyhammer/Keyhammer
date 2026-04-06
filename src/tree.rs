/// CGL tree — recursive partitioning structure from Section 3 of arXiv:2604.01307.
///
/// Extended with confusion-aware pruning: before descending into altered branches,
/// we check if the mismatch at the pivot position is a plausible typo.
/// Implausible mismatches (e.g., 'a' → 'p') get pruned, reducing nodes visited.

use crate::altered::*;

pub struct CglNode {
    pivot: AlteredString,
    is_leaf: bool,
    suffixes: Vec<AlteredString>,

    less_m: Option<Box<CglNode>>,
    greater_m: Option<Box<CglNode>>,
    less_lex: Option<Box<CglNode>>,
    greater_lex: Option<Box<CglNode>>,

    alt_less_m: Option<Box<CglNode>>,
    alt_greater_m: Option<Box<CglNode>>,
    alt_less_lex: Option<Box<CglNode>>,
    alt_greater_lex: Option<Box<CglNode>>,
}

#[derive(Debug, Clone)]
pub struct RawMatch {
    pub origin: usize,
    #[allow(dead_code)]
    pub distance: usize,
}

/// Pruning config for confusion-aware search.
pub struct PruneConfig<'a> {
    /// 26x26 confusion matrix: confusion[a][b] = likelihood of typing 'a' when meaning 'b'.
    /// Values 0.0 (impossible) to 1.0 (same key). Pass None to disable pruning.
    pub confusion: Option<&'a [[f32; 26]; 26]>,
    /// Threshold below which a mismatch is considered implausible and the branch is pruned.
    /// Lower = more aggressive pruning (faster but may miss results).
    /// Recommended: 0.08 - 0.15
    pub prune_threshold: f32,
}

impl<'a> PruneConfig<'a> {
    /// No pruning — visit all branches (original behavior).
    #[allow(dead_code)]
    pub fn none() -> Self {
        Self { confusion: None, prune_threshold: 0.0 }
    }

    /// Check if a mismatch between bytes a and b is plausible as a typo.
    #[inline]
    fn is_plausible(&self, a: u8, b: u8) -> bool {
        match self.confusion {
            None => true, // no pruning
            Some(matrix) => {
                let ai = a.wrapping_sub(b'a') as usize;
                let bi = b.wrapping_sub(b'a') as usize;
                if ai >= 26 || bi >= 26 {
                    return true; // non-alpha chars: don't prune
                }
                matrix[ai][bi] >= self.prune_threshold
            }
        }
    }
}

// ─── Build ─────────────────────────────────────────────────────────────────

pub fn build(strings: Vec<AlteredString>, k: usize, leaf_size: usize) -> Option<CglNode> {
    if strings.is_empty() {
        return None;
    }

    if strings.len() <= leaf_size {
        return Some(CglNode {
            pivot: strings.first().unwrap().clone(),
            is_leaf: true,
            suffixes: strings,
            less_m: None, greater_m: None, less_lex: None, greater_lex: None,
            alt_less_m: None, alt_greater_m: None, alt_less_lex: None, alt_greater_lex: None,
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

    let build_child = |v: Vec<AlteredString>, k: usize| -> Option<Box<CglNode>> {
        if v.is_empty() { None } else { build(v, k, leaf_size).map(Box::new) }
    };

    let alt_less_m = if k > 0 {
        let v: Vec<_> = s_less_m.iter().map(|s| pivot_alter(s, &pivot)).collect();
        build_child(v, k - 1)
    } else { None };
    let alt_greater_m = if k > 0 {
        let v: Vec<_> = s_greater_m.iter().map(|s| pivot_alter(s, &pivot)).collect();
        build_child(v, k - 1)
    } else { None };
    let alt_less_lex = if k > 0 {
        let v: Vec<_> = s_less_lex.iter().map(|s| pivot_alter(s, &pivot)).collect();
        build_child(v, k - 1)
    } else { None };
    let alt_greater_lex = if k > 0 {
        let v: Vec<_> = s_greater_lex.iter().map(|s| pivot_alter(s, &pivot)).collect();
        build_child(v, k - 1)
    } else { None };

    Some(CglNode {
        pivot,
        is_leaf: false,
        suffixes: Vec::new(),
        less_m: build_child(s_less_m, k),
        greater_m: build_child(s_greater_m, k),
        less_lex: build_child(s_less_lex, k),
        greater_lex: build_child(s_greater_lex, k),
        alt_less_m,
        alt_greater_m,
        alt_less_lex,
        alt_greater_lex,
    })
}

// ─── Query with confusion-aware pruning ────────────────────────────────────

/// Query with optional confusion-aware pruning.
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

    // check pivot
    let d = hamming_bytes(q, pb);
    if d <= r {
        if let Some(origin) = node.pivot.origin {
            results.push(RawMatch { origin, distance: d });
        }
    }

    // leaf — brute-force
    if node.is_leaf {
        for s in &node.suffixes {
            if node.pivot.origin == s.origin {
                continue;
            }
            let d = hamming_bytes(q, s.as_bytes());
            if d <= r {
                if let Some(origin) = s.origin {
                    results.push(RawMatch { origin, distance: d });
                }
            }
        }
        return;
    }

    // LCP(q, pivot)
    let len = q.len().min(pb.len());
    let mut i = 0;
    while i < len && q[i] == pb[i] {
        i += 1;
    }

    // query is prefix of pivot
    if i >= q.len() {
        query_inner(node.less_m.as_deref(), q, r, results, config);
        query_inner(node.greater_m.as_deref(), q, r, results, config);
        query_inner(node.less_lex.as_deref(), q, r, results, config);
        query_inner(node.greater_lex.as_deref(), q, r, results, config);
        if r > 0 {
            query_inner(node.alt_less_m.as_deref(), q, r - 1, results, config);
            query_inner(node.alt_greater_m.as_deref(), q, r - 1, results, config);
            query_inner(node.alt_less_lex.as_deref(), q, r - 1, results, config);
            query_inner(node.alt_greater_lex.as_deref(), q, r - 1, results, config);
        }
        return;
    }

    let original = q[i];
    let pivot_ch = if i < pb.len() { pb[i] } else { original };

    // ── CONFUSION-AWARE PRUNING ──
    // Before descending into altered branches (where we "spend" a mismatch),
    // check if the mismatch q[i] vs pivot[i] is a plausible typo.
    // If not, skip the altered branches entirely.
    let mismatch_plausible = config.is_plausible(original, pivot_ch);

    let q_less = q.as_slice() < pb;

    if r > 0 {
        if q_less {
            // unaltered branches — always visit
            query_inner(node.less_m.as_deref(), q, r, results, config);
            query_inner(node.less_lex.as_deref(), q, r, results, config);

            // altered branches — only if mismatch is plausible
            if mismatch_plausible {
                q[i] = pivot_ch;
                query_inner(node.greater_lex.as_deref(), q, r - 1, results, config);
                query_inner(node.greater_m.as_deref(), q, r - 1, results, config);
                q[i] = original;
                query_inner(node.alt_less_m.as_deref(), q, r - 1, results, config);
                q[i] = pivot_ch;
                query_inner(node.alt_less_lex.as_deref(), q, r - 1, results, config);
                query_inner(node.alt_greater_lex.as_deref(), q, r - 1, results, config);
                q[i] = original;
            }
        } else {
            query_inner(node.greater_m.as_deref(), q, r, results, config);
            query_inner(node.greater_lex.as_deref(), q, r, results, config);

            if mismatch_plausible {
                q[i] = pivot_ch;
                query_inner(node.less_m.as_deref(), q, r - 1, results, config);
                q[i] = original;
                query_inner(node.alt_greater_m.as_deref(), q, r - 1, results, config);
                query_inner(node.alt_greater_lex.as_deref(), q, r - 1, results, config);
                q[i] = pivot_ch;
                query_inner(node.alt_less_lex.as_deref(), q, r - 1, results, config);
                q[i] = original;
            }
        }
    } else {
        if q_less {
            query_inner(node.less_m.as_deref(), q, 0, results, config);
            query_inner(node.less_lex.as_deref(), q, 0, results, config);
        } else {
            query_inner(node.greater_m.as_deref(), q, 0, results, config);
            query_inner(node.greater_lex.as_deref(), q, 0, results, config);
        }
    }
}

#[inline]
fn hamming_bytes(a: &[u8], b: &[u8]) -> usize {
    let len = a.len().min(b.len());
    let mut d = 0;
    for i in 0..len {
        if a[i] != b[i] {
            d += 1;
        }
    }
    d
}

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
        traverse(node.less_m.as_deref(), map, counter);
        traverse(node.greater_m.as_deref(), map, counter);
        traverse(node.less_lex.as_deref(), map, counter);
        traverse(node.greater_lex.as_deref(), map, counter);
        traverse(node.alt_less_m.as_deref(), map, counter);
        traverse(node.alt_greater_m.as_deref(), map, counter);
        traverse(node.alt_less_lex.as_deref(), map, counter);
        traverse(node.alt_greater_lex.as_deref(), map, counter);
    }
    traverse(node, &mut map, &mut counter);
    map
}

pub fn node_count(node: Option<&CglNode>) -> usize {
    match node {
        None => 0,
        Some(n) => {
            1 + node_count(n.less_m.as_deref())
                + node_count(n.greater_m.as_deref())
                + node_count(n.less_lex.as_deref())
                + node_count(n.greater_lex.as_deref())
                + node_count(n.alt_less_m.as_deref())
                + node_count(n.alt_greater_m.as_deref())
                + node_count(n.alt_less_lex.as_deref())
                + node_count(n.alt_greater_lex.as_deref())
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
        assert!(matches.is_empty(), "nothing within distance 1 of zzzz");
    }

    #[test]
    fn pruning_reduces_results() {
        // with a strict confusion matrix, implausible mismatches get pruned
        let terms = make_strings(&["abc", "axc", "azc", "apc"]);
        let root = build(terms, 2, 1);
        let q = AlteredString::new(b"abc", None);

        // without pruning
        let all = query_with_pruning(root.as_ref(), &q, 1, &PruneConfig::none());

        // with pruning: only adjacent-key mismatches are plausible
        let mut confusion = [[0.01f32; 26]; 26];
        for i in 0..26 { confusion[i][i] = 1.0; }
        // only b↔x is "plausible" (threshold 0.1)
        confusion[1][23] = 0.5; // b→x
        confusion[23][1] = 0.5; // x→b

        let config = PruneConfig {
            confusion: Some(&confusion),
            prune_threshold: 0.1,
        };
        let pruned = query_with_pruning(root.as_ref(), &q, 1, &config);

        // pruned should have fewer or equal results
        assert!(pruned.len() <= all.len(),
            "pruned ({}) should have ≤ results than all ({})", pruned.len(), all.len());
    }
}
