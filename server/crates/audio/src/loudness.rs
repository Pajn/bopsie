//! Volume normalisation: how loud a track is, and the gain that brings it to a target loudness.
//!
//! Every source's figures are kept as ReplayGain 2 gains: the decibels that bring the audio to
//! −18 LUFS. Peaks are linear sample peaks, 1.0 being full scale.

use std::path::{Path, PathBuf};

use ebur128::EbuR128;

use crate::PcmFormat;
use crate::decode::{DecodeError, Read};
use crate::engine::{self, Input};

/// The loudness ReplayGain 2 gains bring audio to, in LUFS.
pub const REFERENCE_LUFS: f32 = -18.0;

/// How loud a track is, as the gains that bring it, and its album, to [`REFERENCE_LUFS`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Loudness {
    pub track_gain_db: f32,
    /// The track's sample peak; 0 when unknown.
    pub track_peak: f32,
    pub album_gain_db: Option<f32>,
    pub album_peak: Option<f32>,
}

/// How a track is to be normalised.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Normalise {
    /// The loudness to bring it to, in LUFS.
    pub target_lufs: f32,
    /// Its album's gain rather than its own, as when an album plays in order.
    pub album: bool,
}

/// Which of a track's gains was applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Album,
    Track,
}

impl Loudness {
    /// The gain `normalise` calls for, in dB, and which of the track's gains it came from. A
    /// track without an album gain takes its own. The gain is held down so that the peak does not
    /// go past full scale.
    pub fn gain_db(&self, normalise: Normalise) -> (f32, Mode) {
        let (gain, peak, mode) = match (normalise.album, self.album_gain_db) {
            (true, Some(gain)) => (gain, self.album_peak.unwrap_or(0.0), Mode::Album),
            _ => (self.track_gain_db, self.track_peak, Mode::Track),
        };
        let mut db = gain + (normalise.target_lufs - REFERENCE_LUFS);
        if peak > 0.0 {
            db = db.min(-20.0 * peak.log10());
        }
        (db, mode)
    }

    /// The factor samples are multiplied by.
    pub fn factor(&self, normalise: Normalise) -> f32 {
        10f32.powf(self.gain_db(normalise).0 / 20.0)
    }
}

/// Measures the files of one album, as they play: each track's loudness and peak, and the
/// album's over all of them together. A file that cannot be decoded, or is silent, is `None`
/// and left out of the album's.
pub fn measure_album(paths: &[PathBuf], ffmpeg: Option<&Path>) -> Vec<Option<Loudness>> {
    let measured: Vec<Option<(EbuR128, f32)>> = paths
        .iter()
        .map(|path| match measure(path, ffmpeg) {
            Ok(measured) => measured,
            Err(error) => {
                tracing::debug!(path = %path.display(), %error, "cannot measure");
                None
            }
        })
        .collect();
    let states: Vec<&EbuR128> = measured.iter().flatten().map(|(state, _)| state).collect();
    let album_gain = EbuR128::loudness_global_multiple(states.into_iter())
        .ok()
        .filter(|lufs| lufs.is_finite())
        .map(|lufs| REFERENCE_LUFS - lufs as f32);
    let album_peak = measured
        .iter()
        .flatten()
        .map(|(_, peak)| *peak)
        .fold(0f32, f32::max);
    measured
        .into_iter()
        .map(|measured| {
            let (state, peak) = measured?;
            let lufs = state.loudness_global().ok().filter(|l| l.is_finite())?;
            Some(Loudness {
                track_gain_db: REFERENCE_LUFS - lufs as f32,
                track_peak: peak,
                album_gain_db: album_gain,
                album_peak: album_gain.map(|_| album_peak),
            })
        })
        .collect()
}

