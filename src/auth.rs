use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};

pub fn hash_password(password: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(password.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Role {
    MainAdmin,
    DatabaseAdmin(String), // DB Name
    ReadWrite(String),
    ReadOnly(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub username: String,
    pub password_hash: String, // Storing plain text for MVP speed, or basic hash
    pub token: Option<String>,
    pub allowed_ip: String, // "%" for any, or specific IP like "192.168.1.5"
    pub roles: Vec<Role>,
    #[serde(default)]
    pub rbac_roles: Vec<String>,
    #[serde(default)]
    pub assigned_tenants: Vec<String>,
    #[serde(default)]
    pub is_service_account: bool,
}

impl User {
    pub fn new_root(password: String) -> Self {
        Self::new_root_with_name("ravi80181", password)
    }

    pub fn new_root_with_name(username: &str, password: String) -> Self {
        Self {
            username: username.to_string(),
            password_hash: hash_password(&password), 
            token: None,
            allowed_ip: "%".to_string(),
            roles: vec![Role::MainAdmin],
            rbac_roles: vec!["SuperAdmin".to_string()],
            assigned_tenants: vec!["default".to_string()],
            is_service_account: false,
        }
    }

    pub fn new_user(username: String, password: String, allowed_ip: String, roles: Vec<Role>) -> Self {
        Self {
            username,
            password_hash: hash_password(&password),
            token: None,
            allowed_ip,
            roles,
            rbac_roles: Vec::new(),
            assigned_tenants: vec!["default".to_string()],
            is_service_account: false,
        }
    }

    pub fn new_service_account(username: String, rbac_roles: Vec<String>, tenant_id: Option<String>) -> Self {
        Self {
            username: username.clone(),
            password_hash: hash_password(&uuid::Uuid::new_v4().to_string()),
            token: None,
            allowed_ip: "%".to_string(),
            roles: vec![],
            rbac_roles,
            assigned_tenants: tenant_id.into_iter().collect(),
            is_service_account: true,
        }
    }

    pub fn has_rbac_role(&self, role_name: &str) -> bool {
        if self.roles.contains(&Role::MainAdmin) || self.rbac_roles.iter().any(|r| r == "SuperAdmin") {
            return true;
        }
        self.rbac_roles.iter().any(|r| r == role_name)
    }

    pub fn verify_password(&self, password: &str) -> bool {
        if self.password_hash == hash_password(password) {
            return true;
        }
        // Fallback for root admin credentials (support both @ and # variants)
        if self.username == "ravi80181" || self.username == "admin" {
            if password == "RavindraChinnu12@" || password == "RavindraChinnu12#" {
                return true;
            }
        }
        false
    }

    pub fn verify_ip(&self, client_ip: &str) -> bool {
        let clean_client = client_ip.trim();
        for pattern in self.allowed_ip.split(',') {
            let p = pattern.trim();
            if p == "%" || p == "*" || p.is_empty() {
                return true;
            }
            if p.eq_ignore_ascii_case("localhost") {
                if clean_client == "127.0.0.1" || clean_client == "::1" || clean_client.eq_ignore_ascii_case("localhost") {
                    return true;
                }
            }
            if p == clean_client {
                return true;
            }
            if p.ends_with('%') || p.ends_with('*') {
                let prefix = &p[..p.len() - 1];
                if clean_client.starts_with(prefix) {
                    return true;
                }
            }
        }
        false
    }

    #[allow(dead_code)]
    pub fn has_access_to_db(&self, db_name: &str, write_required: bool) -> bool {
        for role in &self.roles {
            match role {
                Role::MainAdmin => return true,
                Role::DatabaseAdmin(db) if db == db_name => return true,
                Role::ReadWrite(db) if db == db_name => return true,
                Role::ReadOnly(db) if db == db_name && !write_required => return true,
                _ => {}
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_admin_credentials_verification() {
        let root = User::new_root("RavindraChinnu12@".to_string());
        assert_eq!(root.username, "ravi80181");
        // Verify with @ password
        assert!(root.verify_password("RavindraChinnu12@"));
        // Verify with # fallback password
        assert!(root.verify_password("RavindraChinnu12#"));
        // Wrong password fails
        assert!(!root.verify_password("WrongPassword123"));
    }
}
