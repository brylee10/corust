//! A synchronous, token bucket rate limiter which limits number of requests in a given time window.
//! The rate limiter and refills at a constant rate. The rate limiter can be configured to have a
//! maximum number of tokens. Implemented as a token bucket instead of leaky bucket limiter to
//! allow for request bursts.

#[cfg(target_arch = "wasm32")]
use web_time::{Duration, Instant};

#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, Instant};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RateLimiterError {
    #[error("Rate limit exceeded.")]
    RateLimitExceeded,
}

#[derive(Default)]
pub struct RateLimiterBuilder {
    /// Maximum number of tokens that can be stored in the rate limiter.
    max_tokens: usize,
    /// Number of tokens that the rate limiter starts with.
    tokens: usize,
    /// Interval over which the rate limiter will add new tokens, in seconds.
    interval_sec: Duration,
    /// Number of tokens to add to the rate limiter every interval.
    refill: usize,
}

impl RateLimiterBuilder {
    pub fn tokens(mut self, tokens: usize) -> Self {
        self.tokens = tokens;
        self
    }

    pub fn max_tokens(mut self, max_tokens: usize) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    pub fn interval(mut self, interval: Duration) -> Self {
        self.interval_sec = interval;
        self
    }

    pub fn refill(mut self, refill: usize) -> Self {
        self.refill = refill;
        self
    }

    pub fn build(self) -> RateLimiter {
        RateLimiter {
            max_tokens: self.max_tokens,
            tokens: self.tokens,
            interval_sec: self.interval_sec,
            last_refresh: Instant::now(),
            refill: self.refill,
        }
    }
}

/// A synchronous token bucket rate limiter. One token is consumed per request, and the rate limiter refills
/// at a constant rate `interval_sec`. The rate limiter can be configured to have a maximum number of tokens.
#[derive(Debug)]
pub struct RateLimiter {
    /// Maximum number of tokens that can be stored in the rate limiter.
    /// `max_tokens` is required since the first refresh may occur long after
    /// the `RateLimiter` construction, so the rate limiter
    /// could add a large number of tokens which may skew `tokens`.
    max_tokens: usize,
    /// Number of tokens that the rate limiter starts with.
    tokens: usize,
    /// Number of seconds before a refresh
    interval_sec: Duration,
    /// Time of previous rate limiter refresh
    last_refresh: Instant,
    /// Number of tokens to add to the rate limiter every interval.
    refill: usize,
}

impl RateLimiter {
    pub fn builder() -> RateLimiterBuilder {
        // A sensible default RateLimiter configuration
        RateLimiterBuilder {
            max_tokens: 100,
            tokens: 0,
            interval_sec: Duration::from_secs(1),
            refill: 1,
        }
    }

    /// Requests one token from the RateLimiter after refreshing token count.
    /// Returns an error if there are no tokens remaining. Otherwise, takes the token.
    pub fn send_one(&mut self, time: Instant) -> Result<(), RateLimiterError> {
        self.refresh_tokens(time);
        if self.tokens == 0 {
            return Err(RateLimiterError::RateLimitExceeded);
        }
        self.tokens -= 1;
        Ok(())
    }

    /// Requests multiple tokens from the RateLimiter after refreshing token count.
    /// If not all tokens can be provided, then returns an error.
    #[allow(unused)]
    pub fn send_many(&mut self, time: Instant, tokens: usize) -> Result<(), RateLimiterError> {
        self.refresh_tokens(time);
        if self.tokens < tokens {
            return Err(RateLimiterError::RateLimitExceeded);
        }
        self.tokens -= tokens;
        Ok(())
    }

    fn refresh_tokens(&mut self, time: Instant) {
        let elapsed = time - self.last_refresh;
        let intervals_elapsed = elapsed.div_duration_f64(self.interval_sec);
        if (self.max_tokens as f64 / self.refill as f64) < intervals_elapsed {
            self.tokens = self.max_tokens;
            return;
        }
        // Only sets `last_refresh` when an entire interval has passed, otherwise
        // a user which sends repeatedly under 1 interval would never see tokens increase.
        if intervals_elapsed > 1.0 {
            self.last_refresh = time;
            // Note: this will take the floor of the number of intervals
            self.tokens += self.refill * intervals_elapsed as usize;
        }
    }

    /// Returns maximum number of tokens in a duration
    pub fn max_tokens(&self) -> usize {
        self.max_tokens
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_rate_limit_builder() {
        let rate_limiter = RateLimiter::builder()
            .tokens(10)
            .interval(Duration::from_secs(1))
            .refill(1)
            .max_tokens(1000)
            .build();
        assert_eq!(rate_limiter.tokens, 10);
        assert_eq!(rate_limiter.interval_sec, Duration::from_secs(1));
        assert_eq!(rate_limiter.refill, 1);
        assert_eq!(rate_limiter.max_tokens, 1000);
    }

    #[test]
    fn test_exhaust_tokens() {
        let mut rate_limiter = RateLimiter::builder()
            .tokens(1)
            .interval(Duration::from_secs(1))
            .refill(1)
            .build();
        let time = Instant::now();
        assert!(rate_limiter.send_one(time).is_ok());
        assert!(matches!(
            rate_limiter.send_one(time),
            Err(RateLimiterError::RateLimitExceeded)
        ));
    }

    /// Tests a sequence of requests, each just under the interval, will still refresh tokens
    /// and the sender will not be starved.
    #[test]
    fn test_refresh_tokens_slightly_below_interval() {
        let mut rate_limiter = RateLimiter::builder()
            .tokens(1)
            .interval(Duration::from_secs(1))
            .refill(1)
            .max_tokens(1)
            .build();
        let time = Instant::now();
        let resend_time1 = time + Duration::from_millis(999);
        let resend_time2 = time + Duration::from_millis(2999);
        assert!(rate_limiter.send_one(time).is_ok());
        assert!(matches!(
            rate_limiter.send_one(resend_time1),
            Err(RateLimiterError::RateLimitExceeded)
        ));
        assert_eq!(rate_limiter.tokens, 0);

        // Rate limiter has 1 token after refresh and consumes 1, leaving 0 tokens.
        assert!(rate_limiter.send_one(resend_time2).is_ok());
        assert_eq!(dbg!(rate_limiter.tokens), 0);
    }

    #[test]
    fn test_send_many() {
        let mut rate_limiter = RateLimiter::builder()
            .tokens(10)
            .interval(Duration::from_secs(1))
            .refill(1)
            .max_tokens(10)
            .build();
        let time = Instant::now();
        assert!(rate_limiter.send_many(time, 10).is_ok());
        assert!(matches!(
            rate_limiter.send_many(time, 10),
            Err(RateLimiterError::RateLimitExceeded)
        ));
    }
}
