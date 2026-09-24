// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

//! # keyhammer-python
//!
//! The PyO3 binding behind the `keyhammer` Python package. Status:
//! unpublished prototype; the interface may change at any time.
//!
//! This crate contains no hand-written `unsafe`; the only `unsafe` code is
//! what PyO3 generates and ships. It exposes what the core supports today: an
//! immutable `Index` built once, and top-k search. The core has no overlay
//! (add/remove) and no serialisation yet, so neither does this binding.
//!
//! Terms and queries must be ASCII. ASCII letters are lower-cased (as in the
//! WebAssembly binding); other ASCII bytes are compared verbatim by the core.
//! Non-ASCII text is refused rather than compared byte by byte, because
//! Unicode support is not in the core yet.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use keyhammer::cost::CostModel;
use keyhammer::search::{
    Ranking as CoreRanking, SearchConfig as CoreConfig, SearchError as CoreSearchError, Searcher,
};
use keyhammer::trie::Trie;
use pyo3::create_exception;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;

create_exception!(
    _keyhammer,
    KeyhammerError,
    PyValueError,
    "Base class of every error raised by keyhammer (a `ValueError`)."
);
create_exception!(
    _keyhammer,
    BuildError,
    KeyhammerError,
    "The index could not be built from the given terms."
);
create_exception!(
    _keyhammer,
    SearchError,
    KeyhammerError,
    "A search was rejected."
);
create_exception!(
    _keyhammer,
    QueryTooLongError,
    SearchError,
    "The query is longer than 128 bytes."
);
create_exception!(
    _keyhammer,
    BudgetTooLargeError,
    SearchError,
    "The budget is larger than the supported maximum (64)."
);

/// How results are ordered.
#[pyclass(
    module = "keyhammer",
    eq,
    eq_int,
    frozen,
    hash,
    from_py_object,
    rename_all = "SCREAMING_SNAKE_CASE"
)]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Ranking {
    /// Weighted cost rounded up to whole units of 16, then higher weight, then term order.
    Coarse,
    /// Exact weighted cost, then higher weight, then term order.
    Exact,
}

impl From<Ranking> for CoreRanking {
    fn from(r: Ranking) -> Self {
        match r {
            Ranking::Coarse => CoreRanking::Coarse,
            Ranking::Exact => CoreRanking::Exact,
        }
    }
}

/// Checks `0 <= v <= max` and returns `v`, naming the field in the error.
fn bounded(name: &str, v: i64, max: u64) -> Result<u64, String> {
    u64::try_from(v)
        .ok()
        .filter(|&u| u <= max)
        .ok_or_else(|| format!("{name} must be between 0 and {max}, got {v}"))
}

/// `bounded` for a `usize` field capped at `u32::MAX`.
fn to_usize(name: &str, v: i64) -> PyResult<usize> {
    let v = bounded(name, v, u32::MAX.into()).map_err(PyValueError::new_err)?;
    Ok(usize::try_from(v).unwrap_or(usize::MAX))
}

/// `bounded` for a `u16` field.
fn to_u16(name: &str, v: i64) -> Result<u16, String> {
    let v = bounded(name, v, u16::MAX.into())?;
    Ok(u16::try_from(v).unwrap_or(u16::MAX))
}

/// Search parameters. Immutable; see `Index.search`.
#[pyclass(module = "keyhammer", frozen, get_all, eq, skip_from_py_object)]
#[derive(Clone, PartialEq, Eq)]
struct SearchConfig {
    /// Number of results wanted.
    k: usize,
    /// Largest accepted edit cost, fixed point (16 = one ordinary edit).
    budget: u16,
    /// Ordering of the results.
    ranking: Ranking,
    /// Use the subtree-signature lower bound (same results, other speed).
    tsb: bool,
    /// Hard limit on expanded trie nodes.
    max_nodes: usize,
}

#[pymethods]
impl SearchConfig {
    #[new]
    #[pyo3(signature = (k=10, budget=32, ranking=Ranking::Coarse, tsb=false, max_nodes=100_000))]
    fn new(k: i64, budget: i64, ranking: Ranking, tsb: bool, max_nodes: i64) -> PyResult<Self> {
        Ok(Self {
            k: to_usize("k", k)?,
            budget: to_u16("budget", budget).map_err(PyValueError::new_err)?,
            ranking,
            tsb,
            max_nodes: to_usize("max_nodes", max_nodes)?,
        })
    }

    /// The opt-in recall preset of the core: budget 48 and `tsb=True`.
    #[staticmethod]
    fn high_recall() -> Self {
        let c = CoreConfig::high_recall();
        Self {
            k: c.k,
            budget: c.budget,
            ranking: Ranking::Coarse,
            tsb: c.tsb,
            max_nodes: c.max_nodes,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "SearchConfig(k={}, budget={}, ranking=Ranking.{}, tsb={}, max_nodes={})",
            self.k,
            self.budget,
            match self.ranking {
                Ranking::Coarse => "COARSE",
                Ranking::Exact => "EXACT",
            },
            if self.tsb { "True" } else { "False" },
            self.max_nodes
        )
    }
}

impl From<&SearchConfig> for CoreConfig {
    fn from(c: &SearchConfig) -> Self {
        CoreConfig {
            k: c.k,
            budget: c.budget,
            tsb: c.tsb,
            max_nodes: c.max_nodes,
            ranking: c.ranking.into(),
        }
    }
}

