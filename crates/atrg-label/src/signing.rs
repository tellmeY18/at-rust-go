//! Label signing utilities.
//!
//! Labels are signed by serializing the label to CBOR, then signing the
//! CBOR bytes with the labeler's private key (ed25519 or secp256k1).

use crate::types::Label;

/// A signer that can produce signatures for labels.
///
/// In v0.2.0, this uses a placeholder HMAC-SHA256 implementation.
/// A future version will support ed25519/secp256k1 per the AT Protocol spec.
pub struct LabelSigner {
    /// The signing key bytes.
    key: Vec<u8>,
}

impl LabelSigner {
    /// Create a signer from raw key bytes.
    pub fn new(key: Vec<u8>) -> Self {
        Self { key }
    }

    /// Create a signer from a base64-encoded key string.
    ///
    /// **Note:** This is a placeholder that treats the encoded string as raw
    /// bytes. A production implementation would perform proper base64 decoding.
    pub fn from_base64(encoded: &str) -> anyhow::Result<Self> {
        // Placeholder: treat the string as raw bytes.
        // A future version will perform proper base64 decoding.
        Ok(Self {
            key: encoded.as_bytes().to_vec(),
        })
    }

    /// Sign a label, returning the base64-encoded signature.
    ///
    /// The signing process:
    /// 1. Serialize the label to canonical CBOR (excluding the `sig` field)
    /// 2. Sign the CBOR bytes with the private key
    /// 3. Return base64-encoded signature
    ///
    /// **Note:** This is a placeholder implementation using a simple hash.
    /// Production use requires ed25519 or secp256k1 signing over CBOR.
    pub fn sign(&self, label: &Label) -> anyhow::Result<String> {
        // Placeholder: hash the JSON representation with the key.
        // Real implementation needs proper CBOR serialization
        // and ed25519/secp256k1 signing.
        let label_json = serde_json::to_vec(label)?;

        let mut hasher_input = Vec::with_capacity(self.key.len() + label_json.len());
        hasher_input.extend_from_slice(&self.key);
        hasher_input.extend_from_slice(&label_json);

        let hash = simple_hash(&hasher_input);
        Ok(base64_encode(&hash))
    }
}

/// Placeholder hash function (djb2 variant producing 8 bytes).
///
/// This is **NOT** cryptographically secure — replace with proper signing.
fn simple_hash(data: &[u8]) -> Vec<u8> {
    let mut hash: u64 = 5381;
    for &byte in data {
        hash = hash.wrapping_mul(33).wrapping_add(byte as u64);
    }
    hash.to_be_bytes().to_vec()
}

/// Simple base64 encoding without external dependency.
fn base64_encode(data: &[u8]) -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;

        result.push(CHARSET[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARSET[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARSET[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARSET[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Label;

    #[test]
    fn sign_produces_non_empty_string() {
        let signer = LabelSigner::new(b"test-key".to_vec());
        let label = Label {
            ver: 1,
            src: "did:plc:test".to_string(),
            uri: "at://did:plc:user/app.bsky.feed.post/abc".to_string(),
            cid: None,
            val: "spam".to_string(),
            neg: false,
            cts: "1970-01-01T00:00:00Z".to_string(),
            exp: None,
        };

        let sig = signer.sign(&label).unwrap();
        assert!(!sig.is_empty());
    }

    #[test]
    fn sign_is_deterministic() {
        let signer = LabelSigner::new(b"key".to_vec());
        let label = Label {
            ver: 1,
            src: "did:plc:test".to_string(),
            uri: "at://did:plc:user/app.bsky.feed.post/abc".to_string(),
            cid: None,
            val: "spam".to_string(),
            neg: false,
            cts: "2024-01-01T00:00:00Z".to_string(),
            exp: None,
        };

        let sig1 = signer.sign(&label).unwrap();
        let sig2 = signer.sign(&label).unwrap();
        assert_eq!(sig1, sig2);
    }

    #[test]
    fn base64_encode_empty() {
        assert_eq!(base64_encode(&[]), "");
    }

    #[test]
    fn base64_encode_single_byte() {
        let result = base64_encode(&[0x4D]);
        assert_eq!(result, "TQ==");
    }

    #[test]
    fn base64_encode_two_bytes() {
        let result = base64_encode(&[0x4D, 0x61]);
        assert_eq!(result, "TWE=");
    }

    #[test]
    fn base64_encode_three_bytes() {
        let result = base64_encode(&[0x4D, 0x61, 0x6E]);
        assert_eq!(result, "TWFu");
    }

    #[test]
    fn from_base64_creates_signer() {
        let signer = LabelSigner::from_base64("dGVzdC1rZXk=").unwrap();
        let label = Label {
            ver: 1,
            src: "did:plc:test".to_string(),
            uri: "at://did:plc:user/app.bsky.feed.post/abc".to_string(),
            cid: None,
            val: "spam".to_string(),
            neg: false,
            cts: "2024-01-01T00:00:00Z".to_string(),
            exp: None,
        };
        let sig = signer.sign(&label).unwrap();
        assert!(!sig.is_empty());
    }
}
