//! A Spotify track as the engine plays it: fetched, decrypted and decoded on a thread of its
//! own, so that the engine waits for it as for any live input rather than for the network.
//!
//! A track is Ogg Vorbis, at 44.1 kHz in stereo, behind a header of Spotify's own. librespot
//! fetches it in ranges as it is read, which makes seeking cheap.

use std::collections::VecDeque;
use std::io::{Read, Seek, SeekFrom};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use bopsie_audio::PcmFormat;
use bopsie_audio::decode::{DecodeError, Decoder, Read as Decoded, SymphoniaDecoder};
use bopsie_audio::engine::Media;
use bopsie_audio::loudness::{Loudness, REFERENCE_LUFS};
use librespot_audio::{AudioDecrypt, AudioFile};
use librespot_core::{SpotifyId, SpotifyUri};
use librespot_metadata::audio::AudioFileFormat;
use librespot_metadata::{Metadata, Track};
use symphonia::core::io::MediaSource;
use tokio::runtime::Handle;

use crate::account::SpotifyAccount;

/// What every Spotify track decodes to.
pub const FORMAT: PcmFormat = PcmFormat {
    sample_rate: 44_100,
    channels: 2,
};
/// Spotify's own header before the Ogg stream.
const HEADER: u64 = 0xa7;
/// Where in the header Spotify keeps the track's loudness: its track gain, track peak, album
/// gain and album peak, as little-endian `f32`s.
const LOUDNESS_AT: u64 = 144;
/// The loudness Spotify's gains bring audio to, in LUFS by EBU R128: not the -14 LUFS Spotify
/// plays at, but about -17, as the `spotify-loudness-probe` example measured of a library's
/// tracks, each within half a dB.
pub const SPOTIFY_REFERENCE_LUFS: f32 = -17.0;
/// The formats played, best first.
const PREFERRED: &[AudioFileFormat] = &[
    AudioFileFormat::OGG_VORBIS_320,
    AudioFileFormat::OGG_VORBIS_160,
    AudioFileFormat::OGG_VORBIS_96,
];
/// What librespot is told to expect, in bytes a second: as at 320 kbps.
const BYTES_PER_SECOND: usize = 40 * 1024;
/// How much decoded audio is kept ahead of the engine.
const AHEAD: Duration = Duration::from_secs(5);
/// How long a read waits for audio before it answers that there is none yet.
const READ_WAIT: Duration = Duration::from_millis(20);

/// The part of a stream after a header, as a seekable source for the decoder.
pub struct Subfile<T> {
    inner: Mutex<T>,
    offset: u64,
    length: u64,
}

impl<T: Read + Seek> Subfile<T> {
    /// `inner` from `offset` on, of `length` bytes in all.
    pub fn new(mut inner: T, offset: u64, length: u64) -> std::io::Result<Self> {
        inner.seek(SeekFrom::Start(offset))?;
        Ok(Self {
            inner: Mutex::new(inner),
            offset,
            length,
        })
    }
}

impl<T: Read + Seek> Read for Subfile<T> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.get_mut().unwrap().read(buf)
    }
}

impl<T: Read + Seek> Seek for Subfile<T> {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        let to = match to {
            SeekFrom::Start(at) => SeekFrom::Start(at + self.offset),
            other => other,
        };
        Ok(self.inner.get_mut().unwrap().seek(to)? - self.offset)
    }
}

impl<T: Read + Seek + Send> MediaSource for Subfile<T> {
    fn is_seekable(&self) -> bool {
        true
    }
    fn byte_len(&self) -> Option<u64> {
        Some(self.length.saturating_sub(self.offset))
    }
}

/// A Spotify track to play, by its id.
pub struct SpotifyMedia {
    account: SpotifyAccount,
    track: SpotifyId,
    runtime: Handle,
}

impl SpotifyMedia {
    /// Track `track` of `account`, fetched on `runtime`.
    pub fn new(account: SpotifyAccount, track: SpotifyId, runtime: Handle) -> Self {
        Self {
            account,
            track,
            runtime,
        }
    }
}

impl Media for SpotifyMedia {
    fn open(&self, _format: PcmFormat) -> Result<Box<dyn Decoder>, DecodeError> {
        let (account, track, runtime) = (self.account.clone(), self.track, self.runtime.clone());
        Ok(Box::new(Fetching::start(move || {
            runtime.block_on(fetch(&account, track))
        })))
    }
}

