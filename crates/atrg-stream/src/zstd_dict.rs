//! ZSTD dictionary auto-fetch and caching.
//!
//! Supports loading a dictionary from a local file path or an HTTP(S) URL.
//! URL-sourced dictionaries are cached under `~/.cache/atrg/` keyed by a
//! SHA-256 hash of the URL so that restarts don't re-download.

use std::path::PathBuf;

use sha2::{Digest, Sha256};

/// Load a ZSTD dictionary from a local path or URL.
///
/// - If `source` is a local file path → load directly.
/// - If `source` is an HTTP(S) URL → download and cache under `~/.cache/atrg/`.
/// - Returns the raw bytes of the dictionary.
pub async fn load_dictionary(source: &str, http: &reqwest::Client) -> anyhow::Result<Vec<u8>> {
    if source.starts_with("http://") || source.starts_with("https://") {
        load_from_url(source, http).await
    } else {
        load_from_file(source).await
    }
}

async fn load_from_file(path: &str) -> anyhow::Result<Vec<u8>> {
    let data = tokio::fs::read(path).await?;
    tracing::info!(path = %path, size = data.len(), "loaded ZSTD dictionary from file");
    Ok(data)
}

async fn load_from_url(url: &str, http: &reqwest::Client) -> anyhow::Result<Vec<u8>> {
    let cache_path = cache_path_for_url(url);

    // Try cached first
    if cache_path.exists() {
        let data = tokio::fs::read(&cache_path).await?;
        tracing::info!(
            path = %cache_path.display(),
            size = data.len(),
            "loaded ZSTD dictionary from cache"
        );
        return Ok(data);
    }

    // Download
    tracing::info!(url = %url, "downloading ZSTD dictionary");
    let resp = http.get(url).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("failed to download ZSTD dictionary: HTTP {}", resp.status());
    }
    let data = resp.bytes().await?.to_vec();

    // Cache
    if let Some(parent) = cache_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(&cache_path, &data).await?;
    tracing::info!(
        path = %cache_path.display(),
        size = data.len(),
        "cached ZSTD dictionary"
    );

    Ok(data)
}

/// Compute the cache file path for a URL.
///
/// The path is deterministic: same URL always maps to the same file.
pub fn cache_path_for_url(url: &str) -> PathBuf {
    let hash = hex_encode(&Sha256::digest(url.as_bytes()));
    let cache_dir = dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from(".cache"))
        .join("atrg");
    cache_dir.join(format!("jetstream-dict-{}.bin", &hash[..16]))
}

fn hex_encode(data: &[u8]) -> String {
    data.iter().map(|b| format!("{:02x}", b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_path_is_deterministic() {
        let p1 = cache_path_for_url("https://example.com/dict.bin");
        let p2 = cache_path_for_url("https://example.com/dict.bin");
        assert_eq!(p1, p2);
    }

    #[test]
    fn different_urls_different_paths() {
        let p1 = cache_path_for_url("https://example.com/a.bin");
        let p2 = cache_path_for_url("https://example.com/b.bin");
        assert_ne!(p1, p2);
    }

    #[test]
    fn cache_path_under_atrg_dir() {
        let p = cache_path_for_url("https://example.com/dict.bin");
        let s = p.to_string_lossy();
        assert!(s.contains("atrg"), "expected 'atrg' in path: {s}");
        assert!(
            s.contains("jetstream-dict-"),
            "expected 'jetstream-dict-' in path: {s}"
        );
    }

    #[test]
    fn hex_encode_works() {
        assert_eq!(hex_encode(&[0xde, 0xad, 0xbe, 0xef]), "deadbeef");
        assert_eq!(hex_encode(&[0x00, 0xff]), "00ff");
    }
}
