//! The most recent audio of a session, by timeline frame, so an output that joins late can be
//! given what is still to play.

use std::collections::VecDeque;

pub struct Ring {
    channels: usize,
    capacity: usize,
    /// Timeline frame of the first sample held.
    start: u64,
    samples: VecDeque<f32>,
}

impl Ring {
    pub fn new(channels: u16, capacity_frames: usize) -> Self {
        Self {
            channels: usize::from(channels),
            capacity: capacity_frames,
            start: 0,
            samples: VecDeque::new(),
        }
    }

    /// The frame after the last one held.
    pub fn end(&self) -> u64 {
        self.start + (self.samples.len() / self.channels) as u64
    }

    /// Adds audio for frames starting at `at`. Audio that does not follow on from what is
    /// held, as after a seek, replaces it.
    pub fn push(&mut self, at: u64, samples: &[f32]) {
        if at != self.end() {
            self.samples.clear();
            self.start = at;
        }
        self.samples.extend(samples);
        let excess = (self.samples.len() / self.channels).saturating_sub(self.capacity);
        self.samples.drain(..excess * self.channels);
        self.start += excess as u64;
    }

    /// Everything held from `frame` on, if `frame` is still held.
    pub fn since(&self, frame: u64) -> Option<Vec<f32>> {
        if frame < self.start || frame > self.end() {
            return None;
        }
        let skip = (frame - self.start) as usize * self.channels;
        Some(self.samples.iter().skip(skip).copied().collect())
    }

    pub fn clear(&mut self) {
        self.samples.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_latest_frames() {
        let mut ring = Ring::new(2, 3);
        ring.push(0, &[0.0, 0.0, 1.0, 1.0]);
        ring.push(2, &[2.0, 2.0, 3.0, 3.0]);
        assert_eq!(ring.end(), 4);
        assert_eq!(ring.since(0), None, "dropped");
        assert_eq!(ring.since(1), Some(vec![1.0, 1.0, 2.0, 2.0, 3.0, 3.0]));
        assert_eq!(ring.since(3), Some(vec![3.0, 3.0]));
        assert_eq!(ring.since(4), Some(vec![]));
        assert_eq!(ring.since(5), None);
    }

    #[test]
    fn a_gap_starts_afresh() {
        let mut ring = Ring::new(1, 10);
        ring.push(0, &[0.0, 1.0]);
        ring.push(100, &[5.0]);
        assert_eq!(ring.since(0), None);
        assert_eq!(ring.since(100), Some(vec![5.0]));
    }
}