/// How loud a track is, as the header before its Ogg stream says, as ReplayGain 2 gains. None
/// when it says nothing.
pub fn header_loudness(stream: &mut (impl Read + Seek)) -> std::io::Result<Option<Loudness>> {
    stream.seek(SeekFrom::Start(LOUDNESS_AT))?;
    let mut bytes = [0u8; 16];
    stream.read_exact(&mut bytes)?;
    let value = |i: usize| f32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
    let (track_gain, track_peak, album_gain, album_peak) =
        (value(0), value(4), value(8), value(12));
    if !(track_gain.is_finite() && track_peak.is_finite() && track_peak > 0.0) {
        return Ok(None);
    }
    let gain = |spotify: f32| spotify + (REFERENCE_LUFS - SPOTIFY_REFERENCE_LUFS);
    let album = album_gain.is_finite() && album_peak.is_finite() && album_peak > 0.0;
    Ok(Some(Loudness {
        track_gain_db: gain(track_gain),
        track_peak,
        album_gain_db: album.then(|| gain(album_gain)),
        album_peak: album.then_some(album_peak),
    }))
}

/// Fetches track `id`: its best Ogg Vorbis file, decrypted, past Spotify's header, and how
/// loud the header says it is.
async fn fetch(
    account: &SpotifyAccount,
    id: SpotifyId,
) -> Result<(Box<dyn MediaSource>, Option<Loudness>), DecodeError> {
    let unavailable =
        |why: String| DecodeError::Unsupported(format!("Spotify track {id:?}: {why}"));
    let session = account
        .session()
        .await
        .map_err(|e| unavailable(e.to_string()))?;
    let mut track = Track::get(&session, &SpotifyUri::Track { id })
        .await
        .map_err(|e| unavailable(e.to_string()))?;
    // A track without files plays as one of its alternatives, as Spotify's apps do.
    if track.files.is_empty() {
        for alternative in track.alternatives.iter() {
            if let Ok(found) = Track::get(&session, alternative).await
                && !found.files.is_empty()
            {
                track = found;
                break;
            }
        }
    }
    let (format, file) = PREFERRED
        .iter()
        .find_map(|format| Some((*format, *track.files.get(format)?)))
        .ok_or_else(|| unavailable("no Ogg Vorbis file".to_owned()))?;
    let track_id: SpotifyId = (&track.id)
        .try_into()
        .map_err(|e: librespot_core::Error| unavailable(e.to_string()))?;
    let key = session
        .audio_key()
        .request(track_id, file)
        .await
        .map_err(|e| unavailable(format!("no key: {e}")))?;
    let audio = AudioFile::open(&session, file, BYTES_PER_SECOND)
        .await
        .map_err(|e| unavailable(e.to_string()))?;
    let length = audio
        .get_stream_loader_controller()
        .map_err(|e| unavailable(e.to_string()))?
        .len() as u64;
    tracing::debug!(track = track.name, ?format, "playing from Spotify");
    let mut decrypted = AudioDecrypt::new(Some(key), audio);
    let loudness = header_loudness(&mut decrypted).unwrap_or_else(|error| {
        tracing::debug!(%error, "no loudness in the Spotify header");
        None
    });
    Ok((Box::new(Subfile::new(decrypted, HEADER, length)?), loudness))
}

#[derive(Default)]
struct State {
    samples: VecDeque<f32>,
    ended: bool,
    error: Option<String>,
    /// A seek asked for and not yet made.
    seek: Option<u64>,
    /// The engine let the track go.
    closed: bool,
    /// How loud the track is, once its header is read.
    loudness: Option<Loudness>,
}

struct Shared {
    state: Mutex<State>,
    changed: Condvar,
}

/// A track decoded on a thread of its own from a source opened there.
pub struct Fetching {
    shared: Arc<Shared>,
}

impl Fetching {
    /// Opens the source `open` gives, with how loud it is, on a thread of its own, and decodes
    /// it there as far as [`AHEAD`] in front of what is read.
    pub fn start(
        open: impl FnOnce() -> Result<(Box<dyn MediaSource>, Option<Loudness>), DecodeError>
        + Send
        + 'static,
    ) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::default(),
            changed: Condvar::new(),
        });
        let decoding = shared.clone();
        std::thread::Builder::new()
            .name("spotify-track".to_owned())
            .spawn(move || {
                let result = open().and_then(|(source, loudness)| {
                    decoding.state.lock().unwrap().loudness = loudness;
                    decode(&decoding, source)
                });
                let mut state = decoding.state.lock().unwrap();
                match result {
                    Ok(()) => state.ended = true,
                    Err(error) => state.error = Some(error.to_string()),
                }
                drop(state);
                decoding.changed.notify_all();
            })
            .expect("spawning a thread");
        Self { shared }
    }
}

