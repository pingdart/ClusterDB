use crate::storage::StorageEngine;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::RwLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tenant {
    pub id: String,
    pub name: String,
    pub max_storage_bytes: u64,
    pub max_vectors: usize,
    pub max_qps: u32,
    pub current_storage_bytes: u64,
    pub current_vectors: usize,
    pub rls_enabled: bool,
    pub created_at: i64,
}

impl Tenant {
    pub fn new(id: String, name: String, max_storage_bytes: u64, max_vectors: usize, max_qps: u32) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        Self {
            id,
            name,
            max_storage_bytes,
            max_vectors,
            max_qps,
            current_storage_bytes: 0,
            current_vectors: 0,
            rls_enabled: false,
            created_at: now,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RlsPolicy {
    pub name: String,
    pub table: String,
    pub command: String, // "ALL", "SELECT", "INSERT", "UPDATE", "DELETE"
    pub using_expr: String,
}

pub struct TenantManager {
    storage: StorageEngine,
    tenants: RwLock<HashMap<String, Tenant>>,
    rls_tables: RwLock<HashMap<String, bool>>, // table_name -> is_rls_enabled
    policies: RwLock<HashMap<String, Vec<RlsPolicy>>>, // table_name -> policies
    request_timestamps: RwLock<HashMap<String, VecDeque<Instant>>>, // tenant_id -> timestamps for rate limiting
}

impl TenantManager {
    pub fn new(storage: StorageEngine) -> Self {
        // Load existing tenants from storage
        let mut tenants = HashMap::new();
        if let Ok(Some(saved_tenants)) = storage.get::<HashMap<String, Tenant>>("_system", "tenants") {
            tenants = saved_tenants;
        }

        // Initialize default tenant if none exists
        if !tenants.contains_key("default") {
            let default_tenant = Tenant::new(
                "default".to_string(),
                "Default Enterprise Tenant".to_string(),
                10 * 1024 * 1024 * 1024, // 10 GB
                10_000_000,              // 10M vectors
                10_000,                  // 10K QPS
            );
            tenants.insert("default".to_string(), default_tenant);
            let _ = storage.set("_system", "tenants", &tenants);
        }

        // Load RLS states
        let mut rls_tables = HashMap::new();
        if let Ok(Some(saved_rls)) = storage.get::<HashMap<String, bool>>("_system", "rls_tables") {
            rls_tables = saved_rls;
        }

        // Load policies
        let mut policies = HashMap::new();
        if let Ok(Some(saved_policies)) = storage.get::<HashMap<String, Vec<RlsPolicy>>>("_system", "rls_policies") {
            policies = saved_policies;
        }

        Self {
            storage,
            tenants: RwLock::new(tenants),
            rls_tables: RwLock::new(rls_tables),
            policies: RwLock::new(policies),
            request_timestamps: RwLock::new(HashMap::new()),
        }
    }

    pub fn create_tenant(
        &self,
        id: String,
        name: String,
        max_storage_bytes: u64,
        max_vectors: usize,
        max_qps: u32,
    ) -> Result<Tenant> {
        let mut tenants = self.tenants.write().unwrap();
        if tenants.contains_key(&id) {
            bail!("Tenant with ID '{}' already exists", id);
        }

        let tenant = Tenant::new(id.clone(), name, max_storage_bytes, max_vectors, max_qps);
        tenants.insert(id, tenant.clone());
        let _ = self.storage.set("_system", "tenants", &*tenants);
        Ok(tenant)
    }

    pub fn get_tenant(&self, id: &str) -> Option<Tenant> {
        let tenants = self.tenants.read().unwrap();
        tenants.get(id).cloned()
    }

    pub fn list_tenants(&self) -> Vec<Tenant> {
        let tenants = self.tenants.read().unwrap();
        tenants.values().cloned().collect()
    }

    pub fn check_rate_limit(&self, tenant_id: &str) -> Result<()> {
        let max_qps = {
            let tenants = self.tenants.read().unwrap();
            if let Some(tenant) = tenants.get(tenant_id) {
                tenant.max_qps
            } else {
                10_000 // Fallback
            }
        };

        let now = Instant::now();
        let mut timestamps = self.request_timestamps.write().unwrap();
        let window = timestamps.entry(tenant_id.to_string()).or_default();

        // Evict timestamps older than 1 second
        while let Some(&t) = window.front() {
            if now.duration_since(t).as_secs_f32() > 1.0 {
                window.pop_front();
            } else {
                break;
            }
        }

        if window.len() >= max_qps as usize {
            bail!(
                "Tenant '{}' rate limit exceeded: current {} QPS, max allowed {} QPS",
                tenant_id,
                window.len(),
                max_qps
            );
        }

        window.push_back(now);
        Ok(())
    }

    pub fn check_quota(&self, tenant_id: &str, delta_bytes: u64, delta_vectors: usize) -> Result<()> {
        let tenants = self.tenants.read().unwrap();
        if let Some(tenant) = tenants.get(tenant_id) {
            if tenant.current_storage_bytes + delta_bytes > tenant.max_storage_bytes {
                bail!(
                    "Tenant '{}' storage quota exceeded: current {} bytes + {} bytes > limit {} bytes",
                    tenant_id,
                    tenant.current_storage_bytes,
                    delta_bytes,
                    tenant.max_storage_bytes
                );
            }
            if tenant.current_vectors + delta_vectors > tenant.max_vectors {
                bail!(
                    "Tenant '{}' vector quota exceeded: current {} vectors + {} vectors > limit {} vectors",
                    tenant_id,
                    tenant.current_vectors,
                    delta_vectors,
                    tenant.max_vectors
                );
            }
        }
        Ok(())
    }

    pub fn record_usage(&self, tenant_id: &str, delta_bytes: u64, delta_vectors: usize) {
        let mut tenants = self.tenants.write().unwrap();
        if let Some(tenant) = tenants.get_mut(tenant_id) {
            tenant.current_storage_bytes += delta_bytes;
            tenant.current_vectors += delta_vectors;
            let _ = self.storage.set("_system", "tenants", &*tenants);
        }
    }

    pub fn set_rls_enabled(&self, table: &str, enabled: bool) {
        let mut rls = self.rls_tables.write().unwrap();
        rls.insert(table.to_string(), enabled);
        let _ = self.storage.set("_system", "rls_tables", &*rls);
    }

    pub fn is_rls_enabled(&self, table: &str) -> bool {
        let rls = self.rls_tables.read().unwrap();
        rls.get(table).copied().unwrap_or(false)
    }

    pub fn add_policy(&self, policy: RlsPolicy) {
        let mut policies = self.policies.write().unwrap();
        let list = policies.entry(policy.table.clone()).or_default();
        list.retain(|p| p.name != policy.name);
        list.push(policy);
        let _ = self.storage.set("_system", "rls_policies", &*policies);
    }

    pub fn get_policies(&self, table: &str, command: &str) -> Vec<RlsPolicy> {
        let policies = self.policies.read().unwrap();
        if let Some(list) = policies.get(table) {
            list.iter()
                .filter(|p| p.command == "ALL" || p.command.eq_ignore_ascii_case(command))
                .cloned()
                .collect()
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_storage(name: &str) -> StorageEngine {
        let temp_dir = format!("test_data_tenant_{}", name);
        let _ = std::fs::remove_dir_all(&temp_dir);
        StorageEngine::new(&temp_dir).expect("Failed to create test storage")
    }

    #[test]
    fn test_tenant_creation_and_quota() {
        let storage = create_test_storage("quota");
        let manager = TenantManager::new(storage);

        // Create tenant with 1000 bytes and 5 vectors
        let tenant = manager
            .create_tenant("tenant_acme".to_string(), "Acme Corp".to_string(), 1000, 5, 100)
            .unwrap();
        assert_eq!(tenant.id, "tenant_acme");

        // Quota check within limit
        assert!(manager.check_quota("tenant_acme", 500, 2).is_ok());

        // Exceed storage limit
        assert!(manager.check_quota("tenant_acme", 1500, 2).is_err());

        // Exceed vector limit
        assert!(manager.check_quota("tenant_acme", 500, 10).is_err());

        let _ = std::fs::remove_dir_all("test_data_tenant_quota");
    }

    #[test]
    fn test_rls_policy_registration() {
        let storage = create_test_storage("rls");
        let manager = TenantManager::new(storage);

        assert!(!manager.is_rls_enabled("patients"));
        manager.set_rls_enabled("patients", true);
        assert!(manager.is_rls_enabled("patients"));

        let policy = RlsPolicy {
            name: "tenant_isolation".to_string(),
            table: "patients".to_string(),
            command: "ALL".to_string(),
            using_expr: "tenant_id = CURRENT_TENANT()".to_string(),
        };
        manager.add_policy(policy);

        let retrieved = manager.get_policies("patients", "SELECT");
        assert_eq!(retrieved.len(), 1);
        assert_eq!(retrieved[0].using_expr, "tenant_id = CURRENT_TENANT()");

        let _ = std::fs::remove_dir_all("test_data_tenant_rls");
    }
}
