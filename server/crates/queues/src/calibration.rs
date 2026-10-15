//! Calibrating how a group keeps time: each speaker in turn plays a chirp at a time the server
//! sets, a phone's microphone hears when each one arrived, and each speaker's `sync_adjust_ms`
//! is set so that they all arrive together. Only the differences between speakers count, so
//! what the phone adds itself, its microphone's latency and its clock's error, cancels out.

use bopsie_audio::timeline::Micros;
use serde::Serialize;

/// The sound each speaker plays: an exponential sweep, which a recording can find to a fraction
/// of a millisecond, even in an echoing room.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Chirp {
    pub from_hz: f64,
    pub to_hz: f64,
    pub duration_ms: u32,
    /// The raised-cosine fade at each end, so that it starts and stops without a click.
    pub fade_ms: u32,
    /// Its peak, as a fraction of full scale.
    pub amplitude: f64,
}

pub const CHIRP: Chirp = Chirp {
    from_hz: 500.0,
    to_hz: 8_000.0,
    duration_ms: 150,
    fade_ms: 10,
    amplitude: 0.3,
};

/// A calibration as it was scheduled: what the phone listens for, and when.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Calibration {
    pub chirp: Chirp,
    /// Every chirp, in the order they play.
    pub chirps: Vec<Scheduled>,
    /// When the last one has died away, on the server's clock.
    pub ends_at: Micros,
    /// Devices the group carries in their own multiroom, which play their leader's chirps and
    /// are not measured.
    pub skipped: Vec<String>,
}

/// One chirp.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Scheduled {
    pub device: String,
    /// The device's name.
    pub name: String,
    /// The endpoint that plays it, whose `sync_adjust_ms` the result sets.
    pub endpoint: String,
    /// When it is due, on the server's clock.
    pub at: Micros,
}

/// How far apart the chirps are: long enough for a room's echo to die away, and for a speaker
/// to be this far off and still be told from the next.
pub const SLOT_MS: u32 = 1_000;
/// How many times each speaker chirps; the phone takes the median.
pub const ROUNDS: usize = 3;

impl Chirp {
    /// Its samples at `rate`, mono. The app builds the same ones to search for.
    pub fn samples(&self, rate: u32) -> Vec<f32> {
        let rate = f64::from(rate);
        let length = (f64::from(self.duration_ms) / 1000.0 * rate).round() as usize;
        let duration = length as f64 / rate;
        let fade = (f64::from(self.fade_ms) / 1000.0 * rate).round() as usize;
        let sweep = duration / (self.to_hz / self.from_hz).ln();
        (0..length)
            .map(|i| {
                let t = i as f64 / rate;
                let phase =
                    2.0 * std::f64::consts::PI * self.from_hz * sweep * ((t / sweep).exp() - 1.0);
                let edge = i.min(length - 1 - i);
                let envelope = if edge < fade {
                    0.5 - 0.5 * (std::f64::consts::PI * edge as f64 / fade as f64).cos()
                } else {
                    1.0
                };
                (self.amplitude * envelope * phase.sin()) as f32
            })
            .collect()
    }
}

/// When the `index`th of `speakers` chirps in `round`, from the start of the calibration.
pub fn offset(round: usize, index: usize, speakers: usize) -> Micros {
    (round * speakers + index) as Micros * Micros::from(SLOT_MS) * 1_000
}

/// How long a calibration of `speakers` lasts, its last chirp's echo included.
pub fn length(speakers: usize) -> Micros {
    offset(ROUNDS, 0, speakers)
}

/// One speaker's audio for a calibration: silence, with its chirp in each of its slots, as
/// `channels`-channel samples at `rate`.
pub fn track(index: usize, speakers: usize, rate: u32, channels: u16) -> Vec<f32> {
    let frames = |micros: Micros| (micros as u64 * u64::from(rate) / 1_000_000) as usize;
    let chirp = CHIRP.samples(rate);
    let channels = usize::from(channels);
    let mut out = vec![0.0; frames(length(speakers)) * channels];
    for round in 0..ROUNDS {
        let start = frames(offset(round, index, speakers));
        for (i, &sample) in chirp.iter().enumerate() {
            let frame = (start + i) * channels;
            out[frame..frame + channels].fill(sample);
        }
    }
    out
}

