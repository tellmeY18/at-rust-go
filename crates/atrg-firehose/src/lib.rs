#![deny(unsafe_code)]
#![warn(missing_docs)]
//! AT Protocol firehose consumer for at-rust-go.
//!
//! Subscribes to `com.atproto.sync.subscribeRepos` on a relay and delivers
//! decoded events through a bounded channel with backpressure.
//!
//! This crate is deliberately independent of `atrg-core` to avoid cyclic
//! dependencies. It defines its own [`FirehoseConfig`] that `atrg-core` maps
//! its firehose configuration into before calling [`spawn_firehose`].

pub mod backoff;
pub mod car;
pub mod consumer;
pub mod event;
pub mod metrics;

pub use consumer::spawn_firehose;
pub use event::{FirehoseCommit, FirehoseEvent, OpAction, RepoOp};
pub use metrics::FirehoseMetrics;

use std::sync::Arc;

use futures::future::BoxFuture;

/// Configuration for the firehose consumer.
///
/// This mirrors firehose-related fields that might live in `atrg-core`'s
/// config but exists in this crate so that `atrg-firehose` has zero
/// dependency on `atrg-core`.
#[derive(Debug, Clone)]
pub struct FirehoseConfig {
    /// Relay WebSocket URL, e.g. `"wss://bsky.network"`.
    pub relay: String,
    /// Cursor (sequence number) to resume from. `None` means start from the
    /// relay's current head.
    pub cursor: Option<i64>,
    /// Bounded back-pressure channel capacity (default: 1024).
    pub channel_capacity: usize,
}

impl Default for FirehoseConfig {
    fn default() -> Self {
        Self {
            relay: "wss://bsky.network".to_string(),
            cursor: None,
            channel_capacity: 1024,
        }
    }
}

/// Type alias for firehose event handler functions.
///
/// The handler receives a [`FirehoseEvent`] and a clone of whatever state
/// object the caller supplied to [`spawn_firehose`]. The state type must be
/// `Clone + Send + Sync + 'static`.
pub type FirehoseHandler<S> =
    Arc<dyn Fn(FirehoseEvent, S) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = FirehoseConfig::default();
        assert_eq!(config.relay, "wss://bsky.network");
        assert!(config.cursor.is_none());
        assert_eq!(config.channel_capacity, 1024);
    }
}