/// The file's loudness state and sample peak, as it plays: decoded as the engine decodes it,
/// mono counted on both channels as it is heard. `None` for a file with no audio.
fn measure(path: &Path, ffmpeg: Option<&Path>) -> Result<Option<(EbuR128, f32)>, DecodeError> {
    let input = Input::File(path.to_owned());
    let mut decoder = engine::open(&input, MEASURED, ffmpeg)?;
    let format = decoder.format();
    let mono = format.channels == 1;
    let channels = if mono { 2 } else { format.channels };
    let mut state = EbuR128::new(
        u32::from(channels),
        format.sample_rate,
        ebur128::Mode::I | ebur128::Mode::SAMPLE_PEAK | ebur128::Mode::HISTOGRAM,
    )
    .map_err(|error| DecodeError::Unsupported(error.to_string()))?;
    let mut decoded = Vec::new();
    let mut doubled = Vec::new();
    let mut any = false;
    loop {
        decoded.clear();
        match decoder.read(&mut decoded)? {
            Read::End => break,
            Read::Pending => continue,
            Read::Data => {}
        }
        any |= !decoded.is_empty();
        let frames = if mono {
            doubled.clear();
            doubled.extend(decoded.iter().flat_map(|&s| [s, s]));
            &doubled
        } else {
            &decoded
        };
        state
            .add_frames_f32(frames)
            .map_err(|error| DecodeError::Corrupt(error.to_string()))?;
    }
    if !any {
        return Ok(None);
    }
    let peak = (0..u32::from(channels))
        .filter_map(|channel| state.sample_peak(channel).ok())
        .fold(0f64, f64::max) as f32;
    Ok(Some((state, peak)))
}

/// What ffmpeg is asked for when it decodes a file to measure; Symphonia decodes as the file
/// is.
const MEASURED: PcmFormat = PcmFormat {
    sample_rate: 48_000,
    channels: 2,
};

/// A ReplayGain gain as tags write it, such as `-6.20 dB` or `+1.5`.
pub fn parse_gain(text: &str) -> Option<f32> {
    let number = text.trim();
    let number = number
        .strip_suffix("dB")
        .or_else(|| number.strip_suffix("db"))
        .or_else(|| number.strip_suffix("DB"))
        .unwrap_or(number)
        .trim();
    let gain: f32 = number.strip_prefix('+').unwrap_or(number).parse().ok()?;
    gain.is_finite().then_some(gain)
}

/// A ReplayGain peak as tags write it, such as `0.988525`.
pub fn parse_peak(text: &str) -> Option<f32> {
    let peak: f32 = text.trim().parse().ok()?;
    (peak.is_finite() && peak >= 0.0).then_some(peak)
}

