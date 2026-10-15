//! Keeping to a service's rate limit: a few calls at once, then one per interval, shared by
//! everything that calls the service.

use std::time::{Duration, Instant};

/// A token bucket: `burst` calls may go at once, then one every `pace`.
#[derive(Debug)]
pub struct Pacer {
    pace: Duration,
    burst: f64,
    tokens: f64,
    at: Instant,
}

impl Pacer {
    pub fn new(pace: Duration, burst: u32) -> Self {
        Self {
            pace,
            burst: f64::from(burst),
            tokens: f64::from(burst),
            at: Instant::now(),
        }
    }

    /// How long to wait before the next call, taking its turn.
    pub fn take(&mut self, now: Instant) -> Duration {
        let refilled =
            now.saturating_duration_since(self.at).as_secs_f64() / self.pace.as_secs_f64();
        self.tokens = (self.tokens + refilled).min(self.burst);
        self.at = now.max(self.at);
        self.tokens -= 1.0;
        if self.tokens >= 0.0 {
            Duration::ZERO
        } else {
            self.pace.mul_f64(-self.tokens)
        }
    }

    /// Waits for the next call's turn.
    pub async fn wait(pacer: &tokio::sync::Mutex<Pacer>) {
        let wait = pacer.lock().await.take(Instant::now());
        tokio::time::sleep(wait).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calls_are_paced_after_a_burst() {
        let pace = Duration::from_secs(1);
        let mut pacer = Pacer::new(pace, 5);
        let start = pacer.at;
        for _ in 0..5 {
            assert_eq!(pacer.take(start), Duration::ZERO);
        }
        assert_eq!(pacer.take(start), pace);
        assert_eq!(pacer.take(start), pace * 2);
        // Waiting refills it.
        assert_eq!(pacer.take(start + pace * 10), Duration::ZERO);
    }
}