/// Each endpoint's new `sync_adjust_ms`, from when its chirps arrived, `delay_ms` after they
/// were due, and the adjustment it played them with. The others are made to wait for the
/// latest, so that none has to play earlier than it can, and then all are moved together so
/// that the least is 0: calibrating again changes nothing that is already in step.
pub fn adjustments(measured: &[(String, f64, i32)]) -> Vec<(String, i32)> {
    let Some(latest) = measured.iter().map(|(_, delay, _)| *delay).reduce(f64::max) else {
        return Vec::new();
    };
    let wanted: Vec<(String, f64)> = measured
        .iter()
        .map(|(id, delay, current)| (id.clone(), f64::from(*current) + latest - delay))
        .collect();
    let least = wanted
        .iter()
        .map(|(_, adjust)| *adjust)
        .reduce(f64::min)
        .unwrap_or(0.0);
    wanted
        .into_iter()
        .map(|(id, adjust)| (id, (adjust - least).round() as i32))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chirp_sweeps_up_within_its_amplitude_and_fades_in_and_out() {
        let samples = CHIRP.samples(48_000);
        assert_eq!(samples.len(), 7_200);
        let peak = samples.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
        assert!(peak <= 0.3 + 1e-6 && peak > 0.29, "{peak}");
        assert!(samples[0].abs() < 1e-6 && samples[7_199].abs() < 0.01);
        // As computed apart from this and the app's implementation, which tests the same.
        for (i, expected) in [
            (100, 0.011_768_940_427_152_442),
            (1_000, -0.288_898_660_890_510_4),
            (5_000, 0.073_301_881_076_231_28),
            (7_000, -0.098_504_481_040_266_8),
        ] {
            assert!((f64::from(samples[i]) - expected).abs() < 1e-6, "{i}");
        }
        // Zero crossings come faster at the end than at the start.
        let crossings = |range: std::ops::Range<usize>| {
            samples[range]
                .windows(2)
                .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
                .count()
        };
        assert!(crossings(6_000..7_000) > 5 * crossings(1_000..2_000));
    }

    #[test]
    fn each_speaker_chirps_in_its_own_slots() {
        assert_eq!(offset(0, 1, 3), 1_000_000);
        assert_eq!(offset(1, 0, 3), 3_000_000);
        assert_eq!(length(2), 6_000_000);
        let track = track(1, 2, 1_000, 2);
        assert_eq!(track.len(), 6_000 * 2);
        let loud = |frame: usize| track[frame * 2..frame * 2 + 2].iter().any(|s| *s != 0.0);
        // Its chirps start at 1 s, 3 s and 5 s; at 1 kHz, a chirp is 150 frames.
        assert!(!loud(500) && loud(1_050) && loud(3_050) && loud(5_050) && !loud(2_500));
        assert_eq!(
            track[1_050 * 2],
            track[1_050 * 2 + 1],
            "every channel alike"
        );
    }

    #[test]
    fn the_others_wait_for_the_latest_and_the_least_is_zero() {
        let measured = [
            ("kitchen".to_owned(), 40.0, 0),
            ("wiim".to_owned(), 15.4, 0),
            ("phone".to_owned(), 35.0, 0),
        ];
        assert_eq!(
            adjustments(&measured),
            [
                ("kitchen".to_owned(), 0),
                ("wiim".to_owned(), 25),
                ("phone".to_owned(), 5)
            ]
        );
        // Measured again once in step, nothing moves.
        let again = [
            ("kitchen".to_owned(), 40.2, 0),
            ("wiim".to_owned(), 39.9, 25),
            ("phone".to_owned(), 40.0, 5),
        ];
        assert_eq!(
            adjustments(&again),
            [
                ("kitchen".to_owned(), 0),
                ("wiim".to_owned(), 25),
                ("phone".to_owned(), 5)
            ]
        );
        assert!(adjustments(&[]).is_empty());
    }
}
