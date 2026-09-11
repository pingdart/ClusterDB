use crate::storage::StorageEngine;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationReport {
    pub job_id: String,
    pub source_rows: usize,
    pub target_rows: usize,
    pub row_count_match: bool,
    pub null_violations_count: usize,
    pub pk_violations_count: usize,
    pub checksum_match: bool,
    pub source_checksum: String,
    pub target_checksum: String,
    pub is_valid: bool,
    pub details: Vec<String>,
}

pub struct MigrationValidator;

impl MigrationValidator {
    pub fn validate(
        job_id: &str,
        storage: &StorageEngine,
        target_table: &str,
        source_rows: usize,
        source_checksum: &str,
        primary_key_col: Option<&str>,
        not_null_cols: &[String],
    ) -> Result<ValidationReport> {
        let mut target_rows = 0;
        let mut null_violations_count = 0;
        let mut pk_violations_count = 0;
        let mut details = Vec::new();

        let mut hasher = Sha256::new();
        let mut seen_pks = std::collections::HashSet::new();

        if let Ok(raw_pairs) = storage.get_raw_pairs(target_table) {
            target_rows = raw_pairs.len();
            for (k, v) in raw_pairs {
                hasher.update(&k);
                hasher.update(&v);

                if let Ok(row) = serde_json::from_slice::<crate::exec::RowData>(&v) {
                    // PK Check
                    if let Some(pk_name) = primary_key_col {
                        if let Some(pk_val) = row.columns.get(pk_name) {
                            if !seen_pks.insert(pk_val.clone()) {
                                pk_violations_count += 1;
                                if details.len() < 10 {
                                    details.push(format!("Duplicate primary key '{}' found in row", pk_val));
                                }
                            }
                        } else {
                            pk_violations_count += 1;
                        }
                    }

                    // Null constraints
                    for nn in not_null_cols {
                        if let Some(val) = row.columns.get(nn) {
                            if val.is_empty() {
                                null_violations_count += 1;
                            }
                        } else {
                            null_violations_count += 1;
                        }
                    }
                }
            }
        }

        let target_checksum = hex::encode(hasher.finalize());
        let row_count_match = source_rows == target_rows;
        let checksum_match = source_checksum.is_empty() || source_checksum == target_checksum;

        let is_valid = row_count_match && null_violations_count == 0 && pk_violations_count == 0;

        if !row_count_match {
            details.push(format!("Row count mismatch: source had {} rows, target has {} rows", source_rows, target_rows));
        }

        Ok(ValidationReport {
            job_id: job_id.to_string(),
            source_rows,
            target_rows,
            row_count_match,
            null_violations_count,
            pk_violations_count,
            checksum_match,
            source_checksum: source_checksum.to_string(),
            target_checksum,
            is_valid,
            details,
        })
    }
}
