//! Exponential backoff with a configurable cap for reconnection.

use std::time::Duration;

/// Exponential backoff starting at 1 second and capping at 60 seconds.
pub struct Backoff {
    current: Duration,
    max: Duration,
    base: Duration,
}

impl Backoff {
    /// Create a new backoff starting at 1s, capping at 60s.
    pub fn new() -> Self {
        Self {
            current: Duration::from_secs(1),
            max: Duration::from_secs(60),
            base: Duration::from_secs(1),
        }
    }

    /// Get the next backoff duration and advance the state.
    pub fn next_delay(&mut self) -> Duration {
        let duration = self.current;
        self.current = (self.current * 2).min(self.max);
        duration
    }

    /// Reset the backoff to the initial value (call on successful connection).
    pub fn reset(&mut self) {
        self.current = self.base;
    }

    /// Get the current backoff duration in milliseconds.
    pub fn current_ms(&self) -> u64 {
        self.current.as_millis() as u64
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
    fn backoff_doubles() {
        let mut b = Backoff::new();
        assert_eq!(b.next_delay(), Duration::from_secs(1));
        assert_eq!(b.next_delay(), Duration::from_secs(2));
        assert_eq!(b.next_delay(), Duration::from_secs(4));
        assert_eq!(b.next_delay(), Duration::from_secs(8));
    }

    #[test]
    fn backoff_caps_at_60s() {
        let mut b = Backoff::new();
        for _ in 0..20 {
            b.next_delay();
        }
        assert_eq!(b.next_delay(), Duration::from_secs(60));
    }

    #[test]
    fn backoff_resets() {
        let mut b = Backoff::new();
        b.next_delay();
        b.next_delay();
        b.reset();
        assert_eq!(b.next_delay(), Duration::from_secs(1));
    }

    #[test]
    fn current_ms_reflects_state() {
        let mut b = Backoff::new();
        assert_eq!(b.current_ms(), 1000);
        b.next_delay();
        assert_eq!(b.current_ms(), 2000);
    }

    #[test]
    fn default_is_same_as_new() {
        let b = Backoff::default();
        assert_eq!(b.current_ms(), 1000);
    }
}
