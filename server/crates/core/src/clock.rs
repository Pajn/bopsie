//! The server's monotonic clock, in microseconds. Sendspin timestamps and every timeline use it,
//! so it must never jump: it counts from an arbitrary point early in the process's life.

use std::sync::OnceLock;
use std::time::Instant;

/// Microseconds on the server's monotonic clock.
pub type Micros = i64;

static ORIGIN: OnceLock<Instant> = OnceLock::new();

pub fn now() -> Micros {
    ORIGIN.get_or_init(Instant::now).elapsed().as_micros() as Micros
}

/// The Unix time, in milliseconds, at which the monotonic `instant` falls, as protocols that
/// schedule by wall clock take it. Read against the wall clock now, so a later change to the
/// wall clock moves it.
pub fn unix_ms(instant: Micros) -> u64 {
    let wall = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_micros() as i64);
    ((wall + instant - now()) / 1_000).max(0) as u64
}

#[cfg(test)]
mod tests {
    #[test]
    fn unix_time_of_an_instant() {
        let wall_now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let soon = super::unix_ms(super::now() + 2_000_000);
        assert!(soon.abs_diff(wall_now + 2_000) <= 5, "{soon} vs {wall_now}");
    }

    #[test]
    fn never_goes_backwards() {
        let mut last = super::now();
        for _ in 0..1000 {
            let now = super::now();
            assert!(now >= last);
            last = now;
        }
    }
}
