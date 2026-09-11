use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HybridSearchResult {
    pub doc_id: String,
    pub combined_score: f32,
    pub vector_score: f32,
    pub text_score: f32,
}

/// Combines vector similarity scores and BM25 full-text scores
/// using normalized alpha weighting: combined = alpha * vector_score + (1.0 - alpha) * text_score
pub fn combine_hybrid_scores(
    vector_results: &[(String, f32)], // (doc_id, similarity)
    text_results: &[(String, f32)],   // (doc_id, bm25_score)
    alpha: f32,                       // 0.0 = 100% text, 1.0 = 100% vector, default: 0.5
    top_k: usize,
) -> Vec<HybridSearchResult> {
    let alpha = alpha.clamp(0.0, 1.0);
    
    // Normalize BM25 text scores into [0, 1] range
    let max_text = text_results.iter().map(|(_, s)| *s).fold(0.0f32, f32::max);
    let norm_factor = if max_text > 0.0 { max_text } else { 1.0 };

    let mut doc_scores: HashMap<String, (f32, f32)> = HashMap::new(); // doc_id -> (vector_score, text_score)

    for (id, v_score) in vector_results {
        let entry = doc_scores.entry(id.clone()).or_insert((0.0, 0.0));
        entry.0 = *v_score;
    }

    for (id, t_score) in text_results {
        let entry = doc_scores.entry(id.clone()).or_insert((0.0, 0.0));
        entry.1 = *t_score / norm_factor;
    }

    let mut combined: Vec<HybridSearchResult> = doc_scores
        .into_iter()
        .map(|(doc_id, (v_score, t_score))| {
            let combined_score = (alpha * v_score) + ((1.0 - alpha) * t_score);
            HybridSearchResult {
                doc_id,
                combined_score,
                vector_score: v_score,
                text_score: t_score,
            }
        })
        .collect();

    combined.sort_by(|a, b| b.combined_score.partial_cmp(&a.combined_score).unwrap_or(std::cmp::Ordering::Equal));
    combined.truncate(top_k);
    combined
}
