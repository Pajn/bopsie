//! A session's audio to a LinkPlay device: a WAV stream with no end, served over HTTP from the
//! server, which the device is told to play and then pulls at its own pace.
//!
//! A device cannot be told when to start; it starts as soon as it has buffered enough, a few
//! seconds after it is told to play. So the sink tells it that long before the audio's place on
//! the timeline, as long as that device last took, and measures how long it takes this time from
//! the countdown its `getPlayerStatus` position reports: negative until the audio is audible.

use std::collections::HashMap;
use std::convert::Infallible;
use std::io;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use bopsie_audio::OutputFormat;
use bopsie_audio::sink::Sink;
use bopsie_audio::timeline::{Micros, Timeline};
use bopsie_core::clock;
use tokio::sync::mpsc;

use crate::endpoint::Shared;
use crate::status::Transport;

/// Audio kept for a device that has not pulled it yet, in chunks as the engine writes them.
const QUEUED_CHUNKS: usize = 4096;
/// How long the session waits for a device to take audio before giving it up.
const STALL_LIMIT: Duration = Duration::from_secs(15);
/// What the first start of a device is assumed to take, from `play` to audible.
pub const FIRST_DELAY_MS: i64 = 2_700;
/// How often the countdown is read after `play`, and for how long.
const COUNTDOWN_POLL: Duration = Duration::from_millis(100);
const COUNTDOWN_WATCH: Duration = Duration::from_secs(8);
/// How often UPnP's position is read after `play`, on devices without a countdown.
const TICK_POLL: Duration = Duration::from_millis(40);
/// How long after audio is audible UPnP's whole-second position turns to 1: a second, and
/// about 130 ms more, as measured on a WiiM Amp Pro against its countdown.
const TICK_AT_MS: i64 = 1_130;

/// Streams waiting to be pulled, by token.
#[derive(Clone, Default)]
pub struct StreamHub {
    waiting: Arc<Mutex<HashMap<String, mpsc::Receiver<Vec<u8>>>>>,
}

impl StreamHub {
    fn register(&self, token: String, audio: mpsc::Receiver<Vec<u8>>) {
        self.waiting.lock().unwrap().insert(token, audio);
    }

    fn forget(&self, token: &str) {
        self.waiting.lock().unwrap().remove(token);
    }

    /// The stream for `token`, once: a device that asks again gets nothing.
    fn take(&self, token: &str) -> Option<mpsc::Receiver<Vec<u8>>> {
        self.waiting.lock().unwrap().remove(token)
    }

    /// `GET /linkplay/stream/{token}.wav`.
    pub fn routes(&self) -> Router {
        Router::new()
            .route("/linkplay/stream/{token}", get(serve))
            .with_state(self.clone())
    }
}

async fn serve(State(hub): State<StreamHub>, Path(token): Path<String>) -> Response {
    let token = token.trim_end_matches(".wav");
    let Some(audio) = hub.take(token) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let body = futures::stream::unfold(audio, |mut audio| async move {
        audio
            .recv()
            .await
            .map(|chunk| (Ok::<_, Infallible>(chunk), audio))
    });
    (
        [(header::CONTENT_TYPE, "audio/wav")],
        Body::from_stream(body),
    )
        .into_response()
}

/// A WAV header for PCM in `format` that goes on for as long as a WAV can say.
pub fn wav_header(format: OutputFormat) -> Vec<u8> {
    let block = format.bytes_per_frame() as u16;
    let mut header = Vec::with_capacity(44);
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&u32::MAX.to_le_bytes());
    header.extend_from_slice(b"WAVEfmt ");
    header.extend_from_slice(&16u32.to_le_bytes());
    header.extend_from_slice(&1u16.to_le_bytes());
    header.extend_from_slice(&format.channels.to_le_bytes());
    header.extend_from_slice(&format.sample_rate.to_le_bytes());
    header.extend_from_slice(&(format.sample_rate * u32::from(block)).to_le_bytes());
    header.extend_from_slice(&block.to_le_bytes());
    header.extend_from_slice(&format.bits.to_le_bytes());
    header.extend_from_slice(b"data");
    header.extend_from_slice(&(u32::MAX - 36).to_le_bytes());
    header
}

