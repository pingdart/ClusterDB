use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use anyhow::{anyhow, Result};
use rand::RngCore;
use sha2::{Digest, Sha256};

pub struct CryptoEngine {
    cipher: Aes256Gcm,
}

impl CryptoEngine {
    /// Initializes the crypto engine by deriving a 256-bit key from a master secret via SHA-256
    pub fn new(master_secret: &str) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(b"PINGDART_ENVELOPE_KEY_DERIVATION_V1");
        hasher.update(master_secret.as_bytes());
        let key_bytes = hasher.finalize();

        let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
        let cipher = Aes256Gcm::new(key);
        Self { cipher }
    }

    /// Encrypts plaintext using AES-256-GCM with a cryptographically secure 96-bit random nonce.
    /// Resulting format: [ 12-byte Nonce || Ciphertext + 16-byte Auth Tag ]
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = self
            .cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| anyhow!("AES-256-GCM encryption failed: {}", e))?;

        let mut output = Vec::with_capacity(12 + ciphertext.len());
        output.extend_from_slice(&nonce_bytes);
        output.extend_from_slice(&ciphertext);

        Ok(output)
    }

    /// Decrypts AES-256-GCM payload with authenticated tag verification
    pub fn decrypt(&self, payload: &[u8]) -> Result<Vec<u8>> {
        if payload.len() < 12 {
            return Err(anyhow!("Ciphertext payload too short for AES-256-GCM nonce"));
        }

        let (nonce_bytes, ciphertext) = payload.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);

        self.cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| anyhow!("AES-256-GCM decryption failed / auth tag invalid: {}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aes_256_gcm_roundtrip() {
        let engine = CryptoEngine::new("my_super_secret_enterprise_master_key_12345");
        let original_data = b"PingDartDB Top Secret AI Memory & Relational Table Payload";

        let encrypted = engine.encrypt(original_data).unwrap();
        assert_ne!(&encrypted[12..], original_data);

        let decrypted = engine.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, original_data);
    }

    #[test]
    fn test_tampered_payload_rejected() {
        let engine = CryptoEngine::new("my_master_key");
        let original_data = b"sensitive transaction data";

        let mut encrypted = engine.encrypt(original_data).unwrap();
        // Tamper with one byte in the ciphertext
        let len = encrypted.len();
        encrypted[len - 1] ^= 0xFF;

        assert!(engine.decrypt(&encrypted).is_err());
    }
}
