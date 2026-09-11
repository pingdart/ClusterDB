use crate::storage::StorageEngine;
use anyhow::{anyhow, Result};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub key_id: String,
    pub key_hash: String,
    pub name: String,
    pub prefix: String, // First 12 chars e.g. "pd_live_a1b2..."
    pub roles: Vec<String>,
    pub tenant_id: Option<String>,
    pub created_at: u64,
    pub expires_at: Option<u64>,
    pub last_used_at: Option<u64>,
    pub revoked: bool,
}

pub struct ApiKeyManager {
    storage: StorageEngine,
}

impl ApiKeyManager {
    pub fn new(storage: StorageEngine) -> Self {
        Self { storage }
    }

    fn hash_secret(secret: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(secret.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Generates a new cryptographically secure API key
    /// Returns (ApiKey metadata, raw_secret) - raw_secret is shown only once!
    pub fn create_key(
        &self,
        name: &str,
        roles: Vec<String>,
        tenant_id: Option<String>,
        ttl_secs: Option<u64>,
    ) -> Result<(ApiKey, String)> {
        let key_id = uuid::Uuid::new_v4().to_string();
        let mut rand_bytes = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut rand_bytes);
        let rand_hex = hex::encode(rand_bytes);
        let raw_secret = format!("pd_live_{}_{}", uuid::Uuid::new_v4().simple(), rand_hex);
        let prefix = format!("pd_live_{}...", &raw_secret[8..16]);
        let key_hash = Self::hash_secret(&raw_secret);

        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let expires_at = ttl_secs.map(|ttl| now + ttl);

        let api_key = ApiKey {
            key_id: key_id.clone(),
            key_hash: key_hash.clone(),
            name: name.to_string(),
            prefix,
            roles,
            tenant_id,
            created_at: now,
            expires_at,
            last_used_at: None,
            revoked: false,
        };

        // Store by hash for fast authentication lookup
        self.storage.set("_api_keys_by_hash", &key_hash, &api_key)?;
        // Store by key_id for management/revocation
        self.storage.set("_api_keys_by_id", &key_id, &api_key)?;
        self.storage.flush()?;

        Ok((api_key, raw_secret))
    }

    /// Validates raw API key and updates last_used_at timestamp
    pub fn validate_key(&self, raw_secret: &str) -> Result<ApiKey> {
        let hash = Self::hash_secret(raw_secret);
        let key: ApiKey = self
            .storage
            .get("_api_keys_by_hash", &hash)?
            .ok_or_else(|| anyhow!("Invalid or non-existent API key"))?;

        if key.revoked {
            return Err(anyhow!("API key '{}' has been revoked", key.name));
        }

        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        if let Some(exp) = key.expires_at {
            if now > exp {
                return Err(anyhow!("API key '{}' has expired", key.name));
            }
        }

        // Update last used timestamp asynchronously/best-effort
        let mut updated = key.clone();
        updated.last_used_at = Some(now);
        let _ = self.storage.set("_api_keys_by_hash", &hash, &updated);
        let _ = self.storage.set("_api_keys_by_id", &key.key_id, &updated);

        Ok(key)
    }

    /// Revokes an API key immediately by key_id
    pub fn revoke_key(&self, key_id: &str) -> Result<()> {
        if let Some(mut key) = self.storage.get::<ApiKey>("_api_keys_by_id", key_id)? {
            key.revoked = true;
            self.storage.set("_api_keys_by_id", key_id, &key)?;
            self.storage.set("_api_keys_by_hash", &key.key_hash, &key)?;
            self.storage.flush()?;
            Ok(())
        } else {
            Err(anyhow!("API key with ID '{}' not found", key_id))
        }
    }

    /// Lists all managed API keys
    pub fn list_keys(&self) -> Result<Vec<ApiKey>> {
        self.storage.get_all::<ApiKey>("_api_keys_by_id")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_key_lifecycle() {
        let storage = StorageEngine::new_temp().unwrap();
        let mgr = ApiKeyManager::new(storage);

        // 1. Create key
        let (meta, raw_secret) = mgr
            .create_key("agent-service-key", vec!["DataScientist".into()], Some("tenant_alpha".into()), Some(3600))
            .unwrap();

        assert_eq!(meta.name, "agent-service-key");
        assert!(raw_secret.starts_with("pd_live_"));
        assert!(!meta.revoked);

        // 2. Validate key
        let validated = mgr.validate_key(&raw_secret).unwrap();
        assert_eq!(validated.key_id, meta.key_id);
        assert_eq!(validated.tenant_id, Some("tenant_alpha".into()));

        // 3. Revoke key
        mgr.revoke_key(&meta.key_id).unwrap();

        // 4. Validate revoked key -> must fail
        assert!(mgr.validate_key(&raw_secret).is_err());
    }
}
