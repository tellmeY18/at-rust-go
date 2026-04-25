#![deny(unsafe_code)]
#![warn(missing_docs)]
//! Jetstream consumer wiring for at-rust-go.
//!
//! Provides a bounded, backpressure-aware Jetstream consumer that
//! spawns as a background task and delivers events to a user-supplied handler.
//!
//! This crate is deliberately independent of `atrg-core` to avoid cyclic
//! dependencies. It defines its own [`StreamConfig`] that `atrg-core` maps
//! its `JetstreamConfig` into before calling [`spawn_consumer`].

pub mod backoff;
pub mod consumer;
pub mod event;
pub mod metrics;
pub mod zstd_dict;

pub use consumer::spawn_consumer;
pub use event::JetstreamEvent;
pub use metrics::JetstreamMetrics;

use std::sync::Arc;

use futures::future::BoxFuture;

/// Configuration for the Jetstream consumer.
///
/// This mirrors the fields from `atrg-core`'s `JetstreamConfig` but lives
/// in this crate so that `atrg-stream` has zero dependency on `atrg-core`.
#[derive(Debug, Clone)]
pub struct StreamConfig {
    /// Jetstream relay host, e.g. `"jetstream1.us-east.bsky.network"`.
    pub host: String,
    /// NSID collections to subscribe to, e.g. `["app.bsky.feed.post"]`.
    pub collections: Vec<String>,
    /// Optional path or URL to a ZSTD dictionary for decompression.
    pub zstd_dict: Option<String>,
    /// Bounded back-pressure channel size (default: 1024).
    pub channel_capacity: usize,
    /// Event lag threshold before shedding/warning (default: 10_000).
    pub max_lag_events: usize,
}

/// Type alias for event handler functions.
///
/// The handler receives a [`JetstreamEvent`] and a clone of whatever state
/// object the caller supplied to [`spawn_consumer`]. The state type must be
/// `Clone + Send + Sync + 'static`.
pub type EventHandler<S> =
    Arc<dyn Fn(JetstreamEvent, S) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>;
