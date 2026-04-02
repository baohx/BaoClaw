use std::time::Duration;

/// Configuration for exponential backoff retry logic.
#[derive(Clone, Debug)]
pub struct RetryConfig {
    pub max_retries: u32,
    pub initial_delay_ms: u64,
    pub max_delay_ms: u64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay_ms: 1000,
            max_delay_ms: 30_000,
        }
    }
}

/// Calculate exponential backoff delay for a given attempt.
///
/// Uses the formula: min(initial_delay * 2^attempt, max_delay)
pub fn calculate_backoff(attempt: u32, config: &RetryConfig) -> Duration {
    let delay_ms = config
        .initial_delay_ms
        .saturating_mul(2u64.saturating_pow(attempt));
    let capped = delay_ms.min(config.max_delay_ms);
    Duration::from_millis(capped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_backoff_first_attempt() {
        let config = RetryConfig {
            max_retries: 3,
            initial_delay_ms: 1000,
            max_delay_ms: 30_000,
        };
        let delay = calculate_backoff(0, &config);
        assert_eq!(delay, Duration::from_millis(1000));
    }

    #[test]
    fn test_calculate_backoff_exponential_growth() {
        let config = RetryConfig {
            max_retries: 5,
            initial_delay_ms: 1000,
            max_delay_ms: 60_000,
        };
        assert_eq!(calculate_backoff(0, &config), Duration::from_millis(1000));
        assert_eq!(calculate_backoff(1, &config), Duration::from_millis(2000));
        assert_eq!(calculate_backoff(2, &config), Duration::from_millis(4000));
        assert_eq!(calculate_backoff(3, &config), Duration::from_millis(8000));
    }

    #[test]
    fn test_calculate_backoff_caps_at_max() {
        let config = RetryConfig {
            max_retries: 10,
            initial_delay_ms: 1000,
            max_delay_ms: 5000,
        };
        assert_eq!(calculate_backoff(0, &config), Duration::from_millis(1000));
        assert_eq!(calculate_backoff(1, &config), Duration::from_millis(2000));
        assert_eq!(calculate_backoff(2, &config), Duration::from_millis(4000));
        // 8000 > 5000, so capped
        assert_eq!(calculate_backoff(3, &config), Duration::from_millis(5000));
        assert_eq!(calculate_backoff(10, &config), Duration::from_millis(5000));
    }

    #[test]
    fn test_calculate_backoff_handles_overflow() {
        let config = RetryConfig {
            max_retries: 100,
            initial_delay_ms: 1000,
            max_delay_ms: 30_000,
        };
        // Very high attempt should not panic, should cap at max_delay
        let delay = calculate_backoff(64, &config);
        assert_eq!(delay, Duration::from_millis(30_000));
    }

    #[test]
    fn test_default_retry_config() {
        let config = RetryConfig::default();
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.initial_delay_ms, 1000);
        assert_eq!(config.max_delay_ms, 30_000);
    }
}
