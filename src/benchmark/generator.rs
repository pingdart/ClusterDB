use rand::Rng;

pub struct SyntheticGenerator;

impl SyntheticGenerator {
    /// Generates a normalized unit vector in N dimensions
    pub fn generate_vector(dims: usize) -> Vec<f32> {
        let mut rng = rand::thread_rng();
        let mut vec: Vec<f32> = (0..dims).map(|_| rng.gen_range(-1.0..1.0)).collect();
        let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for v in &mut vec {
                *v /= norm;
            }
        }
        vec
    }

    /// Formats a vector as a SQL array string `[0.123, -0.456, ...]`
    pub fn format_vector_sql(vec: &[f32]) -> String {
        let items: Vec<String> = vec.iter().map(|f| format!("{:.6}", f)).collect();
        format!("[{}]", items.join(", "))
    }

    /// Generates synthetic relational order records
    pub fn generate_order_row(id: usize) -> (usize, usize, String, f64, String) {
        let mut rng = rand::thread_rng();
        let user_id = (id % 10_000) + 1;
        let categories = ["Electronics", "Fashion", "Home", "Books", "Automotive", "Industrial"];
        let category = categories[rng.gen_range(0..categories.len())].to_string();
        let amount = rng.gen_range(10.0..5000.0);
        let status = if rng.gen_bool(0.85) { "COMPLETED" } else { "PENDING" }.to_string();
        (id, user_id, category, amount, status)
    }

    /// Generates an INSERT SQL query for an order
    pub fn generate_order_insert_sql(id: usize) -> String {
        let (order_id, user_id, category, amount, status) = Self::generate_order_row(id);
        format!(
            "INSERT INTO benchmark_orders VALUES ({}, {}, '{}', {:.2}, '{}');",
            order_id, user_id, category, amount, status
        )
    }

    /// Generates a batch of synthetic order rows as HashMaps for high-throughput columnar ingestion
    pub fn generate_order_batch(start_id: usize, count: usize) -> Vec<std::collections::HashMap<String, String>> {
        use std::collections::HashMap;
        let mut rng = rand::thread_rng();
        let categories = ["Electronics", "Fashion", "Home", "Books", "Automotive", "Industrial"];
        let mut batch = Vec::with_capacity(count);

        for i in 0..count {
            let id = start_id + i;
            let user_id = (id % 10_000) + 1;
            let category = categories[rng.gen_range(0..categories.len())];
            let amount = rng.gen_range(10.0..5000.0);
            let status = if rng.gen_bool(0.85) { "COMPLETED" } else { "PENDING" };

            let mut row = HashMap::with_capacity(5);
            row.insert("id".to_string(), id.to_string());
            row.insert("user_id".to_string(), user_id.to_string());
            row.insert("category".to_string(), category.to_string());
            row.insert("amount".to_string(), format!("{:.2}", amount));
            row.insert("status".to_string(), status.to_string());
            batch.push(row);
        }
        batch
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vector_generation_and_normalization() {
        let vec = SyntheticGenerator::generate_vector(384);
        assert_eq!(vec.len(), 384);
        let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-4);
    }

    #[test]
    fn test_order_generation() {
        let (id, user_id, category, amount, status) = SyntheticGenerator::generate_order_row(42);
        assert_eq!(id, 42);
        assert!(user_id > 0);
        assert!(!category.is_empty());
        assert!(amount >= 10.0);
        assert!(status == "COMPLETED" || status == "PENDING");
    }
}