fn decode(shared: &Shared, source: Box<dyn MediaSource>) -> Result<(), DecodeError> {
    let mut decoder = SymphoniaDecoder::from_source(source, Some("ogg"))?;
    if decoder.format() != FORMAT {
        return Err(DecodeError::Unsupported(format!(
            "Spotify sent {:?}",
            decoder.format()
        )));
    }
    let room = (AHEAD.as_secs_f64() * f64::from(FORMAT.sample_rate)) as usize
        * usize::from(FORMAT.channels);
    let mut out = Vec::new();
    loop {
        let seek = {
            let mut state = shared
                .changed
                .wait_while(shared.state.lock().unwrap(), |state| {
                    !state.closed && state.seek.is_none() && state.samples.len() >= room
                })
                .unwrap();
            if state.closed {
                return Ok(());
            }
            state.seek.take()
        };
        if let Some(frame) = seek {
            decoder.seek(frame)?;
            continue;
        }
        out.clear();
        let read = decoder.read(&mut out)?;
        let mut state = shared.state.lock().unwrap();
        // Decoded before a seek that came meanwhile: not wanted.
        if state.seek.is_none() {
            state.samples.extend(&out);
        }
        drop(state);
        shared.changed.notify_all();
        if read == Decoded::End {
            return Ok(());
        }
    }
}

impl Decoder for Fetching {
    fn format(&self) -> PcmFormat {
        FORMAT
    }

    fn read(&mut self, out: &mut Vec<f32>) -> Result<Decoded, DecodeError> {
        let shared = &*self.shared;
        let mut state = shared
            .changed
            .wait_timeout_while(shared.state.lock().unwrap(), READ_WAIT, |state| {
                state.samples.is_empty() && !state.ended && state.error.is_none()
            })
            .unwrap()
            .0;
        if state.samples.is_empty() {
            if let Some(error) = &state.error {
                return Err(DecodeError::Corrupt(error.clone()));
            }
            return Ok(if state.ended {
                Decoded::End
            } else {
                Decoded::Pending
            });
        }
        let take = state.samples.len().min(8_192);
        out.extend(state.samples.drain(..take));
        drop(state);
        shared.changed.notify_all();
        Ok(Decoded::Data)
    }

    fn loudness(&self) -> Option<Loudness> {
        self.shared.state.lock().unwrap().loudness
    }

    fn seek(&mut self, frame: u64) -> Result<u64, DecodeError> {
        let mut state = self.shared.state.lock().unwrap();
        state.samples.clear();
        state.ended = false;
        state.seek = Some(frame);
        drop(state);
        self.shared.changed.notify_all();
        Ok(frame)
    }
}

impl Drop for Fetching {
    fn drop(&mut self) {
        self.shared.state.lock().unwrap().closed = true;
        self.shared.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn a_subfile_starts_after_its_header() {
        let bytes: Vec<u8> = (0..=255).collect();
        let mut sub = Subfile::new(Cursor::new(bytes), 0xa7, 256).unwrap();
        let mut first = [0u8; 2];
        sub.read_exact(&mut first).unwrap();
        assert_eq!(first, [0xa7, 0xa8]);
        assert_eq!(sub.seek(SeekFrom::Start(3)).unwrap(), 3);
        sub.read_exact(&mut first).unwrap();
        assert_eq!(first, [0xaa, 0xab]);
        assert_eq!(sub.byte_len(), Some(256 - 0xa7));
    }

    #[test]
    fn the_headers_loudness_is_read_as_replaygain() {
        let mut header = vec![0u8; 0xa7];
        for (i, value) in [-5.5f32, 0.9, -3.0, 0.95].into_iter().enumerate() {
            let at = LOUDNESS_AT as usize + i * 4;
            header[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        assert_eq!(
            header_loudness(&mut Cursor::new(&header)).unwrap(),
            Some(Loudness {
                track_gain_db: -6.5,
                track_peak: 0.9,
                album_gain_db: Some(-4.0),
                album_peak: Some(0.95),
            })
        );
        // A header that says nothing.
        assert_eq!(
            header_loudness(&mut Cursor::new(vec![0u8; 0xa7])).unwrap(),
            None
        );
    }

    #[test]
    fn a_track_that_cannot_open_says_why() {
        let mut fetching = Fetching::start(|| Err(DecodeError::Unsupported("no key".to_owned())));
        assert_eq!(fetching.loudness(), None);
        let mut out = Vec::new();
        let error = loop {
            match fetching.read(&mut out) {
                Ok(Decoded::Pending) => continue,
                Ok(other) => panic!("{other:?}"),
                Err(error) => break error,
            }
        };
        assert!(error.to_string().contains("no key"), "{error}");
    }
}
