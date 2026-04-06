use napi::bindgen_prelude::*;
use napi_derive::napi;

#[napi(object)]
pub struct JsSearchResult {
    pub term: String,
    pub term_index: u32,
    pub score: f64,
    pub hamming_distance: u32,
}

#[napi]
pub struct KeyhammerIndex {
    inner: keyhammer::FuzzyIndex,
}

#[napi]
impl KeyhammerIndex {
    #[napi(factory)]
    pub fn build(terms: Vec<String>, k: u32) -> Result<Self> {
        let refs: Vec<&str> = terms.iter().map(|s| s.as_str()).collect();
        let inner = keyhammer::FuzzyIndex::build(&refs, k as usize)
            .map_err(|e| Error::from_reason(format!("{e}")))?;
        Ok(Self { inner })
    }

    #[napi]
    pub fn search(&self, query: String, max_results: u32) -> Result<Vec<JsSearchResult>> {
        let results = self.inner.search(&query, max_results as usize)
            .map_err(|e| Error::from_reason(format!("{e}")))?;
        Ok(results.into_iter().map(|r| JsSearchResult {
            term: r.term,
            term_index: r.term_index as u32,
            score: r.score as f64,
            hamming_distance: r.hamming_distance as u32,
        }).collect())
    }

    #[napi]
    pub fn len(&self) -> u32 {
        self.inner.len() as u32
    }
}
