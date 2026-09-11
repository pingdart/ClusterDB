use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalibratedCostWeights {
    pub cost_per_1k_seq_scan_ms: f64,
    pub cost_btree_lookup_ms: f64,
    pub cost_vector_dist_calc_ms: f64,
    pub cost_columnar_agg_chunk_ms: f64,
    pub cost_network_hop_ms: f64,
    pub timestamp_epoch_secs: u64,
}

impl CalibratedCostWeights {
    pub fn default_calibrated() -> Self {
        Self {
            cost_per_1k_seq_scan_ms: 0.0035, // ~3.5 microseconds per scanned row
            cost_btree_lookup_ms: 0.045,     // ~45 microseconds per BTree point lookup
            cost_vector_dist_calc_ms: 0.00065,// ~650 nanoseconds per 384-dim dot-product
            cost_columnar_agg_chunk_ms: 0.012,// ~12 microseconds per 1,000 vector chunk items
            cost_network_hop_ms: 1.2,        // ~1.2 milliseconds per remote RPC hop
            timestamp_epoch_secs: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }

    pub fn calibrate_from_measurements(
        seq_scan_rows: usize,
        seq_scan_total_ms: f64,
        btree_lookups: usize,
        btree_total_ms: f64,
        vector_comps: usize,
        vector_total_ms: f64,
        columnar_rows: usize,
        columnar_total_ms: f64,
    ) -> Self {
        let cost_per_1k_seq_scan_ms = if seq_scan_rows > 0 {
            (seq_scan_total_ms / seq_scan_rows as f64) * 1000.0
        } else {
            0.0035
        };

        let cost_btree_lookup_ms = if btree_lookups > 0 {
            btree_total_ms / btree_lookups as f64
        } else {
            0.045
        };

        let cost_vector_dist_calc_ms = if vector_comps > 0 {
            vector_total_ms / vector_comps as f64
        } else {
            0.00065
        };

        let cost_columnar_agg_chunk_ms = if columnar_rows > 0 {
            (columnar_total_ms / columnar_rows as f64) * 1000.0
        } else {
            0.012
        };

        Self {
            cost_per_1k_seq_scan_ms,
            cost_btree_lookup_ms,
            cost_vector_dist_calc_ms,
            cost_columnar_agg_chunk_ms,
            cost_network_hop_ms: 1.2,
            timestamp_epoch_secs: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }
}
