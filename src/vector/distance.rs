use crate::vector::types::VectorMetric;

/// Calculates distance/dissimilarity between two vectors based on the selected metric.
/// For Cosine: returns 1.0 - cosine_similarity (0.0 = identical, 2.0 = opposite).
/// For Euclidean: returns L2 distance (>= 0.0).
/// For DotProduct: returns negative dot product (so smaller = closer / higher similarity).
pub fn compute_distance(a: &[f32], b: &[f32], metric: &VectorMetric) -> f32 {
    assert_eq!(a.len(), b.len(), "Vector dimensions must match for distance calculation");
    match metric {
        VectorMetric::Cosine => cosine_distance(a, b),
        VectorMetric::Euclidean => euclidean_distance(a, b),
        VectorMetric::DotProduct => dot_product_distance(a, b),
    }
}

/// Similarity score (higher = closer, typically between -1.0 and 1.0 for cosine, or normalized for others)
pub fn compute_similarity(a: &[f32], b: &[f32], metric: &VectorMetric) -> f32 {
    assert_eq!(a.len(), b.len(), "Vector dimensions must match for similarity calculation");
    match metric {
        VectorMetric::Cosine => cosine_similarity(a, b),
        VectorMetric::Euclidean => {
            let dist = euclidean_distance(a, b);
            1.0 / (1.0 + dist) // Normalize to [0, 1]
        }
        VectorMetric::DotProduct => dot_product(a, b),
    }
}

pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;

    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }

    let denom = (norm_a.sqrt()) * (norm_b.sqrt());
    if denom == 0.0 {
        0.0
    } else {
        dot / denom
    }
}

pub fn cosine_distance(a: &[f32], b: &[f32]) -> f32 {
    1.0 - cosine_similarity(a, b)
}

pub fn euclidean_distance(a: &[f32], b: &[f32]) -> f32 {
    let sum_sq: f32 = a.iter().zip(b.iter()).map(|(x, y)| {
        let diff = x - y;
        diff * diff
    }).sum();
    sum_sq.sqrt()
}

pub fn dot_product(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

pub fn dot_product_distance(a: &[f32], b: &[f32]) -> f32 {
    -dot_product(a, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - 1.0).abs() < 1e-5);

        let c = vec![0.0, 1.0, 0.0];
        let sim_ortho = cosine_similarity(&a, &c);
        assert!((sim_ortho - 0.0).abs() < 1e-5);
    }

    #[test]
    fn test_euclidean_distance() {
        let a = vec![0.0, 0.0, 0.0];
        let b = vec![3.0, 4.0, 0.0];
        let dist = euclidean_distance(&a, &b);
        assert!((dist - 5.0).abs() < 1e-5);
    }
}
