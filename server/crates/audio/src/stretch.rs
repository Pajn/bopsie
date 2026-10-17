//! Playing faster or slower without changing pitch, as for an audiobook at 1.5×: WSOLA, which
//! lays overlapping slices of the audio back down closer together or further apart, each
//! joined where its waveform best continues the last. It suits speech, as browsers' own
//! playback speed does.

use std::num::{NonZeroU16, NonZeroU32};

use wsola::TimeStretch;

use crate::PcmFormat;

/// The slowest and fastest rates played.
pub const MIN_RATE: f32 = 0.5;
pub const MAX_RATE: f32 = 3.0;

/// Stretches a stream of interleaved audio in `format` to play at `rate`.
pub struct Stretch {
    inner: TimeStretch,
}

impl Stretch {
    /// A stretch to `rate`, or `None` at 1.0, which plays the audio as it is.
    pub fn new(format: PcmFormat, rate: f32) -> Option<Self> {
        if (rate - 1.0).abs() < 0.001 || !rate.is_finite() {
            return None;
        }
        let mut inner = TimeStretch::new(
            NonZeroU32::new(format.sample_rate)?,
            NonZeroU16::new(format.channels)?,
        )
        .ok()?;
        inner.set_tempo(rate.clamp(MIN_RATE, MAX_RATE));
        Some(Self { inner })
    }

    /// What `samples` and what came before them play as, as far as it is ready.
    pub fn push(&mut self, samples: &[f32]) -> Vec<f32> {
        self.inner.push(samples);
        self.inner.pull(usize::MAX).collect()
    }

    /// The rest, at the end of the stream.
    pub fn finish(&mut self) -> Vec<f32> {
        self.inner.flush().collect()
    }

    /// Forgets what it holds, as after a seek.
    pub fn reset(&mut self) {
        self.inner.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORMAT: PcmFormat = PcmFormat {
        sample_rate: 8_000,
        channels: 1,
    };

    /// The strongest frequency in `samples`, in Hz, by counting rising zero crossings.
    fn frequency(samples: &[f32], rate: u32) -> f64 {
        let crossings = samples
            .windows(2)
            .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
            .count();
        crossings as f64 * f64::from(rate) / samples.len() as f64
    }

    #[test]
    fn faster_is_shorter_at_the_same_pitch() {
        let tone: Vec<f32> = (0..FORMAT.sample_rate * 6)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 8_000.0).sin() * 0.5)
            .collect();
        let mut stretch = Stretch::new(FORMAT, 1.5).unwrap();
        let mut out = Vec::new();
        for chunk in tone.chunks(1_000) {
            out.extend(stretch.push(chunk));
        }
        out.extend(stretch.finish());
        // 6 s at 1.5× is 4 s.
        let seconds = out.len() as f64 / 8_000.0;
        assert!((seconds - 4.0).abs() < 0.05, "{seconds}");
        let pitch = frequency(&out[8_000..24_000], 8_000);
        assert!((pitch - 440.0).abs() < 5.0, "{pitch}");
        assert!(Stretch::new(FORMAT, 1.0).is_none());
    }
}
