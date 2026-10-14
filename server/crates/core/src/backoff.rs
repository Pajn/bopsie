//! Waiting longer after each failure before trying again: doubling from a base up to a cap,
//! less a random part so that many retries do not line up, and starting over once things have
//! worked for a while.

use std::time::{Duration, Instant};

/// How a [`Backoff`] waits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    /// The wait after a first failure.
    pub base: Duration,
    /// The longest wait.
    pub max: Duration,
    /// How long things must work for the next failure to count as a first one again.
    pub reset_after: Duration,
}

impl Policy {
    /// For reconnecting: 1 s doubling to 30 s, starting over after a minute of working.
    pub const RECONNECT: Self = Self {
        base: Duration::from_secs(1),
        max: Duration::from_secs(30),
        reset_after: Duration::from_secs(60),
    };
}

/// The most a wait is shortened by, as a fraction of it.
const JITTER: f64 = 0.25;

#[derive(Debug, Clone)]
pub struct Backoff {
    policy: Policy,
    /// Failures since things last worked for long enough.
    failures: u32,
    /// No try before this.
    not_before: Option<Instant>,
    /// Working since.
    working_since: Option<Instant>,
}

impl Backoff {
    pub fn new(policy: Policy) -> Self {
        Self {
            policy,
            failures: 0,
            not_before: None,
            working_since: None,
        }
    }

    /// Records a failure at `now`, and returns how long to wait before trying again.
    pub fn failed(&mut self, now: Instant) -> Duration {
        self.failed_with(now, rand::random::<f64>())
    }

    /// As [`Backoff::failed`], with `random` in `0..1` choosing the jitter.
    pub fn failed_with(&mut self, now: Instant, random: f64) -> Duration {
        if self
            .working_since
            .take()
            .is_some_and(|since| now.saturating_duration_since(since) >= self.policy.reset_after)
        {
            self.failures = 0;
        }
        let doubled = self
            .policy
            .base
            .saturating_mul(1 << self.failures.min(16))
            .min(self.policy.max);
        self.failures = self.failures.saturating_add(1);
        let wait = doubled.mul_f64(1.0 - JITTER * random.clamp(0.0, 1.0));
        self.not_before = Some(now + wait);
        wait
    }

    /// Records that things work from `now`: after [`Policy::reset_after`] of it, the next
    /// failure waits as little as a first one.
    pub fn working(&mut self, now: Instant) {
        self.not_before = None;
        self.working_since.get_or_insert(now);
    }

    /// Starts over, as after a failure that was the caller's own doing.
    pub fn reset(&mut self) {
        self.failures = 0;
        self.not_before = None;
        self.working_since = None;
    }

    /// How long until another try, if one must wait.
    pub fn wait(&self, now: Instant) -> Option<Duration> {
        self.not_before
            .map(|at| at.saturating_duration_since(now))
            .filter(|left| !left.is_zero())
    }

    pub fn failures(&self) -> u32 {
        self.failures
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const POLICY: Policy = Policy {
        base: Duration::from_secs(1),
        max: Duration::from_secs(8),
        reset_after: Duration::from_secs(60),
    };

    #[test]
    fn waits_double_up_to_the_cap() {
        let mut backoff = Backoff::new(POLICY);
        let now = Instant::now();
        let waits: Vec<u64> = (0..6)
            .map(|_| backoff.failed_with(now, 0.0).as_secs())
            .collect();
        assert_eq!(waits, [1, 2, 4, 8, 8, 8]);
        assert_eq!(backoff.failures(), 6);
    }

    #[test]
    fn jitter_only_shortens() {
        let mut backoff = Backoff::new(POLICY);
        let now = Instant::now();
        assert_eq!(
            backoff.failed_with(now, 1.0),
            Duration::from_millis(750),
            "at most a quarter less"
        );
        assert_eq!(backoff.failed_with(now, 0.5), Duration::from_millis(1_750));
    }

    #[test]
    fn says_how_long_is_left() {
        let mut backoff = Backoff::new(POLICY);
        let now = Instant::now();
        assert_eq!(backoff.wait(now), None);
        backoff.failed_with(now, 0.0);
        assert_eq!(
            backoff.wait(now + Duration::from_millis(400)),
            Some(Duration::from_millis(600))
        );
        assert_eq!(backoff.wait(now + Duration::from_secs(1)), None);
    }

    #[test]
    fn working_long_enough_starts_over() {
        let mut backoff = Backoff::new(POLICY);
        let now = Instant::now();
        for _ in 0..3 {
            backoff.failed_with(now, 0.0);
        }
        // Worked briefly: the next failure keeps doubling.
        backoff.working(now);
        assert_eq!(
            backoff.failed_with(now + Duration::from_secs(5), 0.0),
            Duration::from_secs(8)
        );
        // Worked for a minute: it counts as a first failure.
        backoff.working(now);
        assert_eq!(
            backoff.failed_with(now + Duration::from_secs(61), 0.0),
            Duration::from_secs(1)
        );
        backoff.failed_with(now, 0.0);
        backoff.reset();
        assert_eq!(backoff.failed_with(now, 0.0), Duration::from_secs(1));
    }
}
