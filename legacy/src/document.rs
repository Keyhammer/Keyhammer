/// Multi-field document support.
///
/// Allows indexing objects with multiple searchable fields, each with a weight.
///
/// # Example
///
/// ```
/// use keyhammer_legacy::DocumentIndex;
///
/// let docs = vec![
///     vec![("name", "JavaScript"), ("category", "language")],
///     vec![("name", "TypeScript"), ("category", "language")],
///     vec![("name", "React"), ("category", "framework")],
/// ];
///
/// let index = DocumentIndex::builder()
///     .key("name", 2.0)
///     .key("category", 1.0)
///     .k(2)
///     .build(&docs)
///     .unwrap();
///
/// let results = index.search("javscript", 5).unwrap();
/// assert_eq!(results[0].doc_index, 0);
/// ```

use crate::error::{Error, Result};
use crate::index::FuzzyIndex;
use crate::scorer::KeyboardLayout;

/// A search result from a multi-field document search.
#[derive(Debug, Clone)]
pub struct DocSearchResult {
    /// Index of the document in the original list.
    pub doc_index: usize,
    /// Combined score across all matched fields.
    pub score: f32,
    /// Which fields matched and their individual scores.
    pub field_matches: Vec<FieldMatch>,
}

#[derive(Debug, Clone)]
pub struct FieldMatch {
    pub key: String,
    pub term: String,
    pub score: f32,
    pub match_indices: Vec<(usize, usize)>,
}

/// Configuration for a searchable field.
struct FieldConfig {
    key: String,
    weight: f32,
}

/// Builder for `DocumentIndex`.
pub struct DocumentIndexBuilder {
    fields: Vec<FieldConfig>,
    k: usize,
    layout: KeyboardLayout,
    threshold: f32,
}

impl DocumentIndexBuilder {
    pub fn new() -> Self {
        Self {
            fields: Vec::new(),
            k: 2,
            layout: KeyboardLayout::Qwerty,
            threshold: 0.0,
        }
    }

    /// Add a searchable field with a weight. Higher weight = more influence on score.
    pub fn key(mut self, name: &str, weight: f32) -> Self {
        self.fields.push(FieldConfig {
            key: name.to_string(),
            weight,
        });
        self
    }

    /// Max mismatches for fuzzy matching (default: 2).
    pub fn k(mut self, k: usize) -> Self {
        self.k = k;
        self
    }

    /// Keyboard layout for confusion-aware scoring.
    pub fn layout(mut self, layout: KeyboardLayout) -> Self {
        self.layout = layout;
        self
    }

    /// Minimum score threshold. Results below this are discarded (default: 0.0).
    pub fn threshold(mut self, threshold: f32) -> Self {
        self.threshold = threshold;
        self
    }

    /// Build the index from a list of documents.
    /// Each document is a list of (key, value) pairs.
    pub fn build(self, docs: &[Vec<(&str, &str)>]) -> Result<DocumentIndex> {
        if docs.is_empty() {
            return Err(Error::EmptyInput);
        }
        if self.fields.is_empty() {
            return Err(Error::EmptyInput);
        }

        // build a separate FuzzyIndex per field
        let mut field_indices = Vec::with_capacity(self.fields.len());

        for field in &self.fields {
            // extract this field's values from all docs
            let mut values: Vec<&str> = Vec::with_capacity(docs.len());
            let mut doc_map: Vec<usize> = Vec::with_capacity(docs.len());

            for (doc_idx, doc) in docs.iter().enumerate() {
                for (k, v) in doc {
                    if *k == field.key {
                        values.push(v);
                        doc_map.push(doc_idx);
                    }
                }
            }

            let index = if values.is_empty() {
                None
            } else {
                Some(FuzzyIndex::build_with_layout(&values, self.k, self.layout)?)
            };

            field_indices.push(FieldIndex {
                key: field.key.clone(),
                weight: field.weight,
                index,
                doc_map,
            });
        }

        Ok(DocumentIndex {
            num_docs: docs.len(),
            fields: field_indices,
            threshold: self.threshold,
        })
    }
}

struct FieldIndex {
    key: String,
    weight: f32,
    index: Option<FuzzyIndex>,
    doc_map: Vec<usize>, // term_index → doc_index
}

