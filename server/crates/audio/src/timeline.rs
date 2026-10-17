//! A session's timeline: a frame counter at the session's rate, anchored to the server's
//! monotonic clock. Outputs translate frames into whatever time their protocol speaks.

pub use bopsie_core::clock::Micros;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeline {
    rate: u32,
    /// When frame 0 plays.
    anchor: Micros,
}

impl Timeline {
    pub fn new(rate: u32, anchor: Micros) -> Self {
        assert!(rate > 0, "a timeline needs a sample rate");
        Self { rate, anchor }
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// The same frames, each playing `ms` later.
    pub fn delayed(&self, ms: i32) -> Self {
        Self {
            rate: self.rate,
            anchor: self.anchor + Micros::from(ms) * 1_000,
        }
    }

    /// When `frame` plays: the first whole microsecond at or after it starts, so that
    /// [`Timeline::frame_at`] maps the instant back to the same frame.
    pub fn instant_of(&self, frame: u64) -> Micros {
        let rate = i128::from(self.rate);
        self.anchor + ((i128::from(frame) * 1_000_000 + rate - 1) / rate) as Micros
    }

    /// The frame playing at `instant`: the last one to start at or before it. Before the
    /// anchor, frame 0.
    pub fn frame_at(&self, instant: Micros) -> u64 {
        let since = i128::from(instant - self.anchor).max(0);
        (since * i128::from(self.rate) / 1_000_000) as u64
    }

    /// How many frames last `micros`, rounded up so that a lead is never short.
    pub fn frames_in(&self, micros: Micros) -> u64 {
        let micros = i128::from(micros.max(0));
        ((micros * i128::from(self.rate) + 999_999) / 1_000_000) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_and_instants() {
        let t = Timeline::new(48_000, 1_000_000);
        assert_eq!(t.instant_of(0), 1_000_000);
        assert_eq!(t.instant_of(48_000), 2_000_000);
        assert_eq!(t.instant_of(1), 1_000_021);
        assert_eq!(t.frame_at(2_000_000), 48_000);
        assert_eq!(t.frame_at(1_000_021), 1);
        assert_eq!(t.frame_at(1_000_020), 0);
        assert_eq!(t.frame_at(0), 0, "before the anchor");
        assert_eq!(t.frames_in(1), 1, "rounded up");
        assert_eq!(t.frames_in(500_000), 24_000);
    }

    #[test]
    fn round_trips_without_drift_over_days() {
        let t = Timeline::new(44_100, 0);
        let frame = 44_100u64 * 60 * 60 * 24 * 3 + 12_345;
        assert_eq!(t.frame_at(t.instant_of(frame)), frame);
    }
}