/// One search result.
#[pyclass(module = "keyhammer", frozen, get_all, eq, skip_from_py_object)]
#[derive(Clone, PartialEq, Eq)]
struct Hit {
    /// The matched term (lower-cased).
    term: String,
    /// Exact weighted edit cost, fixed point (16 = one ordinary edit).
    cost: u16,
    /// The term's weight.
    weight: u16,
}

#[pymethods]
impl Hit {
    fn __repr__(&self) -> String {
        format!(
            "Hit(term={:?}, cost={}, weight={})",
            self.term, self.cost, self.weight
        )
    }
}

/// Hits, best first, plus work counters.
#[pyclass(module = "keyhammer", frozen, get_all)]
struct SearchResult {
    /// The hits, best first.
    hits: Vec<Hit>,
    /// Trie nodes expanded by the search.
    nodes_expanded: usize,
    /// The node limit stopped the search early (the hits may be incomplete).
    truncated: bool,
}

#[pymethods]
impl SearchResult {
    fn __len__(&self) -> usize {
        self.hits.len()
    }

    fn __repr__(&self) -> String {
        format!(
            "SearchResult(hits={}, nodes_expanded={}, truncated={})",
            self.hits.len(),
            self.nodes_expanded,
            if self.truncated { "True" } else { "False" }
        )
    }
}

/// Checks ASCII and lower-cases; the core compares other bytes verbatim.
fn normalise(what: &str, s: &str) -> Result<String, String> {
    if !s.is_ascii() {
        return Err(format!(
            "{what} {s:?} is not ASCII; only ASCII text is supported for now"
        ));
    }
    Ok(s.to_ascii_lowercase())
}

/// An immutable index over `(term, weight)` pairs.
///
/// Immutable after construction, so one instance can be searched from many
/// threads at once; the GIL is released while building and searching.
#[pyclass(module = "keyhammer", frozen)]
struct Index {
    trie: Trie,
    costs: CostModel,
}

#[pymethods]
impl Index {
    #[new]
    fn new(py: Python<'_>, items: &Bound<'_, PyAny>) -> PyResult<Self> {
        let mut owned: Vec<(String, u16)> = Vec::new();
        for item in items.try_iter()? {
            let (term, weight): (String, i64) = item?.extract().map_err(|_| {
                PyTypeError::new_err("items must be (term: str, weight: int) pairs")
            })?;
            let weight = to_u16("weight", weight).map_err(BuildError::new_err)?;
            let term = normalise("term", &term).map_err(BuildError::new_err)?;
            owned.push((term, weight));
        }
        let trie = py
            .detach(|| {
                let refs: Vec<(&str, u16)> = owned.iter().map(|(t, w)| (t.as_str(), *w)).collect();
                Trie::build(&refs)
            })
            .map_err(|e| BuildError::new_err(e.to_string()))?;
        Ok(Self {
            trie,
            costs: CostModel::qwerty(),
        })
    }

    /// Number of distinct terms (duplicates keep the highest weight).
    fn __len__(&self) -> usize {
        self.trie.len()
    }

    /// Searches the index. Keyword arguments override `config`; without
    /// either, the core defaults are used (k=10, budget=32, coarse ranking).
    #[pyo3(signature = (query, k=None, budget=None, ranking=None, *, config=None))]
    fn search(
        &self,
        py: Python<'_>,
        query: &str,
        k: Option<i64>,
        budget: Option<i64>,
        ranking: Option<Ranking>,
        config: Option<&SearchConfig>,
    ) -> PyResult<SearchResult> {
        let mut cfg = config.map_or_else(CoreConfig::default, CoreConfig::from);
        if let Some(k) = k {
            cfg.k = to_usize("k", k)?;
        }
        if let Some(b) = budget {
            cfg.budget = to_u16("budget", b).map_err(PyValueError::new_err)?;
        }
        if let Some(r) = ranking {
            cfg.ranking = r.into();
        }
        let q = normalise("query", query).map_err(PyValueError::new_err)?;
        let out = py
            .detach(|| Searcher::new().search(&self.trie, &self.costs, q.as_bytes(), &cfg))
            .map_err(|e| {
                let msg = e.to_string();
                match e {
                    CoreSearchError::QueryTooLong { .. } => QueryTooLongError::new_err(msg),
                    CoreSearchError::BudgetTooLarge { .. } => BudgetTooLargeError::new_err(msg),
                    _ => SearchError::new_err(msg),
                }
            })?;
        Ok(SearchResult {
            hits: out
                .hits
                .iter()
                .map(|h| Hit {
                    term: self.trie.term(h.id).to_owned(),
                    cost: h.cost,
                    weight: h.weight,
                })
                .collect(),
            nodes_expanded: out.stats.nodes_expanded,
            truncated: out.stats.truncated,
        })
    }

    fn __repr__(&self) -> String {
        format!("Index(<{} terms>)", self.trie.len())
    }
}

/// The compiled module; the public API is re-exported by `keyhammer/__init__.py`.
#[pymodule]
fn _keyhammer(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    m.add_class::<Index>()?;
    m.add_class::<SearchConfig>()?;
    m.add_class::<SearchResult>()?;
    m.add_class::<Hit>()?;
    m.add_class::<Ranking>()?;
    m.add("KeyhammerError", py.get_type::<KeyhammerError>())?;
    m.add("BuildError", py.get_type::<BuildError>())?;
    m.add("SearchError", py.get_type::<SearchError>())?;
    m.add("QueryTooLongError", py.get_type::<QueryTooLongError>())?;
    m.add("BudgetTooLargeError", py.get_type::<BudgetTooLargeError>())?;
    Ok(())
}
