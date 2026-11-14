use std::time::Duration;

use crate::timing::{MAX_RECONNECT_INTERVAL, RECONNECT_INTERVAL};

/// How long to wait before the next attempt to connect. It starts at the
/// reconnect interval, so a lost connection is replaced promptly, and
/// doubles with every attempt that fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Backoff {
    delay: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Self {
            delay: RECONNECT_INTERVAL,
        }
    }
}

impl Backoff {
    pub(crate) fn delay(self) -> Duration {
        self.delay
    }

    /// The backoff once another attempt has failed.
    pub(crate) fn next(self) -> Self {
        Self {
            delay: self.delay.saturating_mul(2).min(MAX_RECONNECT_INTERVAL),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_at_the_reconnect_interval() {
        assert_eq!(Backoff::default().delay(), RECONNECT_INTERVAL);
    }

    #[test]
    fn doubles_with_every_failed_attempt() {
        let backoff = Backoff::default();
        assert_eq!(backoff.next().delay(), RECONNECT_INTERVAL * 2);
        assert_eq!(backoff.next().next().delay(), RECONNECT_INTERVAL * 4);
    }

    #[test]
    fn stays_at_the_cap() {
        let mut backoff = Backoff::default();
        let mut delays = Vec::new();
        for _ in 0..64 {
            backoff = backoff.next();
            delays.push(backoff.delay());
        }
        assert!(delays.windows(2).all(|d| d[0] <= d[1]));
        assert!(delays.iter().all(|d| *d <= MAX_RECONNECT_INTERVAL));
        assert_eq!(backoff.delay(), MAX_RECONNECT_INTERVAL);
    }
}
