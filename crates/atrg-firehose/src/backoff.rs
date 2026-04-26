//! Exponential backoff with jitter for firehose reconnection.

use std::time::Duration;

/// Exponential backoff starting at 500ms and capping at 30 seconds.
///
/// Each call to [`next_delay`](Backoff::next_delay) doubles the delay
/// (with jitter) up to the configured maximum. Call [`reset`](Backoff::reset)
/// after a successful connection to start over.
pub struct Backoff {
    attempt: u32,
    base_ms: u64,
    max_ms: u64,
}

impl Backoff {
    /// Create a new backoff starting at 500ms, capping at 30s.
    pub fn new() -> Self {
        Self {
            attempt: 0,
            base_ms: 500,
            max_ms: 30_000,
        }
    }

    /// Get the next backoff duration and advance the state.
    ///
    /// The delay is `base_ms * 2^attempt`, clamped to `max_ms`, with a
    /// small deterministic jitter derived from the attempt number.
    pub fn next_delay(&mut self) -> Duration {
        let shift = self.attempt.min(31);
        let delay_ms = self
            .base_ms
            .saturating_mul(1u64.checked_shl(shift).unwrap_or(u64::MAX));
        let clamped = delay_ms.min(self.max_ms);

        // Deterministic jitter: add up to ~12% based on attempt parity.
        let jitter_ms =
            (clamped / 8).wrapping_mul((self.attempt as u64 % 3) + 1) % (clamped / 4 + 1);
        let total = clamped.saturating_add(jitter_ms).min(self.max_ms);

        self.attempt = self.attempt.saturating_add(1);
        Duration::from_millis(total)
    }

    /// Reset the backoff to the initial value (call on successful connection).
    pub fn reset(&mut self) {
        self.attempt = 0;
    }

    /// Get the current backoff duration in milliseconds (without advancing).
    pub fn current_ms(&self) -> u64 {
        let shift = self.attempt.min(31);
        let delay_ms = self
            .base_ms
            .saturating_mul(1u64.checked_shl(shift).unwrap_or(u64::MAX));
        delay_ms.min(self.max_ms)
    }
}

impl Default for Backoff {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_increases() {
        let mut b = Backoff::new();
        let d1 = b.next_delay();
        let d2 = b.next_delay();
        let d3 = b.next_delay();
        // Each delay should be >= the previous base (500, 1000, 2000).
        assert!(d1.as_millis() >= 500);
        assert!(d2.as_millis() >= 1000);
        assert!(d3.as_millis() >= 2000);
    }

    #[test]
    fn backoff_caps_at_max() {
        let mut b = Backoff::new();
        for _ in 0..30 {
            b.next_delay();
        }
        let d = b.next_delay();
        assert!(d.as_millis() <= 30_000);
    }

    #[test]
    fn backoff_resets() {
        let mut b = Backoff::new();
        b.next_delay();
        b.next_delay();
        b.next_delay();
        b.reset();
        assert_eq!(b.current_ms(), 500);
        let d = b.next_delay();
        assert!(d.as_millis() >= 500);
        assert!(d.as_millis() <= 1000);
    }

    #[test]
    fn current_ms_reflects_state() {
        let mut b = Backoff::new();
        assert_eq!(b.current_ms(), 500);
        b.next_delay();
        assert_eq!(b.current_ms(), 1000);
        b.next_delay();
        assert_eq!(b.current_ms(), 2000);
    }

    #[test]
    fn default_is_same_as_new() {
        let b = Backoff::default();
        assert_eq!(b.current_ms(), 500);
    }
}
