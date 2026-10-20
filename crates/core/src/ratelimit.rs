//! Per-uplink rate limiting -- token bucket for service side (Sec.4.5 abuse controls)
//!
//! The relay already has RateLimiter (global cap, per-IP, handshake rate, per-session token bucket).
//! This module adds a per-uplink rate limiter for the service egress path, to prevent
//! a single uplink from being overrun and to enforce user-configured caps (e.g., metered links).

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// Token bucket: rate in bytes/sec, burst in bytes
#[derive(Debug, Clone)]
pub struct TokenBucket {
    rate_bps: u64,
    burst_bytes: u64,
    tokens: f64,
    last_refill: Instant,
}

impl TokenBucket {
    pub fn new(rate_bps: u64, burst_bytes: u64, now: Instant) -> Self {
        Self {
            rate_bps,
            burst_bytes,
            tokens: burst_bytes as f64,
            last_refill: now,
        }
    }

    pub fn set_rate(&mut self, rate_bps: u64, burst_bytes: u64) {
        self.rate_bps = rate_bps;
        self.burst_bytes = burst_bytes;
        self.tokens = self.tokens.min(burst_bytes as f64);
    }

    fn refill(&mut self, now: Instant) {
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        if elapsed <= 0.0 {
            return;
        }
        self.tokens += elapsed * (self.rate_bps as f64 / 8.0);
        if self.tokens > self.burst_bytes as f64 {
            self.tokens = self.burst_bytes as f64;
        }
        self.last_refill = now;
    }

    /// Try to consume `bytes`. Returns true if allowed.
    pub fn try_consume(&mut self, bytes: u64, now: Instant) -> bool {
        self.refill(now);
        if self.tokens >= bytes as f64 {
            self.tokens -= bytes as f64;
            true
        } else {
            false
        }
    }

    /// How long to wait before `bytes` would be available
    pub fn delay_for(&mut self, bytes: u64, now: Instant) -> Duration {
        self.refill(now);
        if self.tokens >= bytes as f64 {
            Duration::from_millis(0)
        } else {
            let needed = bytes as f64 - self.tokens;
            let rate_per_sec = self.rate_bps as f64 / 8.0;
            if rate_per_sec <= 0.0 {
                Duration::from_secs(3600)
            } else {
                Duration::from_secs_f64(needed / rate_per_sec)
            }
        }
    }

    pub fn tokens(&self) -> f64 {
        self.tokens
    }
}

/// Per-uplink rate limiters
#[derive(Debug, Clone)]
pub struct RateLimiter {
    buckets: BTreeMap<u8, TokenBucket>,
    default_rate_bps: u64,
    default_burst: u64,
}

impl RateLimiter {
    pub fn new(default_rate_bps: u64, default_burst: u64) -> Self {
        Self {
            buckets: BTreeMap::new(),
            default_rate_bps,
            default_burst,
        }
    }

    pub fn set_uplink_rate(&mut self, uplink: u8, rate_bps: u64, burst: u64, now: Instant) {
        if let Some(b) = self.buckets.get_mut(&uplink) {
            b.set_rate(rate_bps, burst);
        } else {
            self.buckets.insert(uplink, TokenBucket::new(rate_bps, burst, now));
        }
    }

    pub fn remove_uplink(&mut self, uplink: u8) {
        self.buckets.remove(&uplink);
    }

    pub fn try_send(&mut self, uplink: u8, bytes: u64, now: Instant) -> bool {
        let default_rate = self.default_rate_bps;
        let default_burst = self.default_burst;
        let bucket = self.buckets.entry(uplink).or_insert_with(|| TokenBucket::new(default_rate, default_burst, now));
        bucket.try_consume(bytes, now)
    }

    pub fn delay_for(&mut self, uplink: u8, bytes: u64, now: Instant) -> Duration {
        let default_rate = self.default_rate_bps;
        let default_burst = self.default_burst;
        let bucket = self.buckets.entry(uplink).or_insert_with(|| TokenBucket::new(default_rate, default_burst, now));
        bucket.delay_for(bytes, now)
    }

    pub fn set_default(&mut self, rate_bps: u64, burst: u64) {
        self.default_rate_bps = rate_bps;
        self.default_burst = burst;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn token_bucket_allows_burst_then_limits() {
        let now = Instant::now();
        let mut tb = TokenBucket::new(8000, 1000, now); // 1KB/s, burst 1KB
        assert!(tb.try_consume(1000, now));
        assert!(!tb.try_consume(1, now));
        // After 1 sec, should have 1KB again
        assert!(tb.try_consume(1000, now + Duration::from_secs(1)));
    }

    #[test]
    fn rate_limiter_per_uplink() {
        let now = Instant::now();
        let mut rl = RateLimiter::new(8000, 1000);
        assert!(rl.try_send(0, 1000, now));
        assert!(!rl.try_send(0, 1, now));
        assert!(rl.try_send(1, 1000, now)); // different uplink, separate bucket
    }

    #[test]
    fn delay_for_calculates_wait() {
        let now = Instant::now();
        let mut tb = TokenBucket::new(8000, 1000, now); // 1KB/s
        tb.try_consume(1000, now);
        let delay = tb.delay_for(1000, now);
        assert!(delay.as_millis() >= 900 && delay.as_millis() <= 1100);
    }
}
