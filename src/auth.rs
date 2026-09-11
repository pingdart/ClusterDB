use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};

fn hash_password(password: &str) -> String {
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
        Self {
            username: "admin".to_string(),
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
        self.password_hash == hash_password(password)
    }

    pub fn verify_ip(&self, ip: &str) -> bool {
        if self.allowed_ip == "%" {
            return true;
        }
        self.allowed_ip == ip
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
