//! Fail-open utilities for graceful degradation
//!
//! This module provides utilities for operations that should fail gracefully without
//! crashing the entire system. Use these for infrastructure operations like logging,
//! metrics, and non-critical features.
//!
//! DO NOT use fail-open for:
//! - Agent execution (business logic)
//! - Backpressure checks (correctness)
//! - Metadata reads (state)

use std::future::Future;
use tracing::warn;

use crate::Result;

/// Execute an operation that should fail open (infrastructure, not business logic)
///
/// Logs the error via `tracing::warn!` on failure and returns `None`.
///
/// # Usage
///
/// ```no_run
/// use hox_core::fail_open::fail_open;
/// use hox_core::Result;
///
/// async fn log_activity() -> Result<()> {
///     // Some operation that might fail
///     Ok(())
/// }
///
/// async fn example() {
///     let result = fail_open("activity_logger", || log_activity()).await;
///     // result is None if log_activity() failed, otherwise Some(())
/// }
/// ```
///
/// # Examples of appropriate use:
/// - Activity logging
/// - Metrics/telemetry
/// - OpLog polling
/// - Workspace cleanup
/// - Pattern recording
pub async fn fail_open<F, Fut, T>(operation_name: &str, f: F) -> Option<T>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    match f().await {
        Ok(val) => Some(val),
        Err(e) => {
            warn!("{} failed (fail-open): {}", operation_name, e);
            None
        }
    }
}

/// Like fail_open but with exponential backoff retries
///
/// Retries the operation up to `max_retries` times with exponential backoff.
/// The backoff duration is `100ms * attempt`.
///
/// # Usage
///
/// ```no_run
/// use hox_core::fail_open::fail_open_with_retries;
/// use hox_core::Result;
///
/// async fn poll_oplog() -> Result<String> {
///     // Some operation that might have transient failures
///     Ok("data".to_string())
/// }
///
/// async fn example() {
///     let result = fail_open_with_retries("oplog_watcher", || poll_oplog(), 3).await;
///     // Retries up to 3 times with 100ms, 200ms, 300ms delays
/// }
/// ```
pub async fn fail_open_with_retries<F, Fut, T>(
    operation_name: &str,
    mut f: F,
    max_retries: usize,
) -> Option<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    for attempt in 1..=max_retries {
        match f().await {
            Ok(val) => return Some(val),
            Err(e) => {
                if attempt == max_retries {
                    warn!(
                        "{} failed after {} retries (fail-open): {}",
                        operation_name, max_retries, e
                    );
                    return None;
                }
                warn!(
                    "{} failed (attempt {}/{}): {}",
                    operation_name, attempt, max_retries, e
                );
                let delay_ms = 100 * attempt as u64;
                tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HoxError;

    #[tokio::test]
    async fn test_fail_open_success() {
        let result = fail_open("test_op", || async { Ok::<_, HoxError>(42) }).await;
        assert_eq!(result, Some(42));
    }

    #[tokio::test]
    async fn test_fail_open_failure() {
        let result = fail_open("test_op", || async {
            Err::<i32, _>(HoxError::Other("test error".to_string()))
        })
        .await;
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn test_fail_open_with_retries_success_first_try() {
        let mut attempts = 0;
        let result = fail_open_with_retries(
            "test_op",
            || {
                attempts += 1;
                async move { Ok::<_, HoxError>(42) }
            },
            3,
        )
        .await;
        assert_eq!(result, Some(42));
        assert_eq!(attempts, 1);
    }

    #[tokio::test]
    async fn test_fail_open_with_retries_success_after_retry() {
        let mut attempts = 0;
        let result = fail_open_with_retries(
            "test_op",
            || {
                attempts += 1;
                async move {
                    if attempts < 2 {
                        Err(HoxError::Other("transient error".to_string()))
                    } else {
                        Ok(42)
                    }
                }
            },
            3,
        )
        .await;
        assert_eq!(result, Some(42));
        assert_eq!(attempts, 2);
    }

    #[tokio::test]
    async fn test_fail_open_with_retries_all_failures() {
        let mut attempts = 0;
        let result = fail_open_with_retries(
            "test_op",
            || {
                attempts += 1;
                async move { Err::<i32, _>(HoxError::Other("persistent error".to_string())) }
            },
            3,
        )
        .await;
        assert_eq!(result, None);
        assert_eq!(attempts, 3);
    }
}
