use crate::storage::StorageEngine;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Permission {
    DatabaseCreate,
    DatabaseDrop,
    DatabaseRead,
    DatabaseWrite,
    TableCreate,
    TableDrop,
    TableRead,
    TableWrite,
    VectorSearch,
    VectorInsert,
    GraphRead,
    GraphWrite,
    MemoryRecall,
    MemoryWrite,
    BackupCreate,
    BackupRestore,
    PITR,
    TenantAdmin,
    UserAdmin,
    AuditRead,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RoleScope {
    Global,
    Database(String),
    Tenant(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleDefinition {
    pub name: String,
    pub description: String,
    pub permissions: HashSet<Permission>,
    pub scope: RoleScope,
}

pub struct RoleManager {
    storage: StorageEngine,
    roles: RwLock<std::collections::HashMap<String, RoleDefinition>>,
}

impl RoleManager {
    pub fn new(storage: StorageEngine) -> Self {
        let mut roles = std::collections::HashMap::new();

        // 1. Load custom saved roles from storage if any
        if let Ok(Some(saved)) = storage.get::<std::collections::HashMap<String, RoleDefinition>>("_system", "rbac_roles") {
            roles = saved;
        }

        let manager = Self {
            storage,
            roles: RwLock::new(roles),
        };

        // 2. Ensure standard predefined enterprise roles exist
        manager.ensure_default_roles();
        manager
    }

    fn ensure_default_roles(&self) {
        let mut roles = self.roles.write().unwrap();

        if !roles.contains_key("SuperAdmin") {
            let mut all_perms = HashSet::new();
            all_perms.insert(Permission::DatabaseCreate);
            all_perms.insert(Permission::DatabaseDrop);
            all_perms.insert(Permission::DatabaseRead);
            all_perms.insert(Permission::DatabaseWrite);
            all_perms.insert(Permission::TableCreate);
            all_perms.insert(Permission::TableDrop);
            all_perms.insert(Permission::TableRead);
            all_perms.insert(Permission::TableWrite);
            all_perms.insert(Permission::VectorSearch);
            all_perms.insert(Permission::VectorInsert);
            all_perms.insert(Permission::GraphRead);
            all_perms.insert(Permission::GraphWrite);
            all_perms.insert(Permission::MemoryRecall);
            all_perms.insert(Permission::MemoryWrite);
            all_perms.insert(Permission::BackupCreate);
            all_perms.insert(Permission::BackupRestore);
            all_perms.insert(Permission::PITR);
            all_perms.insert(Permission::TenantAdmin);
            all_perms.insert(Permission::UserAdmin);
            all_perms.insert(Permission::AuditRead);

            roles.insert("SuperAdmin".to_string(), RoleDefinition {
                name: "SuperAdmin".to_string(),
                description: "Unrestricted platform administrator".to_string(),
                permissions: all_perms,
                scope: RoleScope::Global,
            });
        }

        if !roles.contains_key("DataScientist") {
            let mut perms = HashSet::new();
            perms.insert(Permission::DatabaseRead);
            perms.insert(Permission::TableRead);
            perms.insert(Permission::VectorSearch);
            perms.insert(Permission::VectorInsert);
            perms.insert(Permission::GraphRead);
            perms.insert(Permission::MemoryRecall);

            roles.insert("DataScientist".to_string(), RoleDefinition {
                name: "DataScientist".to_string(),
                description: "Read, vector query, and feature embedding evaluation".to_string(),
                permissions: perms,
                scope: RoleScope::Global,
            });
        }

        if !roles.contains_key("AppDeveloper") {
            let mut perms = HashSet::new();
            perms.insert(Permission::DatabaseRead);
            perms.insert(Permission::TableCreate);
            perms.insert(Permission::TableRead);
            perms.insert(Permission::TableWrite);
            perms.insert(Permission::VectorSearch);
            perms.insert(Permission::VectorInsert);
            perms.insert(Permission::GraphRead);
            perms.insert(Permission::GraphWrite);
            perms.insert(Permission::MemoryRecall);
            perms.insert(Permission::MemoryWrite);

            roles.insert("AppDeveloper".to_string(), RoleDefinition {
                name: "AppDeveloper".to_string(),
                description: "Application service account and developer privileges".to_string(),
                permissions: perms,
                scope: RoleScope::Global,
            });
        }

        if !roles.contains_key("ReadOnly") {
            let mut perms = HashSet::new();
            perms.insert(Permission::DatabaseRead);
            perms.insert(Permission::TableRead);
            perms.insert(Permission::VectorSearch);
            perms.insert(Permission::GraphRead);
            perms.insert(Permission::MemoryRecall);
            perms.insert(Permission::AuditRead);

            roles.insert("ReadOnly".to_string(), RoleDefinition {
                name: "ReadOnly".to_string(),
                description: "Strict read-only access".to_string(),
                permissions: perms,
                scope: RoleScope::Global,
            });
        }
    }

    pub fn create_role(&self, def: RoleDefinition) -> Result<()> {
        let mut roles = self.roles.write().unwrap();
        if roles.contains_key(&def.name) {
            return Err(anyhow!("Role '{}' already exists", def.name));
        }
        roles.insert(def.name.clone(), def);
        let _ = self.storage.set("_system", "rbac_roles", &*roles);
        Ok(())
    }

    pub fn get_role(&self, name: &str) -> Option<RoleDefinition> {
        let roles = self.roles.read().unwrap();
        roles.get(name).cloned()
    }

    pub fn list_roles(&self) -> Vec<RoleDefinition> {
        let roles = self.roles.read().unwrap();
        roles.values().cloned().collect()
    }

    /// Checks if a collection of role names grants a given permission for a given database/tenant context
    pub fn has_permission(
        &self,
        assigned_roles: &[String],
        perm: &Permission,
        target_db: Option<&str>,
        target_tenant: Option<&str>,
    ) -> bool {
        let roles = self.roles.read().unwrap();
        for r_name in assigned_roles {
            if let Some(role) = roles.get(r_name) {
                if !role.permissions.contains(perm) {
                    continue;
                }

                match &role.scope {
                    RoleScope::Global => return true,
                    RoleScope::Database(db) => {
                        if let Some(target) = target_db {
                            if db == target || target == "default" {
                                return true;
                            }
                        }
                    }
                    RoleScope::Tenant(t) => {
                        if let Some(target) = target_tenant {
                            if t == target {
                                return true;
                            }
                        }
                    }
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_permissions() {
        let storage = StorageEngine::new_temp().unwrap();
        let mgr = RoleManager::new(storage);

        // SuperAdmin has everything
        assert!(mgr.has_permission(&["SuperAdmin".into()], &Permission::DatabaseDrop, None, None));
        assert!(mgr.has_permission(&["SuperAdmin".into()], &Permission::TableCreate, Some("prod_db"), None));

        // DataScientist has VectorSearch but NOT TableCreate
        assert!(mgr.has_permission(&["DataScientist".into()], &Permission::VectorSearch, None, None));
        assert!(!mgr.has_permission(&["DataScientist".into()], &Permission::TableCreate, None, None));

        // ReadOnly has TableRead but NOT TableWrite
        assert!(mgr.has_permission(&["ReadOnly".into()], &Permission::TableRead, None, None));
        assert!(!mgr.has_permission(&["ReadOnly".into()], &Permission::TableWrite, None, None));
    }
}
