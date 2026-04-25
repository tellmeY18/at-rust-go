#![deny(unsafe_code)]
#![warn(missing_docs)]
//! DID and handle resolution with TTL-backed in-memory caching for at-rust-go.
//!
//! Wraps DID/handle resolution with a [`moka`] TTL-backed in-memory cache.
//! Every handler that needs to resolve a DID document or handle should go
//! through [`IdentityResolver`] rather than making raw HTTP calls.

use std::time::Duration;

/// A resolved identity from the AT Protocol network.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ResolvedIdentity {
    /// The DID of the resolved identity.
    pub did: String,
    /// The handle (e.g. `alice.bsky.social`).
    pub handle: String,
    /// The PDS endpoint URL.
    pub pds_endpoint: Option<String>,
    /// The raw DID document (if resolved via DID).
    pub did_document: Option<serde_json::Value>,
}

/// Cache performance metrics.
#[derive(Debug, Clone)]
pub struct IdentityMetrics {
    /// Number of cache hits.
    pub hits: u64,
    /// Number of cache misses (network lookups).
    pub misses: u64,
    /// Current number of entries in the cache.
    pub entry_count: u64,
}

/// Configuration for the identity resolver.
#[derive(Debug, Clone)]
pub struct IdentityConfig {
    /// Maximum number of cached entries.
    pub cache_capacity: u64,
    /// TTL per cache entry in seconds.
    pub cache_ttl_secs: u64,
    /// PLC directory URL.
    pub plc_directory: String,
}

impl Default for IdentityConfig {
    fn default() -> Self {
        Self {
            cache_capacity: 10_000,
            cache_ttl_secs: 3600,
            plc_directory: "https://plc.directory".to_string(),
        }
    }
}

/// DID and handle resolver with TTL-backed in-memory cache.
///
/// Use `state.identity.resolve("did:plc:...")` or `state.identity.resolve("alice.bsky.social")`
/// to resolve identities. Results are cached for the configured TTL.
pub struct IdentityResolver {
    cache: moka::future::Cache<String, ResolvedIdentity>,
    http: reqwest::Client,
    plc_directory: String,
    hits: std::sync::atomic::AtomicU64,
    misses: std::sync::atomic::AtomicU64,
}