/// An Opus R128 gain (`R128_TRACK_GAIN`): a Q7.8 number of dB to −23 LUFS, as a ReplayGain
/// gain.
pub fn parse_r128(text: &str) -> Option<f32> {
    let q78: i32 = text.trim().parse().ok()?;
    (-32768..=32767)
        .contains(&q78)
        .then(|| q78 as f32 / 256.0 + (REFERENCE_LUFS - -23.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sine at 1 kHz, `seconds` long, at `amplitude` of full scale, as a 16-bit WAV.
    fn sine(path: &Path, amplitude: f64, seconds: u32, channels: u16) {
        let rate = 48_000u32;
        let samples: Vec<i16> = (0..rate * seconds)
            .flat_map(|i| {
                let value = amplitude
                    * (2.0 * std::f64::consts::PI * 1_000.0 * f64::from(i) / f64::from(rate)).sin();
                let sample = (value * 32_767.0).round() as i16;
                std::iter::repeat_n(sample, usize::from(channels))
            })
            .collect();
        bopsie_fixtures::write_wav(path, rate, channels, &samples).unwrap();
    }

    #[test]
    fn files_are_measured_as_they_are_heard() {
        let dir = tempfile::tempdir().unwrap();
        let loud = dir.path().join("loud.wav");
        let quiet = dir.path().join("quiet.wav");
        let mono = dir.path().join("mono.wav");
        let broken = dir.path().join("broken.wav");
        // A full-scale 1 kHz sine on both channels is 0 LUFS, as BS.1770 is calibrated; half as
        // loud is 6 dB quieter.
        sine(&loud, 1.0, 3, 2);
        sine(&quiet, 0.5, 3, 2);
        sine(&mono, 0.5, 3, 1);
        std::fs::write(&broken, b"not audio").unwrap();
        let measured = measure_album(&[loud, quiet, mono, broken], None);
        let gain = |i: usize| measured[i].unwrap().track_gain_db;
        assert!((gain(0) - -18.0).abs() < 0.2, "{}", gain(0));
        assert!((gain(1) - gain(0) - 6.02).abs() < 0.1, "{}", gain(1));
        // Mono plays on both channels, so it is heard as loud as the same in stereo.
        assert!((gain(2) - gain(1)).abs() < 0.1, "{}", gain(2));
        assert!(measured[3].is_none());
        let peak = measured[0].unwrap().track_peak;
        assert!((peak - 1.0).abs() < 0.01, "{peak}");
        // The album's gain lies between its loudest and quietest tracks', and its peak is the
        // loudest's.
        let album = measured[1].unwrap();
        let album_gain = album.album_gain_db.unwrap();
        assert!(gain(0) < album_gain && album_gain < gain(1), "{album_gain}");
        assert_eq!(album.album_peak, Some(peak));
    }

    const LOUD: Loudness = Loudness {
        track_gain_db: -8.0,
        track_peak: 1.0,
        album_gain_db: Some(-6.0),
        album_peak: Some(1.0),
    };

    fn normalise(target_lufs: f32, album: bool) -> Normalise {
        Normalise { target_lufs, album }
    }

    #[test]
    fn the_gain_is_the_tracks_or_its_albums_moved_to_the_target() {
        assert_eq!(LOUD.gain_db(normalise(-18.0, false)), (-8.0, Mode::Track));
        assert_eq!(LOUD.gain_db(normalise(-18.0, true)), (-6.0, Mode::Album));
        assert_eq!(LOUD.gain_db(normalise(-14.0, false)), (-4.0, Mode::Track));
        let alone = Loudness {
            album_gain_db: None,
            album_peak: None,
            ..LOUD
        };
        assert_eq!(alone.gain_db(normalise(-18.0, true)), (-8.0, Mode::Track));
    }

    #[test]
    fn a_raised_track_stops_short_of_clipping() {
        let quiet = Loudness {
            track_gain_db: 9.0,
            track_peak: 0.5,
            album_gain_db: None,
            album_peak: None,
        };
        let (db, _) = quiet.gain_db(normalise(-18.0, false));
        assert!((db - 6.0206).abs() < 0.001, "{db}");
        assert!((quiet.factor(normalise(-18.0, false)) * 0.5 - 1.0).abs() < 0.001);
        // Without a peak nothing holds it down.
        let unknown = Loudness {
            track_peak: 0.0,
            ..quiet
        };
        assert_eq!(unknown.gain_db(normalise(-18.0, false)).0, 9.0);
    }

    #[test]
    fn tags_are_read_as_they_are_written() {
        assert_eq!(parse_gain("-6.20 dB"), Some(-6.2));
        assert_eq!(parse_gain("+1.5 dB"), Some(1.5));
        assert_eq!(parse_gain(" 3 "), Some(3.0));
        assert_eq!(parse_gain("loud"), None);
        assert_eq!(parse_peak("0.988525"), Some(0.988525));
        assert_eq!(parse_peak("-1"), None);
        // −256 is −1 dB to −23 LUFS: +4 dB to −18.
        assert_eq!(parse_r128("-256"), Some(4.0));
        assert_eq!(parse_r128("0"), Some(5.0));
        assert_eq!(parse_r128("99999"), None);
    }
}
