//! Circuit breaker for API rate limit protection
//!
//! Implements the circuit breaker pattern to prevent cascading failures
//! when hitting API rate limits or experiencing repeated failures.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Circuit breaker states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Normal operation - requests allowed
    Closed,
    /// Too many failures - reject requests immediately
    Open,
    /// Testing recovery - allow one request
    HalfOpen,
}

/// Circuit breaker to prevent cascading failures
///
/// # States
///
/// - **Closed**: Normal operation, all requests allowed
/// - **Open**: Too many failures, reject requests immediately
/// - **HalfOpen**: After timeout, allow one request to test recovery
///
/// # Example
///
/// ```
/// use hox_agent::CircuitBreaker;
///
/// let cb = CircuitBreaker::default();
///
/// // Record failures
/// cb.record_failure();
/// cb.record_failure();
/// cb.record_failure();
///
/// // Circuit is now open
/// assert!(!cb.can_execute());
/// ```
pub struct CircuitBreaker {
    failure_count: AtomicU32,
    last_failure: AtomicU64, // Unix timestamp millis
    threshold: u32,
    timeout: Duration,
}

impl CircuitBreaker {
    /// Create a new circuit breaker
    ///
    /// # Arguments
    ///
    /// * `threshold` - Number of consecutive failures before opening circuit
    /// * `timeout_secs` - Seconds to wait before attempting recovery (half-open state)
    pub fn new(threshold: u32, timeout_secs: u64) -> Self {
        Self {
            failure_count: AtomicU32::new(0),
            last_failure: AtomicU64::new(0),
            threshold,
            timeout: Duration::from_secs(timeout_secs),
        }
    }

    /// Get current circuit state
    pub fn state(&self) -> CircuitState {
        let failures = self.failure_count.load(Ordering::Relaxed);

        if failures < self.threshold {
            return CircuitState::Closed;
        }

        // Circuit is open - check if timeout has elapsed
        let last_failure = self.last_failure.load(Ordering::Relaxed);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let elapsed = now.saturating_sub(last_failure);

        if elapsed >= self.timeout.as_millis() as u64 {
            CircuitState::HalfOpen
        } else {
            CircuitState::Open
        }
    }

    /// Record a successful operation (resets failure count)
    pub fn record_success(&self) {
        self.failure_count.store(0, Ordering::Relaxed);
    }

    /// Record a failed operation
    pub fn record_failure(&self) {
        self.failure_count.fetch_add(1, Ordering::Relaxed);

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        self.last_failure.store(now, Ordering::Relaxed);
    }

    /// Check if a request can be executed
    ///
    /// Returns `true` if circuit is closed or half-open (testing recovery)
    /// Returns `false` if circuit is open (too many failures)
    pub fn can_execute(&self) -> bool {
        match self.state() {
            CircuitState::Closed => true,
            CircuitState::HalfOpen => true,
            CircuitState::Open => false,
        }
    }

    /// Get current failure count (for monitoring)
    pub fn failure_count(&self) -> u32 {
        self.failure_count.load(Ordering::Relaxed)
    }

    /// Get time until circuit can be tested (ms), 0 if not open
    pub fn time_until_retry(&self) -> u64 {
        match self.state() {
            CircuitState::Open => {
                let last_failure = self.last_failure.load(Ordering::Relaxed);
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64;

                let elapsed = now.saturating_sub(last_failure);
                (self.timeout.as_millis() as u64).saturating_sub(elapsed)
            }
            // Closed and HalfOpen states can execute immediately - no retry delay
            _ => 0,
        }
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        // Conservative defaults: 3 failures, 60 second timeout
        Self::new(3, 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn test_initial_state_closed() {
        let cb = CircuitBreaker::default();
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.can_execute());
    }

    #[test]
    fn test_opens_after_threshold() {
        let cb = CircuitBreaker::new(3, 60);

        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Closed);
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Closed);
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
        assert!(!cb.can_execute());
    }

    #[test]
    fn test_success_resets_failures() {
        let cb = CircuitBreaker::new(3, 60);

        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.failure_count(), 2);

        cb.record_success();
        assert_eq!(cb.failure_count(), 0);
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn test_half_open_after_timeout() {
        let cb = CircuitBreaker::new(2, 1); // 1 second timeout

        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);

        sleep(Duration::from_millis(1100));
        assert_eq!(cb.state(), CircuitState::HalfOpen);
        assert!(cb.can_execute());
    }

    #[test]
    fn test_half_open_can_recover() {
        let cb = CircuitBreaker::new(2, 1);

        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);

        sleep(Duration::from_millis(1100));
        assert_eq!(cb.state(), CircuitState::HalfOpen);

        cb.record_success();
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.can_execute());
    }

    #[test]
    fn test_time_until_retry() {
        let cb = CircuitBreaker::new(2, 2); // 2 second timeout

        cb.record_failure();
        cb.record_failure();

        let time_remaining = cb.time_until_retry();
        assert!(time_remaining > 0);
        assert!(time_remaining <= 2000);
    }
}
