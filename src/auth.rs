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
}

impl User {
    pub fn new_root(password: String) -> Self {
        Self {
            username: "admin".to_string(),
            password_hash: hash_password(&password), 
            token: None,
            allowed_ip: "%".to_string(),
            roles: vec![Role::MainAdmin],
        }
    }

    pub fn new_user(username: String, password: String, allowed_ip: String, roles: Vec<Role>) -> Self {
        Self {
            username,
            password_hash: hash_password(&password),
            token: None,
            allowed_ip,
            roles,
        }
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
