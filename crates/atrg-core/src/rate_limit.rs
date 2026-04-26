//! Token-bucket rate limiting middleware.
//!
//! Provides a per-IP token-bucket rate limiter that can be used as Axum
//! middleware or checked manually in handlers. Disabled by default when
//! [`RateLimitConfig::enabled`] is `false`.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use tokio::sync::Mutex;

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Configuration for the token-bucket rate limiter.
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Sustained request rate (tokens added per second).
    pub requests_per_second: f64,
    /// Maximum burst size (bucket capacity).
    pub burst: u32,
    /// Whether rate limiting is active. When `false`, all requests are allowed.
    pub enabled: bool,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_second: 10.0,
            burst: 50,
            enabled: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Token bucket (internal)
// ---------------------------------------------------------------------------

struct TokenBucket {
    tokens: f64,
    last_refill: Instant,
    max_tokens: f64,
    refill_rate: f64,
}

impl TokenBucket {
    fn new(max_tokens: f64, refill_rate: f64) -> Self {
        Self {
            tokens: max_tokens,
            last_refill: Instant::now(),
            max_tokens,
            refill_rate,
        }
    }

    /// Refill tokens based on elapsed time since last refill.
    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.max_tokens);
        self.last_refill = now;
    }

    /// Try to consume one token. Returns `true` if allowed.
    fn try_consume(&mut self) -> bool {
        self.refill();
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    /// Seconds until the next token becomes available.
    fn retry_after(&self) -> f64 {
        if self.tokens >= 1.0 {
            return 0.0;
        }
        let deficit = 1.0 - self.tokens;
        deficit / self.refill_rate
    }
}

// ---------------------------------------------------------------------------
// Rate limiter
// ---------------------------------------------------------------------------

/// Per-IP token-bucket rate limiter.
///
/// Thread-safe and cheaply cloneable (inner state is `Arc<Mutex<_>>`).
#[derive(Clone)]
pub struct RateLimiter {
    buckets: Arc<Mutex<HashMap<IpAddr, TokenBucket>>>,
    config: RateLimitConfig,
}

impl RateLimiter {
    /// Create a new rate limiter with the given configuration.
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            buckets: Arc::new(Mutex::new(HashMap::new())),
            config,
        }
    }

    /// Check whether a request from `ip` is allowed.
    ///
    /// Returns `Ok(())` if the request is within limits, or `Err(retry_after)`
    /// with the number of seconds the client should wait before retrying.
    pub async fn check(&self, ip: IpAddr) -> Result<(), f64> {
        if !self.config.enabled {
            return Ok(());
        }

        let mut buckets = self.buckets.lock().await;
        let bucket = buckets.entry(ip).or_insert_with(|| {
            TokenBucket::new(
                f64::from(self.config.burst),
                self.config.requests_per_second,
            )
        });

        if bucket.try_consume() {
            Ok(())
        } else {
            Err(bucket.retry_after())
        }
    }

    /// Remove buckets that have not been used for longer than `max_age`.
    ///
    /// Call this periodically (e.g. every few minutes) to prevent unbounded
    /// memory growth from unique IP addresses.
    pub async fn cleanup(&self, max_age: Duration) {
        let mut buckets = self.buckets.lock().await;
        let cutoff = Instant::now() - max_age;
        buckets.retain(|_ip, bucket| bucket.last_refill > cutoff);
    }
}

// ---------------------------------------------------------------------------
// HTTP response helper
// ---------------------------------------------------------------------------

/// Build a `429 Too Many Requests` response with AT-Protocol-style JSON body.
pub fn rate_limit_response(retry_after_secs: f64) -> axum::response::Response<Body> {
    let retry_after_ceil = retry_after_secs.ceil() as u64;

    let body = serde_json::json!({
        "error": "rate_limit_exceeded",
        "message": format!(
            "Rate limit exceeded. Retry after {} seconds.",
            retry_after_ceil
        ),
    });

    let mut response = (StatusCode::TOO_MANY_REQUESTS, Json(body)).into_response();
    if let Ok(val) = axum::http::HeaderValue::from_str(&retry_after_ceil.to_string()) {
        response.headers_mut().insert("Retry-After", val);
    }
    response
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn token_bucket_allows_within_burst() {
        let mut bucket = TokenBucket::new(5.0, 1.0);
        for _ in 0..5 {
            assert!(bucket.try_consume(), "should allow requests within burst");
        }
        assert!(!bucket.try_consume(), "should deny after burst exhausted");
    }

    #[test]
    fn token_bucket_retry_after_positive_when_empty() {
        let mut bucket = TokenBucket::new(1.0, 10.0);
        assert!(bucket.try_consume());
        assert!(!bucket.try_consume());
        let retry = bucket.retry_after();
        assert!(
            retry > 0.0,
            "retry_after should be positive when empty, got {}",
            retry
        );
        // At 10 tokens/sec, retry should be <= 0.1s
        assert!(
            retry <= 0.15,
            "retry_after should be small at high refill rate, got {}",
            retry
        );
    }

    #[tokio::test]
    async fn rate_limiter_allows_burst() {
        let config = RateLimitConfig {
            requests_per_second: 1.0,
            burst: 3,
            enabled: true,
        };
        let limiter = RateLimiter::new(config);
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));

        for i in 0..3 {
            assert!(
                limiter.check(ip).await.is_ok(),
                "request {} should be allowed within burst",
                i
            );
        }
        assert!(
            limiter.check(ip).await.is_err(),
            "request beyond burst should be denied"
        );
    }

    #[tokio::test]
    async fn rate_limiter_disabled_allows_all() {
        let config = RateLimitConfig {
            requests_per_second: 1.0,
            burst: 1,
            enabled: false,
        };
        let limiter = RateLimiter::new(config);
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));

        // Even 100 requests should be fine when disabled.
        for _ in 0..100 {
            assert!(limiter.check(ip).await.is_ok());
        }
    }

    #[tokio::test]
    async fn cleanup_removes_old_entries() {
        let config = RateLimitConfig {
            requests_per_second: 10.0,
            burst: 10,
            enabled: true,
        };
        let limiter = RateLimiter::new(config);
        let ip = IpAddr::V4(Ipv4Addr::new(172, 16, 0, 1));

        // Generate an entry.
        let _ = limiter.check(ip).await;

        // Cleanup with a zero max_age removes everything.
        limiter.cleanup(Duration::from_secs(0)).await;

        let buckets = limiter.buckets.lock().await;
        assert!(
            buckets.is_empty(),
            "cleanup should have removed the stale entry"
        );
    }

    #[test]
    fn default_config_values() {
        let cfg = RateLimitConfig::default();
        assert!((cfg.requests_per_second - 10.0).abs() < f64::EPSILON);
        assert_eq!(cfg.burst, 50);
        assert!(cfg.enabled);
    }

    #[tokio::test]
    async fn rate_limit_response_returns_429() {
        let response = rate_limit_response(1.5);
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);

        let retry_after = response
            .headers()
            .get("retry-after")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(retry_after, "2"); // ceil(1.5) = 2

        // Check body contains error
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "rate_limit_exceeded");
    }
}
