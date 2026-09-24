// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

use napi::bindgen_prelude::*;
use napi_derive::napi;

#[napi(object)]
pub struct JsSearchResult {
    pub term: String,
    pub term_index: u32,
    pub score: f64,
    pub hamming_distance: u32,
    /// Matching character ranges for UI highlighting: [[start, end], ...]
    pub match_ranges: Vec<Vec<u32>>,
}

#[napi(object)]
pub struct JsIndexStats {
    pub num_terms: u32,
    pub max_mismatches: u32,
    pub has_tree: bool,
}

#[napi]
pub struct KeyhammerIndex {
    inner: keyhammer::FuzzyIndex,
}

fn to_js_results(results: Vec<keyhammer::SearchResult>) -> Vec<JsSearchResult> {
    results.into_iter().map(|r| JsSearchResult {
        term: r.term,
        term_index: r.term_index as u32,
        score: r.score as f64,
        hamming_distance: r.hamming_distance as u32,
        match_ranges: r.match_ranges.iter()
            .map(|&(s, e)| vec![s as u32, e as u32])
            .collect(),
    }).collect()
}

#[napi]
impl KeyhammerIndex {
    /// Build with default QWERTY layout.
    #[napi(factory)]
    pub fn build(terms: Vec<String>, k: u32) -> Result<Self> {
        let refs: Vec<&str> = terms.iter().map(|s| s.as_str()).collect();
        let inner = keyhammer::FuzzyIndex::build(&refs, k as usize)
            .map_err(|e| Error::from_reason(format!("{e}")))?;
        Ok(Self { inner })
    }

    /// Build with a specific keyboard layout.
    #[napi(factory)]
    pub fn build_with_layout(terms: Vec<String>, k: u32, layout: String) -> Result<Self> {
        let refs: Vec<&str> = terms.iter().map(|s| s.as_str()).collect();
        let kb = match layout.to_lowercase().as_str() {
            "azerty" => keyhammer::KeyboardLayout::Azerty,
            "qwertz" => keyhammer::KeyboardLayout::Qwertz,
            _ => keyhammer::KeyboardLayout::Qwerty,
        };
        let inner = keyhammer::FuzzyIndex::build_with_layout(&refs, k as usize, kb)
            .map_err(|e| Error::from_reason(format!("{e}")))?;
        Ok(Self { inner })
    }

    /// Reconstruct from exported data.
    #[napi(factory)]
    pub fn import(terms: Vec<String>, k: u32) -> Result<Self> {
        let inner = keyhammer::FuzzyIndex::import(&terms, k as usize)
            .map_err(|e| Error::from_reason(format!("{e}")))?;
        Ok(Self { inner })
    }

    /// Fuzzy search.
    #[napi]
    pub fn search(&self, query: String, max_results: u32) -> Result<Vec<JsSearchResult>> {
        let results = self.inner.search(&query, max_results as usize)
            .map_err(|e| Error::from_reason(format!("{e}")))?;
        Ok(to_js_results(results))
    }

    /// Search with minimum score threshold.
    #[napi]
    pub fn search_with_threshold(&self, query: String, max_results: u32, threshold: f64) -> Result<Vec<JsSearchResult>> {
        let results = self.inner.search_with_threshold(&query, max_results as usize, threshold as f32)
            .map_err(|e| Error::from_reason(format!("{e}")))?;
        Ok(to_js_results(results))
    }

    /// Add a term to the index.
    #[napi]
    pub fn add(&mut self, term: String) {
        self.inner.add(&term);
    }

    /// Remove a term by index.
    #[napi]
    pub fn remove(&mut self, index: u32) {
        self.inner.remove(index as usize);
    }

    /// Export terms and k for serialization.
    #[napi]
    pub fn export(&self) -> Vec<String> {
        let (terms, _k) = self.inner.export();
        terms
    }

    #[napi]
    pub fn len(&self) -> u32 {
        self.inner.len() as u32
    }

    #[napi]
    pub fn stats(&self) -> JsIndexStats {
        let s = self.inner.stats();
        JsIndexStats {
            num_terms: s.num_terms as u32,
            max_mismatches: s.max_mismatches as u32,
            has_tree: s.has_tree,
        }
    }
}
