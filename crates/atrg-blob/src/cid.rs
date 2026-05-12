//! Content identifier (CID) computation.

use sha2::{Digest, Sha256};

/// Compute a content-addressed identifier from data bytes.
/// Uses SHA-256, hex-encoded with a `sha256-` prefix.
pub fn compute_cid(data: &[u8]) -> String {
    let hash = Sha256::digest(data);
    format!("sha256-{}", hex::encode(hash))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_cid_deterministic() {
        let data = b"hello world";
        let cid1 = compute_cid(data);
        let cid2 = compute_cid(data);
        assert_eq!(cid1, cid2);
        assert!(cid1.starts_with("sha256-"));
    }

    #[test]
    fn test_different_data_different_cid() {
        assert_ne!(compute_cid(b"hello"), compute_cid(b"world"));
    }
}
