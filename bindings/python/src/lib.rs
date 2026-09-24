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
//! Terms and queries are any Unicode text. They are normalised identically
//! with the core's `keyhammer::text::Normalizer` (by default case and
//! diacritics are folded: `São Paulo` is found by `sao paulo`, `ACAO` finds
//! `ação`; `Index(..., fold_case=False)` and `fold_diacritics=False` turn a
//! folding off). A hit's `term` is the text the caller gave (the entry that
//! was kept among terms equal after normalisation) and `index` its position
//! in `items`. Strings with lone surrogates cannot be encoded as UTF-8 and are
//! rejected.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use keyhammer::cost::CostModel;
use keyhammer::search::{
    Ranking as CoreRanking, SearchConfig as CoreConfig, SearchError as CoreSearchError, Searcher,
};
use keyhammer::text::Normalizer;
use keyhammer::trie::Trie;
use pyo3::create_exception;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyInt, PyString};

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
    "The query is longer than 128 code points (counted after normalisation)."
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

impl From<CoreRanking> for Ranking {
    fn from(r: CoreRanking) -> Self {
        match r {
            CoreRanking::Exact => Ranking::Exact,
            _ => Ranking::Coarse,
        }
    }
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
    let v = bounded(name, v, u32::MAX.into()).map_err(KeyhammerError::new_err)?;
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
            budget: to_u16("budget", budget).map_err(KeyhammerError::new_err)?,
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
            ranking: c.ranking.into(),
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
#[pyclass(module = "keyhammer", frozen, get_all, eq, hash, skip_from_py_object)]
#[derive(Clone, PartialEq, Eq, Hash)]
struct Hit {
    /// The matched term as it was given to `Index` (original case and
    /// accents, not the normalised form).
    term: String,
    /// Exact weighted edit cost, fixed point (16 = one ordinary edit).
    cost: u16,
    /// The term's weight.
    weight: u16,
    /// Position in `items` of the entry that was kept (highest weight, first
    /// among ties). Terms equal after normalisation are merged, so this is
    /// the way to map a hit back to the caller's record.
    index: u32,
}

#[pymethods]
impl Hit {
    fn __repr__(&self) -> String {
        format!(
            "Hit(term={:?}, cost={}, weight={}, index={})",
            self.term, self.cost, self.weight, self.index
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

/// An immutable index over `(term, weight)` pairs.
///
/// Immutable after construction, so one instance can be searched from many
/// threads at once; the GIL is released while building and searching.
#[pyclass(module = "keyhammer", frozen)]
struct Index {
    trie: Trie,
    /// The terms as given, by position in `items` (`Trie::input_index`).
    originals: Vec<String>,
    costs: CostModel,
}

#[pymethods]
impl Index {
    /// `fold_case` and `fold_diacritics` (both on by default) choose the
    /// normalisation applied to terms and queries alike.
    #[new]
    #[pyo3(signature = (items, *, fold_case=true, fold_diacritics=true))]
    fn new(
        py: Python<'_>,
        items: &Bound<'_, PyAny>,
        fold_case: bool,
        fold_diacritics: bool,
    ) -> PyResult<Self> {
        let mut owned: Vec<(String, u16)> = Vec::new();
        for item in items.try_iter()? {
            let (term, weight): (Bound<'_, PyAny>, Bound<'_, PyAny>) =
                item?.extract().map_err(|_| {
                    PyTypeError::new_err("items must be (term: str, weight: int) pairs")
                })?;
            let term: String = term.extract().map_err(|e| {
                if term.is_instance_of::<PyString>() {
                    BuildError::new_err(
                        "term is not valid Unicode text (for example a lone surrogate) and cannot be encoded as UTF-8",
                    )
                } else {
                    e
                }
            })?;
            let weight = match weight.extract::<i64>() {
                Ok(w) => w,
                Err(_) if weight.is_instance_of::<PyInt>() => {
                    return Err(BuildError::new_err(
                        "weight must be between 0 and 65535 (integer out of range)",
                    ));
                }
                Err(e) => return Err(e),
            };
            let weight = to_u16("weight", weight).map_err(BuildError::new_err)?;
            owned.push((term, weight));
        }
        let normalizer = Normalizer::new()
            .with_case_folding(fold_case)
            .with_diacritic_folding(fold_diacritics);
        let trie = py
            .detach(|| {
                let refs: Vec<(&str, u16)> = owned.iter().map(|(t, w)| (t.as_str(), *w)).collect();
                Trie::build_normalized(&refs, &normalizer)
            })
            .map_err(|e| BuildError::new_err(e.to_string()))?;
        Ok(Self {
            trie,
            originals: owned.into_iter().map(|(t, _)| t).collect(),
            costs: CostModel::qwerty(),
        })
    }

    /// Number of distinct terms (duplicates keep the highest weight).
    fn __len__(&self) -> usize {
        self.trie.len()
    }

    /// Searches the index. The query is normalised like the terms. Keyword
    /// arguments override `config`; without either, the core defaults are
    /// used (k=10, budget=32, coarse ranking).
    #[pyo3(signature = (query, k=None, budget=None, ranking=None, *, config=None))]
    fn search(
        &self,
        py: Python<'_>,
        query: &Bound<'_, PyAny>,
        k: Option<i64>,
        budget: Option<i64>,
        ranking: Option<Ranking>,
        config: Option<&SearchConfig>,
    ) -> PyResult<SearchResult> {
        let mut cfg = config.map_or_else(CoreConfig::default, CoreConfig::from);
        if let Some(k) = k {
            cfg.k = to_usize("k", k).map_err(|e| SearchError::new_err(e.value(py).to_string()))?;
        }
        if let Some(b) = budget {
            cfg.budget = to_u16("budget", b).map_err(|m| {
                if b > 0 {
                    BudgetTooLargeError::new_err(m)
                } else {
                    SearchError::new_err(m)
                }
            })?;
        }
        if let Some(r) = ranking {
            cfg.ranking = r.into();
        }
        let query: String = query.extract().map_err(|e| {
            if query.is_instance_of::<PyString>() {
                SearchError::new_err(
                    "query is not valid Unicode text (for example a lone surrogate) and cannot be encoded as UTF-8",
                )
            } else {
                e
            }
        })?;
        let out = py
            .detach(|| Searcher::new().search_text(&self.trie, &self.costs, &query, &cfg))
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
                    term: self
                        .originals
                        .get(self.trie.input_index(h.id) as usize)
                        .map_or_else(|| self.trie.term(h.id), String::as_str)
                        .to_owned(),
                    cost: h.cost,
                    weight: h.weight,
                    index: self.trie.input_index(h.id),
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
