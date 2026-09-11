use crate::search::tokenizer::Tokenizer;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvertedIndex {
    pub table_column: String,
    /// Term -> HashMap<doc_id, term_frequency>
    pub term_postings: HashMap<String, HashMap<String, usize>>,
    /// doc_id -> doc_length (number of tokens)
    pub doc_lengths: HashMap<String, usize>,
    pub total_docs: usize,
    pub total_tokens: usize,
    pub k1: f32, // Default: 1.2
    pub b: f32,  // Default: 0.75
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub doc_id: String,
    pub score: f32,
}

impl InvertedIndex {
    pub fn new(table_column: String) -> Self {
        Self {
            table_column,
            term_postings: HashMap::new(),
            doc_lengths: HashMap::new(),
            total_docs: 0,
            total_tokens: 0,
            k1: 1.2,
            b: 0.75,
        }
    }

    pub fn add_document(&mut self, doc_id: &str, text: &str) {
        let tokenizer = Tokenizer::new();
        let tokens = tokenizer.tokenize(text);

        // Remove old document entries if updating
        if self.doc_lengths.contains_key(doc_id) {
            self.remove_document(doc_id);
        }

        let doc_len = tokens.len();
        self.doc_lengths.insert(doc_id.to_string(), doc_len);
        self.total_docs += 1;
        self.total_tokens += doc_len;

        let mut freq_map = HashMap::new();
        for token in tokens {
            *freq_map.entry(token).or_insert(0usize) += 1;
        }

        for (term, freq) in freq_map {
            self.term_postings
                .entry(term)
                .or_insert_with(HashMap::new)
                .insert(doc_id.to_string(), freq);
        }
    }

    pub fn remove_document(&mut self, doc_id: &str) {
        if let Some(doc_len) = self.doc_lengths.remove(doc_id) {
            self.total_docs = self.total_docs.saturating_sub(1);
            self.total_tokens = self.total_tokens.saturating_sub(doc_len);

            for postings in self.term_postings.values_mut() {
                postings.remove(doc_id);
            }
            self.term_postings.retain(|_, postings| !postings.is_empty());
        }
    }

    pub fn search(&self, query: &str, top_k: usize) -> Vec<SearchHit> {
        if self.total_docs == 0 {
            return Vec::new();
        }

        let tokenizer = Tokenizer::new();
        let query_terms = tokenizer.tokenize(query);
        if query_terms.is_empty() {
            return Vec::new();
        }

        let avg_dl = if self.total_docs > 0 {
            (self.total_tokens as f32) / (self.total_docs as f32)
        } else {
            1.0
        };

        let mut scores: HashMap<String, f32> = HashMap::new();

        for term in query_terms {
            if let Some(postings) = self.term_postings.get(&term) {
                let df = postings.len() as f32;
                // Standard BM25 IDF
                let idf = (((self.total_docs as f32 - df + 0.5) / (df + 0.5)) + 1.0).ln();
                let idf = idf.max(0.0);

                for (doc_id, &tf) in postings {
                    let doc_len = *self.doc_lengths.get(doc_id).unwrap_or(&1) as f32;
                    let tf = tf as f32;
                    let numerator = tf * (self.k1 + 1.0);
                    let denominator = tf + self.k1 * (1.0 - self.b + self.b * (doc_len / avg_dl));
                    let term_score = idf * (numerator / denominator);

                    *scores.entry(doc_id.clone()).or_insert(0.0) += term_score;
                }
            }
        }

        let mut hits: Vec<SearchHit> = scores
            .into_iter()
            .map(|(doc_id, score)| SearchHit { doc_id, score })
            .collect();

        hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        hits.truncate(top_k);
        hits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bm25_search() {
        let mut index = InvertedIndex::new("articles__content".to_string());
        index.add_document("doc1", "The artificial intelligence revolution is transforming databases.");
        index.add_document("doc2", "Relational databases use B-Trees for standard indexing.");
        index.add_document("doc3", "Artificial intelligence agents require fast vector and database memory.");

        let hits = index.search("artificial intelligence database", 3);
        assert!(!hits.is_empty());
        assert_eq!(hits[0].doc_id, "doc3");
    }
}