fn token() -> String {
    format!("{:032x}", rand::random::<u128>())
}

/// What a device does with one stream.
pub(crate) struct Timing {
    /// How long the device last took from `play` to audible, in milliseconds.
    pub delay_ms: AtomicI64,
    /// Counts streams, so that a `play` for one that was replaced is not sent.
    pub generation: AtomicU64,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            delay_ms: AtomicI64::new(FIRST_DELAY_MS),
            generation: AtomicU64::new(0),
        }
    }
}

struct Live {
    token: String,
    audio: mpsc::Sender<Vec<u8>>,
}

pub struct LinkplaySink {
    shared: Arc<Shared>,
    format: OutputFormat,
    timeline: Timeline,
    live: Option<Live>,
    /// The last stream's token, withdrawn when the next begins or playback halts: a stream
    /// that ended may not have been pulled yet.
    last_token: Option<String>,
}

impl LinkplaySink {
    pub(crate) fn new(shared: Arc<Shared>, format: OutputFormat, timeline: Timeline) -> Self {
        Self {
            shared,
            format,
            timeline,
            live: None,
            last_token: None,
        }
    }

    /// Starts a stream whose first audio is timeline frame `at`.
    fn begin(&mut self, at: u64) -> io::Result<()> {
        if let Some(old) = self.last_token.take() {
            self.shared.hub.forget(&old);
        }
        let token = token();
        let (audio, pulled) = mpsc::channel(QUEUED_CHUNKS);
        self.shared.hub.register(token.clone(), pulled);
        let _ = audio.try_send(wav_header(self.format));
        let url = self
            .shared
            .stream_url(&token)
            .ok_or_else(|| io::Error::other("no address the device can reach the server at"))?;
        let generation = self.shared.timing.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let audible = self.timeline.instant_of(at);
        let shared = self.shared.clone();
        self.shared
            .runtime
            .spawn(async move { start(shared, url, audible, generation).await });
        self.live = Some(Live { token, audio });
        Ok(())
    }

    /// Ends the stream; the device plays what it was given of it.
    fn end(&mut self) {
        if let Some(live) = self.live.take() {
            self.last_token = Some(live.token);
        }
    }

    /// Ends the stream, cancels a `play` not yet sent, and stops the device.
    fn halt(&mut self) {
        let was_live = self.live.is_some();
        self.end();
        if let Some(token) = self.last_token.take() {
            self.shared.hub.forget(&token);
        }
        self.shared.timing.generation.fetch_add(1, Ordering::SeqCst);
        if was_live {
            let shared = self.shared.clone();
            self.shared.runtime.spawn(async move {
                if let Err(error) = shared.api.command("setPlayerCmd:stop").await {
                    tracing::debug!(device = shared.id, %error, "cannot stop");
                }
            });
        }
    }
}

/// Tells the device to play `url` so that it is audible at `audible`, then learns from its
/// countdown how long it really took.
async fn start(shared: Arc<Shared>, url: String, audible: Micros, generation: u64) {
    let delay_ms = shared.timing.delay_ms.load(Ordering::SeqCst);
    let at = audible - delay_ms * 1_000;
    let wait = at - clock::now();
    if wait > 0 {
        tokio::time::sleep(Duration::from_micros(wait as u64)).await;
    }
    if shared.timing.generation.load(Ordering::SeqCst) != generation {
        return;
    }
    if let Err(error) = shared
        .api
        .command(&format!("setPlayerCmd:play:{url}"))
        .await
    {
        tracing::warn!(device = shared.id, %error, "cannot tell the device to play");
        return;
    }
    let heard = if shared.has_player_status() {
        countdown(&shared, generation).await
    } else {
        first_tick(&shared, generation).await
    };
    let Some(heard) = heard else {
        tracing::debug!(device = shared.id, "no start seen; keeping the start delay");
        return;
    };
    let off_ms = (heard - audible) / 1_000;
    // Halfway to what this start says, so one odd start moves it only so far.
    let next = (delay_ms + off_ms / 2).clamp(500, 8_000);
    shared.timing.delay_ms.store(next, Ordering::SeqCst);
    tracing::debug!(
        device = shared.id,
        off_ms,
        delay_ms = next,
        "audible relative to the timeline"
    );
}

