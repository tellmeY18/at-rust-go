//! Jetstream consumer metrics.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Metrics snapshot for the Jetstream consumer.
#[derive(Debug, Clone)]
pub struct JetstreamMetrics {
    /// Total events received from the WebSocket.
    pub events_received: u64,
    /// Events dropped due to backpressure.
    pub events_dropped: u64,
    /// Errors encountered during processing.
    pub errors: u64,
    /// Number of reconnections.
    pub reconnects: u64,
    /// Timestamp of last event (unix ms).
    pub last_event_at: u64,
    /// Current backoff duration in ms.
    pub current_backoff_ms: u64,
    /// Current queue depth.
    pub queue_depth: u64,
}

/// Shared, atomic counters for the consumer.
#[derive(Debug)]
pub struct MetricsCounter {
    /// Total events received from the WebSocket.
    pub(crate) events_received: AtomicU64,
    /// Events dropped due to backpressure.
    pub(crate) events_dropped: AtomicU64,
    /// Errors encountered during processing.
    pub(crate) errors: AtomicU64,
    /// Number of reconnections.
    pub(crate) reconnects: AtomicU64,
    /// Timestamp of last event (unix ms).
    pub(crate) last_event_at: AtomicU64,
    /// Current backoff duration in ms.
    pub(crate) current_backoff_ms: AtomicU64,
}

impl MetricsCounter {
    /// Create a new zeroed counter set.
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Snapshot the current metrics.
    pub fn snapshot(&self, queue_depth: u64) -> JetstreamMetrics {
        JetstreamMetrics {
            events_received: self.events_received.load(Ordering::Relaxed),
            events_dropped: self.events_dropped.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
            reconnects: self.reconnects.load(Ordering::Relaxed),
            last_event_at: self.last_event_at.load(Ordering::Relaxed),
            current_backoff_ms: self.current_backoff_ms.load(Ordering::Relaxed),
            queue_depth,
        }
    }
}

impl Default for MetricsCounter {
    fn default() -> Self {
        Self {
            events_received: AtomicU64::new(0),
            events_dropped: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            reconnects: AtomicU64::new(0),
            last_event_at: AtomicU64::new(0),
            current_backoff_ms: AtomicU64::new(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_counter_starts_at_zero() {
        let counter = MetricsCounter::new();
        let snapshot = counter.snapshot(0);
        assert_eq!(snapshot.events_received, 0);
        assert_eq!(snapshot.events_dropped, 0);
        assert_eq!(snapshot.errors, 0);
        assert_eq!(snapshot.reconnects, 0);
        assert_eq!(snapshot.last_event_at, 0);
        assert_eq!(snapshot.current_backoff_ms, 0);
        assert_eq!(snapshot.queue_depth, 0);
    }

    #[test]
    fn metrics_counter_increments() {
        let counter = MetricsCounter::new();
        counter.events_received.fetch_add(5, Ordering::Relaxed);
        counter.errors.fetch_add(1, Ordering::Relaxed);
        let snapshot = counter.snapshot(3);
        assert_eq!(snapshot.events_received, 5);
        assert_eq!(snapshot.errors, 1);
        assert_eq!(snapshot.queue_depth, 3);
    }

    #[test]
    fn metrics_snapshot_is_independent() {
        let counter = MetricsCounter::new();
        counter.events_received.fetch_add(1, Ordering::Relaxed);
        let snap1 = counter.snapshot(0);
        counter.events_received.fetch_add(1, Ordering::Relaxed);
        let snap2 = counter.snapshot(0);
        assert_eq!(snap1.events_received, 1);
        assert_eq!(snap2.events_received, 2);
    }
}
