use serde::{Deserialize, Serialize};

/// Supported and validated vector dimensions for embeddings
pub const VALID_DIMENSIONS: &[usize] = &[384, 768, 1024, 1536, 3072];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum VectorMetric {
    Cosine,
    Euclidean,
    DotProduct,
}

impl Default for VectorMetric {
    fn default() -> Self {
        VectorMetric::Cosine
    }
}

impl std::str::FromStr for VectorMetric {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "cosine" => Ok(VectorMetric::Cosine),
            "euclidean" | "l2" => Ok(VectorMetric::Euclidean),
            "dot" | "dot_product" | "inner_product" => Ok(VectorMetric::DotProduct),
            _ => Err(format!("Unknown vector distance metric: {}", s)),
        }
    }
}

/// A dense vector representation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Vector(pub Vec<f32>);

impl Vector {
    pub fn new(values: Vec<f32>) -> Self {
        Self(values)
    }

    pub fn dim(&self) -> usize {
        self.0.len()
    }

    pub fn is_valid_dimension(&self) -> bool {
        VALID_DIMENSIONS.contains(&self.dim())
    }

    pub fn normalize(&self) -> Self {
        let norm: f32 = self.0.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm == 0.0 {
            return self.clone();
        }
        Self(self.0.iter().map(|x| x / norm).collect())
    }
}