impl IdentityResolver {
    /// Create a new resolver with the given configuration and HTTP client.
    pub fn new(config: &IdentityConfig, http: reqwest::Client) -> Self {
        let cache = moka::future::Cache::builder()
            .max_capacity(config.cache_capacity)
            .time_to_live(Duration::from_secs(config.cache_ttl_secs))
            .build();

        Self {
            cache,
            http,
            plc_directory: config.plc_directory.clone(),
            hits: std::sync::atomic::AtomicU64::new(0),
            misses: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Create a new resolver with default configuration.
    pub fn with_defaults(http: reqwest::Client) -> Self {
        Self::new(&IdentityConfig::default(), http)
    }

    /// Resolve a DID or handle to a [`ResolvedIdentity`].
    ///
    /// The `subject` can be either a DID (`did:plc:...`, `did:web:...`) or
    /// a handle (`alice.bsky.social`). Results are cached.
    pub async fn resolve(&self, subject: &str) -> anyhow::Result<ResolvedIdentity> {
        if let Some(cached) = self.cache.get(subject).await {
            self.hits.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return Ok(cached);
        }

        self.misses
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let resolved = self.resolve_uncached(subject).await?;

        // Cache under both DID and handle for fast lookup either way.
        self.cache
            .insert(resolved.did.clone(), resolved.clone())
            .await;
        self.cache
            .insert(resolved.handle.clone(), resolved.clone())
            .await;

        Ok(resolved)
    }

    /// Invalidate a cached entry by DID or handle.
    pub async fn invalidate(&self, subject: &str) {
        self.cache.invalidate(subject).await;
    }

    /// Return current cache metrics.
    pub fn metrics(&self) -> IdentityMetrics {
        IdentityMetrics {
            hits: self.hits.load(std::sync::atomic::Ordering::Relaxed),
            misses: self.misses.load(std::sync::atomic::Ordering::Relaxed),
            entry_count: self.cache.entry_count(),
        }
    }

    /// Resolve without cache. Dispatches to DID or handle resolution.
    async fn resolve_uncached(&self, subject: &str) -> anyhow::Result<ResolvedIdentity> {
        if subject.starts_with("did:") {
            self.resolve_did(subject).await
        } else {
            self.resolve_handle(subject).await
        }
    }

    /// Resolve a DID via the PLC directory or did:web.
    async fn resolve_did(&self, did: &str) -> anyhow::Result<ResolvedIdentity> {
        let doc = if did.starts_with("did:plc:") {
            let url = format!("{}/{}", self.plc_directory.trim_end_matches('/'), did);
            tracing::debug!(did = %did, url = %url, "resolving DID via PLC directory");
            let resp = self.http.get(&url).send().await?;
            if !resp.status().is_success() {
                anyhow::bail!("PLC directory returned {} for DID {}", resp.status(), did);
            }
            resp.json::<serde_json::Value>().await?
        } else if did.starts_with("did:web:") {
            let domain = did.strip_prefix("did:web:").unwrap_or(did);
            let url = format!("https://{}/.well-known/did.json", domain);
            tracing::debug!(did = %did, url = %url, "resolving did:web");
            let resp = self.http.get(&url).send().await?;
            if !resp.status().is_success() {
                anyhow::bail!("did:web resolution returned {} for {}", resp.status(), did);
            }
            resp.json::<serde_json::Value>().await?
        } else {
            anyhow::bail!("unsupported DID method: {}", did);
        };

        // Extract handle from alsoKnownAs
        let handle = doc["alsoKnownAs"]
            .as_array()
            .and_then(|arr| {
                arr.iter().find_map(|v| {
                    v.as_str()
                        .and_then(|s| s.strip_prefix("at://"))
                        .map(|s| s.to_string())
                })
            })
            .unwrap_or_default();

        // Extract PDS endpoint from service array
        let pds_endpoint = doc["service"].as_array().and_then(|arr| {
            arr.iter().find_map(|svc| {
                if svc["id"].as_str() == Some("#atproto_pds") {
                    svc["serviceEndpoint"].as_str().map(|s| s.to_string())
                } else {
                    None
                }
            })
        });

        Ok(ResolvedIdentity {
            did: did.to_string(),
            handle,
            pds_endpoint,
            did_document: Some(doc),
        })
    }

    /// Resolve a handle to a DID, then resolve the DID.
    async fn resolve_handle(&self, handle: &str) -> anyhow::Result<ResolvedIdentity> {
        // Use the handle resolution endpoint
        let url = format!(
            "https://bsky.social/xrpc/com.atproto.identity.resolveHandle?handle={}",
            handle
        );
        tracing::debug!(handle = %handle, "resolving handle");

        let resp = self.http.get(&url).send().await?;
        if !resp.status().is_success() {
            anyhow::bail!(
                "handle resolution returned {} for {}",
                resp.status(),
                handle
            );
        }

        let body: serde_json::Value = resp.json().await?;
        let did = body["did"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("handle resolution response missing 'did' field"))?;

        // Now resolve the full DID document
        let mut identity = self.resolve_did(did).await?;
        // Ensure the handle is set even if DID doc doesn't have it
        if identity.handle.is_empty() {
            identity.handle = handle.to_string();
        }
        Ok(identity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = IdentityConfig::default();
        assert_eq!(config.cache_capacity, 10_000);
        assert_eq!(config.cache_ttl_secs, 3600);
        assert_eq!(config.plc_directory, "https://plc.directory");
    }

    #[tokio::test]
    async fn resolver_creation() {
        let resolver = IdentityResolver::with_defaults(reqwest::Client::new());
        let metrics = resolver.metrics();
        assert_eq!(metrics.hits, 0);
        assert_eq!(metrics.misses, 0);
        assert_eq!(metrics.entry_count, 0);
    }

    #[tokio::test]
    async fn invalidate_nonexistent_key() {
        let resolver = IdentityResolver::with_defaults(reqwest::Client::new());
        // Should not panic
        resolver.invalidate("did:plc:nonexistent").await;
    }
}