/// When the device became audible, from UPnP's whole-second position: the moment it first
/// turns from 0 to 1, less [`TICK_AT_MS`]. For devices whose firmware has no countdown.
async fn first_tick(shared: &Shared, generation: u64) -> Option<Micros> {
    let watched = Instant::now();
    // When the last reading below a second was asked for.
    let mut before: Option<Micros> = None;
    while watched.elapsed() < COUNTDOWN_WATCH {
        tokio::time::sleep(TICK_POLL).await;
        if shared.timing.generation.load(Ordering::SeqCst) != generation {
            return None;
        }
        let asked = clock::now();
        let Ok(Some(position)) = shared.upnp.position_ms().await else {
            continue;
        };
        let answered = clock::now();
        if position < 1_000 {
            before = Some(asked);
        } else if let Some(before) = before {
            // It turned between the two readings.
            return Some((before + answered) / 2 - TICK_AT_MS * 1_000);
        }
    }
    None
}

/// When the device will be audible, from the negative positions `getPlayerStatus` counts
/// down with.
async fn countdown(shared: &Shared, generation: u64) -> Option<Micros> {
    // Readings of when it will be audible, from each negative position.
    let mut readings = Vec::new();
    let watched = Instant::now();
    while watched.elapsed() < COUNTDOWN_WATCH && readings.len() < 3 {
        tokio::time::sleep(COUNTDOWN_POLL).await;
        if shared.timing.generation.load(Ordering::SeqCst) != generation {
            return None;
        }
        let asked = clock::now();
        let Ok(status) = shared.player_status().await else {
            continue;
        };
        let answered = clock::now();
        match (status.transport, status.position_ms) {
            (Some(Transport::Playing | Transport::Loading), Some(position)) if position < 0 => {
                readings.push((asked + answered) / 2 - position * 1_000);
            }
            // Audible already: the readings so far are what there is.
            (Some(Transport::Playing), Some(_)) if !readings.is_empty() => break,
            _ => {}
        }
    }
    if readings.is_empty() {
        return None;
    }
    readings.sort_unstable();
    Some(readings[readings.len() / 2])
}

impl Sink for LinkplaySink {
    fn format(&self) -> OutputFormat {
        self.format
    }

    fn write(&mut self, at: u64, data: &[u8]) -> io::Result<()> {
        if self.live.is_none() {
            self.begin(at)?;
        }
        let audio = &self.live.as_ref().expect("begun").audio;
        let mut chunk = data.to_vec();
        let deadline = Instant::now() + STALL_LIMIT;
        loop {
            match audio.try_send(chunk) {
                Ok(()) => return Ok(()),
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    self.live = None;
                    return Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "the device stopped pulling the stream",
                    ));
                }
                Err(mpsc::error::TrySendError::Full(back)) => {
                    if Instant::now() >= deadline {
                        self.halt();
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "the device stopped taking audio",
                        ));
                    }
                    chunk = back;
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        }
    }

    fn clear(&mut self) -> io::Result<()> {
        self.halt();
        Ok(())
    }

    fn stop(&mut self) -> io::Result<()> {
        self.halt();
        Ok(())
    }

    fn drain(&mut self) -> io::Result<()> {
        // The device plays to the end of what it was given, then stops by itself.
        self.end();
        Ok(())
    }

    fn finish(&mut self) -> io::Result<()> {
        self.end();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_endless_wav_header() {
        let header = wav_header(OutputFormat {
            sample_rate: 44_100,
            channels: 2,
            bits: 16,
        });
        assert_eq!(header.len(), 44);
        assert_eq!(&header[0..4], b"RIFF");
        assert_eq!(
            u32::from_le_bytes(header[24..28].try_into().unwrap()),
            44_100
        );
        assert_eq!(
            u32::from_le_bytes(header[28..32].try_into().unwrap()),
            176_400
        );
        assert_eq!(u16::from_le_bytes(header[32..34].try_into().unwrap()), 4);
        assert_eq!(&header[36..40], b"data");
    }
}