pub struct DocumentIndex {
    num_docs: usize,
    fields: Vec<FieldIndex>,
    threshold: f32,
}

impl DocumentIndex {
    pub fn builder() -> DocumentIndexBuilder {
        DocumentIndexBuilder::new()
    }

    /// Search across all fields. Returns results sorted by combined weighted score.
    pub fn search(&self, query: &str, max_results: usize) -> Result<Vec<DocSearchResult>> {
        // accumulate scores per document
        let mut doc_scores: Vec<f32> = vec![0.0; self.num_docs];
        let mut doc_matches: Vec<Vec<FieldMatch>> = (0..self.num_docs)
            .map(|_| Vec::new())
            .collect();

        let total_weight: f32 = self.fields.iter().map(|f| f.weight).sum();

        for field in &self.fields {
            let index = match &field.index {
                Some(i) => i,
                None => continue,
            };

            let hits = index.search(query, max_results * 2)?;
            let norm_weight = field.weight / total_weight;

            for hit in hits {
                let doc_idx = field.doc_map[hit.term_index];
                doc_scores[doc_idx] += hit.score * norm_weight;
                doc_matches[doc_idx].push(FieldMatch {
                    key: field.key.clone(),
                    term: hit.term,
                    score: hit.score,
                    match_indices: Vec::new(), // TODO: populate when highlighting is implemented
                });
            }
        }

        let mut results: Vec<DocSearchResult> = doc_scores.iter()
            .enumerate()
            .filter(|(_, score)| **score > self.threshold)
            .map(|(i, &score)| DocSearchResult {
                doc_index: i,
                score,
                field_matches: std::mem::take(&mut doc_matches[i]),
            })
            .collect();

        results.sort_unstable_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(max_results);
        Ok(results)
    }

    pub fn len(&self) -> usize {
        self.num_docs
    }

    pub fn is_empty(&self) -> bool {
        self.num_docs == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_docs() -> Vec<Vec<(&'static str, &'static str)>> {
        vec![
            vec![("name", "JavaScript"), ("category", "language")],
            vec![("name", "TypeScript"), ("category", "language")],
            vec![("name", "React"), ("category", "framework")],
            vec![("name", "Vue"), ("category", "framework")],
            vec![("name", "Python"), ("category", "language")],
        ]
    }

    #[test]
    fn basic_doc_search() {
        let index = DocumentIndex::builder()
            .key("name", 2.0)
            .key("category", 1.0)
            .k(2)
            .build(&sample_docs())
            .unwrap();

        let results = index.search("javascript", 5).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].doc_index, 0); // JavaScript
    }

    #[test]
    fn weighted_fields() {
        let index = DocumentIndex::builder()
            .key("name", 10.0)     // name matters much more
            .key("category", 1.0)
            .k(2)
            .build(&sample_docs())
            .unwrap();

        let results = index.search("react", 5).unwrap();
        assert_eq!(results[0].doc_index, 2); // React (name match, high weight)
    }

    #[test]
    fn category_search() {
        let index = DocumentIndex::builder()
            .key("name", 1.0)
            .key("category", 1.0)
            .k(2)
            .build(&sample_docs())
            .unwrap();

        let results = index.search("framework", 5).unwrap();
        assert!(results.len() >= 2); // React + Vue
    }

    #[test]
    fn typo_in_field() {
        let index = DocumentIndex::builder()
            .key("name", 1.0)
            .k(2)
            .build(&sample_docs())
            .unwrap();

        let results = index.search("javscript", 5).unwrap();
        assert!(!results.is_empty());
    }

    #[test]
    fn threshold_filters() {
        let index = DocumentIndex::builder()
            .key("name", 1.0)
            .k(2)
            .threshold(0.9)
            .build(&sample_docs())
            .unwrap();

        // high threshold — only very close matches
        let results = index.search("zzzzz", 5).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn field_matches_reported() {
        let index = DocumentIndex::builder()
            .key("name", 1.0)
            .key("category", 1.0)
            .k(2)
            .build(&sample_docs())
            .unwrap();

        let results = index.search("language", 5).unwrap();
        assert!(!results.is_empty());
        // should have field_matches indicating which field matched
        assert!(results[0].field_matches.iter().any(|fm| fm.key == "category"));
    }
}
