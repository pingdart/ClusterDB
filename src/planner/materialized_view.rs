use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterializedViewMetadata {
    pub view_name: String,
    pub query: String,
    pub base_tables: Vec<String>,
    pub created_at: u64,
    pub last_refreshed_at: u64,
    pub row_count: usize,
}

pub struct MaterializedViewManager {
    metadata: RwLock<HashMap<String, MaterializedViewMetadata>>,
    cached_data: RwLock<HashMap<String, Vec<HashMap<String, String>>>>,
}

impl MaterializedViewManager {
    pub fn new() -> Self {
        Self {
            metadata: RwLock::new(HashMap::new()),
            cached_data: RwLock::new(HashMap::new()),
        }
    }

    /// Registers and populates a new Materialized View
    pub fn create_materialized_view(
        &self,
        view_name: &str,
        query: &str,
        base_tables: Vec<String>,
        rows: Vec<HashMap<String, String>>,
    ) -> Result<()> {
        let mut meta_map = self.metadata.write().unwrap();
        if meta_map.contains_key(view_name) {
            bail!("Materialized view '{}' already exists", view_name);
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let row_count = rows.len();
        meta_map.insert(
            view_name.to_string(),
            MaterializedViewMetadata {
                view_name: view_name.to_string(),
                query: query.trim().to_string(),
                base_tables,
                created_at: now,
                last_refreshed_at: now,
                row_count,
            },
        );

        let mut data_map = self.cached_data.write().unwrap();
        data_map.insert(view_name.to_string(), rows);

        Ok(())
    }

    /// Refreshes the cached data of an existing Materialized View
    pub fn refresh_materialized_view(
        &self,
        view_name: &str,
        new_rows: Vec<HashMap<String, String>>,
    ) -> Result<usize> {
        let mut meta_map = self.metadata.write().unwrap();
        let meta = meta_map.get_mut(view_name).ok_or_else(|| anyhow::anyhow!("View not found"))?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        meta.last_refreshed_at = now;
        meta.row_count = new_rows.len();

        let mut data_map = self.cached_data.write().unwrap();
        data_map.insert(view_name.to_string(), new_rows);

        Ok(meta.row_count)
    }

    /// Fetches rows from the Materialized View in O(1) memory lookup
    pub fn get_view_data(&self, view_name: &str) -> Option<Vec<HashMap<String, String>>> {
        let data_map = self.cached_data.read().unwrap();
        data_map.get(view_name).cloned()
    }

    /// Optimizer Query Rewrite: Checks if incoming query matches an existing materialized view
    /// and returns the view name to scan directly instead of the base tables.
    pub fn try_rewrite_query(&self, incoming_query: &str) -> Option<String> {
        let normalized = incoming_query.trim().trim_end_matches(';').to_lowercase();
        let meta_map = self.metadata.read().unwrap();

        for (name, meta) in meta_map.iter() {
            let meta_q = meta.query.trim().trim_end_matches(';').to_lowercase();
            if meta_q == normalized {
                return Some(name.clone());
            }
        }
        None
    }
}

impl Default for MaterializedViewManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_materialized_view_create_refresh_and_query_rewrite() {
        let mgr = MaterializedViewManager::new();

        let mut rows = Vec::new();
        let mut r1 = HashMap::new();
        r1.insert("dept".into(), "Engineering".into());
        r1.insert("total_salary".into(), "500000".into());
        rows.push(r1);

        let query = "SELECT dept, SUM(salary) AS total_salary FROM employees GROUP BY dept";
        mgr.create_materialized_view(
            "mv_dept_salaries",
            query,
            vec!["employees".into()],
            rows,
        ).unwrap();

        // 1. Verify data lookup
        let cached = mgr.get_view_data("mv_dept_salaries").unwrap();
        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0].get("total_salary").unwrap(), "500000");

        // 2. Query rewrite match
        let rewritten_target = mgr.try_rewrite_query("SELECT dept, SUM(salary) AS total_salary FROM employees GROUP BY dept;");
        assert_eq!(rewritten_target, Some("mv_dept_salaries".into()));

        // 3. Refresh with updated row
        let mut updated = Vec::new();
        let mut r2 = HashMap::new();
        r2.insert("dept".into(), "Engineering".into());
        r2.insert("total_salary".into(), "650000".into());
        updated.push(r2);

        mgr.refresh_materialized_view("mv_dept_salaries", updated).unwrap();
        let refreshed = mgr.get_view_data("mv_dept_salaries").unwrap();
        assert_eq!(refreshed[0].get("total_salary").unwrap(), "650000");
    }
}
