//! Volume normalisation: how loud a track is, and the gain that brings it to a target loudness.
//!
//! Every source's figures are kept as ReplayGain 2 gains: the decibels that bring the audio to
//! −18 LUFS. Peaks are linear sample peaks, 1.0 being full scale.

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
