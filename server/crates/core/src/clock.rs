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

#[cfg(test)]
mod tests {
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
