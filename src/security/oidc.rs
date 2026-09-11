use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub iss: Option<String>,
    #[serde(default)]
    pub roles: Vec<String>,
    pub tenant_id: Option<String>,
    pub exp: u64,
}

pub struct JwtValidator {
    secret: String,
    expected_issuer: Option<String>,
}

impl JwtValidator {
    pub fn new(secret: &str, expected_issuer: Option<String>) -> Self {
        Self {
            secret: secret.to_string(),
            expected_issuer,
        }
    }

    /// Validates an OIDC / OAuth2 HMAC-SHA256 bearer JWT
    pub fn validate(&self, token: &str) -> Result<Claims> {
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 3 {
            return Err(anyhow!("Invalid JWT format: expected 3 dot-separated segments"));
        }

        let signing_input = format!("{}.{}", parts[0], parts[1]);
        let expected_signature = Self::hmac_sha256(&self.secret, signing_input.as_bytes());

        if parts[2] != expected_signature {
            return Err(anyhow!("Invalid JWT signature"));
        }

        // Decode payload (standard base64 or URL-safe base64 without padding)
        let payload_json = Self::base64_decode(parts[1])?;
        let claims: Claims = serde_json::from_slice(&payload_json)
            .map_err(|e| anyhow!("Failed to parse JWT claims: {}", e))?;

        // Validate expiration
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        if now > claims.exp {
            return Err(anyhow!("JWT token has expired"));
        }

        // Validate issuer if configured
        if let Some(expected_iss) = &self.expected_issuer {
            if claims.iss.as_deref() != Some(expected_iss) {
                return Err(anyhow!("JWT issuer mismatch"));
            }
        }

        Ok(claims)
    }

    /// Helper to issue signed JWTs for testing or internal cluster token issuance
    pub fn issue_token(&self, claims: &Claims) -> Result<String> {
        let header = r#"{"alg":"HS256","typ":"JWT"}"#;
        let header_b64 = Self::base64_encode(header.as_bytes());
        let payload_bytes = serde_json::to_vec(claims)?;
        let payload_b64 = Self::base64_encode(&payload_bytes);

        let signing_input = format!("{}.{}", header_b64, payload_b64);
        let signature = Self::hmac_sha256(&self.secret, signing_input.as_bytes());

        Ok(format!("{}.{}", signing_input, signature))
    }

    fn hmac_sha256(key: &str, data: &[u8]) -> String {
        // Lightweight HMAC-SHA256 implementation without external openssl
        let mut block = [0u8; 64];
        let key_bytes = key.as_bytes();
        if key_bytes.len() > 64 {
            let mut hasher = Sha256::new();
            hasher.update(key_bytes);
            block[..32].copy_from_slice(&hasher.finalize());
        } else {
            block[..key_bytes.len()].copy_from_slice(key_bytes);
        }

        let mut o_key_pad = [0x5cu8; 64];
        let mut i_key_pad = [0x36u8; 64];
        for i in 0..64 {
            o_key_pad[i] ^= block[i];
            i_key_pad[i] ^= block[i];
        }

        let mut inner = Sha256::new();
        inner.update(&i_key_pad);
        inner.update(data);
        let inner_hash = inner.finalize();

        let mut outer = Sha256::new();
        outer.update(&o_key_pad);
        outer.update(inner_hash);
        let outer_hash = outer.finalize();

        Self::base64_encode(&outer_hash)
    }

    fn base64_encode(data: &[u8]) -> String {
        const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut result = String::new();
        let mut i = 0;
        while i < data.len() {
            let b0 = data[i] as usize;
            let b1 = if i + 1 < data.len() { data[i + 1] as usize } else { 0 };
            let b2 = if i + 2 < data.len() { data[i + 2] as usize } else { 0 };

            result.push(CHARS[(b0 >> 2) & 0x3F] as char);
            result.push(CHARS[((b0 << 4) | (b1 >> 4)) & 0x3F] as char);
            if i + 1 < data.len() {
                result.push(CHARS[((b1 << 2) | (b2 >> 6)) & 0x3F] as char);
            }
            if i + 2 < data.len() {
                result.push(CHARS[b2 & 0x3F] as char);
            }
            i += 3;
        }
        result
    }

    fn base64_decode(input: &str) -> Result<Vec<u8>> {
        fn decode_char(c: u8) -> Result<u8> {
            match c {
                b'A'..=b'Z' => Ok(c - b'A'),
                b'a'..=b'z' => Ok(c - b'a' + 26),
                b'0'..=b'9' => Ok(c - b'0' + 52),
                b'-' | b'+' => Ok(62),
                b'_' | b'/' => Ok(63),
                b'=' => Ok(0),
                _ => Err(anyhow!("Invalid base64 character: {}", c as char)),
            }
        }

        let input_bytes = input.as_bytes();
        let mut output = Vec::new();
        let mut i = 0;

        while i < input_bytes.len() {
            let chunk_len = std::cmp::min(4, input_bytes.len() - i);
            let c0 = decode_char(input_bytes[i])? as usize;
            let c1 = if chunk_len > 1 { decode_char(input_bytes[i + 1])? as usize } else { 0 };
            let c2 = if chunk_len > 2 { decode_char(input_bytes[i + 2])? as usize } else { 0 };
            let c3 = if chunk_len > 3 { decode_char(input_bytes[i + 3])? as usize } else { 0 };

            output.push(((c0 << 2) | (c1 >> 4)) as u8);
            if chunk_len > 2 && input_bytes[i + 2] != b'=' {
                output.push(((c1 << 4) | (c2 >> 2)) as u8);
            }
            if chunk_len > 3 && input_bytes[i + 3] != b'=' {
                output.push(((c2 << 6) | c3) as u8);
            }

            i += 4;
        }

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jwt_issuance_and_validation() {
        let validator = JwtValidator::new("enterprise_jwt_signing_secret_xyz", Some("https://auth.pingdart.io".into()));

        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let claims = Claims {
            sub: "analyst_sarah".to_string(),
            iss: Some("https://auth.pingdart.io".to_string()),
            roles: vec!["DataScientist".to_string()],
            tenant_id: Some("tenant_health".to_string()),
            exp: now + 3600,
        };

        let token = validator.issue_token(&claims).unwrap();
        let decoded = validator.validate(&token).unwrap();

        assert_eq!(decoded.sub, "analyst_sarah");
        assert_eq!(decoded.tenant_id, Some("tenant_health".to_string()));
        assert_eq!(decoded.roles, vec!["DataScientist"]);
    }
}
