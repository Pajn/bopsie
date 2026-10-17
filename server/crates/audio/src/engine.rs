//! The stream engine: one thread per session that decodes tracks one after another onto the
//! session's timeline and hands the audio to every sink.
//!
//! Tracks follow each other with no gap: the next track's first frame is written right after
//! the last frame of the one before, and the next track is opened as soon as the current one
//! starts, so it is ready before it is due. Audio is packed once per distinct output format
//! and shared by the sinks that take it.
//!
//! A real-time session is anchored to the server's clock: whenever playback starts afresh —
//! the first track, after a jump, a seek or a stop — the write head moves to one lead ahead of
//! now, so every sink has the lead to get the audio to its speaker. A sink that joins while the
//! session plays is given the recent audio that is still to play.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;

use tokio::sync::mpsc as async_mpsc;

use crate::convert::Converter;
use crate::decode::{
    self, DecodeError, Decoder, DecoderKind, FfmpegDecoder, Read, SymphoniaDecoder,
};
use crate::format::{OutputFormat, PcmFormat};
use crate::loudness::{Loudness, Mode, Normalise};
use crate::pack::pack;
use crate::ring::Ring;
use crate::sink::Sink;
use crate::timeline::{Micros, Timeline};

/// How far beyond its lead a queue's session may run ahead of real time.
pub const QUEUE_AHEAD: Micros = 10_000_000;
/// How far beyond its lead a live input is read: little, so that what a pause throws away is
/// little more than the lead.
pub const LIVE_AHEAD: Micros = 300_000;

/// A track to play, and where in it to start.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackRequest {
    /// Identifies the track in events, such as a queue item id.
    pub id: String,
    pub input: Input,
    pub start: Duration,
    /// How loud the track is, when known before it is opened. A decoder that knows says so
    /// itself otherwise.
    pub loudness: Option<Loudness>,
    /// How to normalise it; `None` plays it as it is.
    pub normalise: Option<Normalise>,
}

/// The gain a track plays with, from normalisation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Applied {
    pub gain_db: f32,
    pub mode: Mode,
    /// The loudness it was taken from.
    pub loudness: Loudness,
    pub target_lufs: f32,
}

/// Where a track's audio comes from.
#[derive(Clone)]
pub enum Input {
    /// A local file, decoded by the decoder its extension calls for.
    File(PathBuf),
    /// Anything else that opens as a [`Decoder`]: a stream, or live audio.
    Open(Arc<dyn Media>),
}

impl Input {
    /// A decoder that is already open, such as a [`crate::live::LiveInput`]. It plays once.
    pub fn decoder(decoder: Box<dyn Decoder>) -> Self {
        Self::Open(Arc::new(Once(Mutex::new(Some(decoder)))))
    }
}

/// What a track plays from, and how loud it is there, when that is known before it opens.
#[derive(Debug, Clone, PartialEq)]
pub struct Playable {
    pub input: Input,
    pub loudness: Option<Loudness>,
    /// Where it starts when played from its beginning: where listening left off, for an
    /// audiobook.
    pub start: Duration,
}

impl From<PathBuf> for Playable {
    fn from(path: PathBuf) -> Self {
        Input::File(path).into()
    }
}

impl From<Input> for Playable {
    fn from(input: Input) -> Self {
        Self {
            input,
            loudness: None,
            start: Duration::ZERO,
        }
    }
}

impl From<PathBuf> for Input {
    fn from(path: PathBuf) -> Self {
        Self::File(path)
    }
}

impl std::fmt::Debug for Input {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::File(path) => f.debug_tuple("File").field(path).finish(),
            Self::Open(_) => f.write_str("Open(..)"),
        }
    }
}

impl PartialEq for Input {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::File(a), Self::File(b)) => a == b,
            (Self::Open(a), Self::Open(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl Eq for Input {}

/// Audio that is not a local file. Opened on the engine's thread, so it must not block for
/// long.
pub trait Media: Send + Sync {
    /// A decoder from the start. Its format need not be the session's, which it is converted
    /// to.
    fn open(&self, format: PcmFormat) -> Result<Box<dyn Decoder>, DecodeError>;
}

struct Once(Mutex<Option<Box<dyn Decoder>>>);

impl Media for Once {
    fn open(&self, _format: PcmFormat) -> Result<Box<dyn Decoder>, DecodeError> {
        self.0
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| DecodeError::Unsupported("a live input plays once".to_owned()))
    }
}

/// Supplies tracks in order. Called on the engine's thread, so it must not block for long.
pub trait TrackSource: Send + 'static {
    /// The track after the last one returned, or `None` when there is none yet.
    fn next(&mut self) -> Option<TrackRequest>;
}

pub enum Command {
    /// Moves within the current track.
    Seek(Duration),
    /// Abandons the current and prepared tracks and asks the source again, as after the
    /// listener picks another track. Events from then on carry `epoch`. Playback starts no
    /// earlier than `not_before`, when the outputs need longer than the lead to be ready.
    Restart {
        epoch: u64,
        not_before: Option<Micros>,
    },
    /// Forgets the prepared next track, because what follows the current one changed.
    NextChanged,
    /// Stops playing. A later `Restart` resumes from the source. Events from then on carry
    /// `epoch`.
    Stop { epoch: u64 },
    /// Adds a sink under `key`, replacing any with the same key.
    AddSink { key: String, sink: Box<dyn Sink> },
    /// Stops and removes the sink under `key`.
    RemoveSink { key: String },
    /// Ends the session.
    Shutdown,
}

/// An event, with the epoch of the `Restart` or `Stop` it follows, so that a listener can
/// tell events about what it asked for from ones already on their way before it asked.
#[derive(Debug, Clone, PartialEq)]
pub struct Tagged {
    pub epoch: u64,
    pub event: Event,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The track, from `start` within it, begins at timeline frame `at`, with the gain
    /// normalisation gave it, if any and known yet.
    TrackStarted {
        id: String,
        at: u64,
        start: Duration,
        normalised: Option<Applied>,
    },
    /// Normalisation gave the track this gain, known only once its audio came: a streamed
    /// track's loudness arrives with it.
    Normalised { id: String, applied: Applied },
    /// The track's last frame ends at timeline frame `at`.
    TrackEnded { id: String, at: u64 },
    /// The track could not be played and was skipped.
    TrackFailed { id: String, error: String },
    /// Playback continues at `position` within the track, from timeline frame `at`.
    Seeked {
        id: String,
        position: Duration,
        at: u64,
    },
    /// Nothing is left to play; what was written ends at timeline frame `at`.
    Idle { at: u64 },
    /// An output failed and was dropped from the session. Not tied to an epoch.
    OutputFailed { key: String, error: String },
    /// The session's thread panicked: the session is over. Not tied to an epoch.
    Crashed { error: String },
}

/// What anchors a session to real time.
#[derive(Debug, Clone, Copy)]
pub struct Realtime {
    pub timeline: Timeline,
    /// How far ahead of now playback starts.
    pub lead: Micros,
    /// The server's clock.
    pub clock: fn() -> Micros,
}

pub struct Settings {
    /// The session's format: every track is converted to it.
    pub format: PcmFormat,
    /// Used for files Symphonia cannot play gaplessly, when installed.
    pub ffmpeg: Option<PathBuf>,
    /// How much recent audio to keep for sinks that join late.
    pub ring_frames: usize,
    /// `None` runs as fast as the sinks take audio, starting at frame 0, as for files.
    pub realtime: Option<Realtime>,
    /// How far beyond its lead a real-time session may run ahead of real time:
    /// [`QUEUE_AHEAD`], or [`LIVE_AHEAD`] for live input.
    pub max_ahead: Micros,
}

pub struct EngineHandle {
    commands: mpsc::Sender<Command>,
    pub events: async_mpsc::UnboundedReceiver<Tagged>,
    thread: Option<JoinHandle<()>>,
    head: Arc<AtomicU64>,
}

impl EngineHandle {
    /// The timeline frame the next audio will be written at: how far ahead of what is heard
    /// the session has written.
    pub fn head(&self) -> u64 {
        self.head.load(Ordering::SeqCst)
    }

    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    /// A handle for sending commands from elsewhere.
    pub fn commander(&self) -> mpsc::Sender<Command> {
        self.commands.clone()
    }

    /// Ends the session and waits for the thread, which finishes every sink.
    pub fn shutdown(mut self) {
        self.send(Command::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for EngineHandle {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
    }
}

/// Starts a session on its own thread. It waits for a `Restart` before it asks the source for
/// anything.
pub fn spawn(
    settings: Settings,
    source: Box<dyn TrackSource>,
    sinks: Vec<(String, Box<dyn Sink>)>,
) -> EngineHandle {
    let (commands, inbox) = mpsc::channel();
    let (events, outbox) = async_mpsc::unbounded_channel();
    let head = Arc::new(AtomicU64::new(0));
    let written = head.clone();
    let thread = std::thread::Builder::new()
        .name("stream-engine".to_owned())
        .spawn(move || {
            let crashed = events.clone();
            let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                Engine::new(settings, source, sinks, events, written).run(inbox)
            }));
            if let Err(panic) = ran {
                let error = panic_message(panic.as_ref());
                tracing::error!(error, "the stream engine crashed");
                let _ = crashed.send(Tagged {
                    epoch: 0,
                    event: Event::Crashed { error },
                });
            }
        })
        .expect("spawning the engine thread");
    EngineHandle {
        commands,
        events: outbox,
        thread: Some(thread),
        head,
    }
}

/// What a panic said.
fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    panic
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic".to_owned())
}

/// Opens a decoder for `input` that produces audio the session can take.
pub fn open(
    input: &Input,
    format: PcmFormat,
    ffmpeg: Option<&Path>,
) -> Result<Box<dyn Decoder>, decode::DecodeError> {
    let path = match input {
        Input::File(path) => path,
        Input::Open(media) => return media.open(format),
    };
    match decode::choose(path, ffmpeg.is_some()) {
        Some(DecoderKind::Ffmpeg) => Ok(Box::new(FfmpegDecoder::open(
            ffmpeg.expect("chosen only when available"),
            path,
            format,
        )?)),
        Some(DecoderKind::Symphonia) => Ok(Box::new(SymphoniaDecoder::open(path)?)),
        None => Err(decode::DecodeError::Unsupported(format!(
            "no decoder for {}",
            path.display()
        ))),
    }
}

struct Opened {
    id: String,
    decoder: Box<dyn Decoder>,
    start: Duration,
    loudness: Option<Loudness>,
    normalise: Option<Normalise>,
}

struct Playing {
    id: String,
    decoder: Box<dyn Decoder>,
    converter: Converter,
    /// What its samples are multiplied by; `None` until the decoder says how loud they are,
    /// which it does by its first audio.
    factor: Option<f32>,
    normalise: Option<Normalise>,
}

/// The gain `normalise` gives audio as loud as `loudness`.
fn applied(loudness: Loudness, normalise: Normalise) -> Applied {
    let (gain_db, mode) = loudness.gain_db(normalise);
    Applied {
        gain_db,
        mode,
        loudness,
        target_lufs: normalise.target_lufs,
    }
}

fn factor(applied: Option<&Applied>) -> f32 {
    applied.map_or(1.0, |applied| 10f32.powf(applied.gain_db / 20.0))
}

struct Engine {
    settings: Settings,
    source: Box<dyn TrackSource>,
    sinks: BTreeMap<String, Box<dyn Sink>>,
    events: async_mpsc::UnboundedSender<Tagged>,
    epoch: u64,
    ring: Ring,
    /// The timeline frame the next audio is written at.
    head: u64,
    current: Option<Playing>,
    prepared: Option<Opened>,
    idle: bool,
    /// The next track follows on from the last without a break.
    continuous: bool,
    /// The earliest playback may start after a restart.
    not_before: Option<Micros>,
    /// Shared with the handle: `head`, as last written.
    written: Arc<AtomicU64>,
    /// The current track had nothing to give at the last read.
    starved: bool,
}

impl Engine {
    fn new(
        settings: Settings,
        source: Box<dyn TrackSource>,
        sinks: Vec<(String, Box<dyn Sink>)>,
        events: async_mpsc::UnboundedSender<Tagged>,
        written: Arc<AtomicU64>,
    ) -> Self {
        let ring = Ring::new(settings.format.channels, settings.ring_frames);
        let mut engine = Self {
            settings,
            source,
            sinks: BTreeMap::new(),
            events,
            epoch: 0,
            ring,
            head: 0,
            current: None,
            prepared: None,
            idle: true,
            continuous: false,
            not_before: None,
            written,
            starved: false,
        };
        for (key, sink) in sinks {
            engine.add_sink(key, sink);
        }
        engine
    }

    fn emit(&self, event: Event) {
        let _ = self.events.send(Tagged {
            epoch: self.epoch,
            event,
        });
    }

    fn run(mut self, inbox: mpsc::Receiver<Command>) {
        loop {
            // Commands wait when there is nothing to play, and are polled between chunks
            // otherwise.
            let command = if self.current.is_none() && self.idle {
                match inbox.recv() {
                    Ok(command) => Some(command),
                    Err(_) => break,
                }
            } else {
                match inbox.try_recv() {
                    Ok(command) => Some(command),
                    Err(mpsc::TryRecvError::Empty) => None,
                    Err(mpsc::TryRecvError::Disconnected) => break,
                }
            };
            if let Some(command) = command {
                if !self.handle(command) {
                    break;
                }
                continue;
            }
            if self.current.is_none() {
                self.start_next();
                continue;
            }
            self.play_chunk();
        }
        for (key, sink) in &mut self.sinks {
            if let Err(error) = sink.finish() {
                tracing::warn!(sink = key, %error, "an output failed to finish");
            }
        }
    }

    /// Returns `false` when the session ends.
    fn handle(&mut self, command: Command) -> bool {
        match command {
            Command::Seek(position) => self.seek(position),
            Command::Restart { epoch, not_before } => {
                self.epoch = epoch;
                self.not_before = not_before;
                self.current = None;
                self.prepared = None;
                self.idle = false;
                self.continuous = false;
                self.each_sink("clear", |sink| sink.clear());
                self.ring.clear();
                self.rewind();
            }
            Command::NextChanged => {
                self.prepared = None;
                self.idle = false;
            }
            Command::Stop { epoch } => {
                self.epoch = epoch;
                self.current = None;
                self.prepared = None;
                self.idle = true;
                self.continuous = false;
                self.each_sink("stop", |sink| sink.stop());
                self.ring.clear();
                self.emit(Event::Idle { at: self.head });
            }
            Command::AddSink { key, sink } => self.add_sink(key, sink),
            Command::RemoveSink { key } => {
                if let Some(mut sink) = self.sinks.remove(&key) {
                    let _ = sink.stop();
                    let _ = sink.finish();
                }
            }
            Command::Shutdown => return false,
        }
        true
    }

    /// Reports an output that failed, which is no longer part of the session.
    fn output_failed(&self, key: &str, error: impl std::fmt::Display) {
        tracing::warn!(sink = key, %error, "an output failed; dropping it");
        self.emit(Event::OutputFailed {
            key: key.to_owned(),
            error: error.to_string(),
        });
    }

    fn add_sink(&mut self, key: String, mut sink: Box<dyn Sink>) {
        if sink.format().sample_rate != self.settings.format.sample_rate {
            let error = format!(
                "it takes {} Hz and the session plays {} Hz",
                sink.format().sample_rate,
                self.settings.format.sample_rate
            );
            self.output_failed(&key, error);
            return;
        }
        // The newcomer gets what is still to play of the recent audio, which may be all there
        // is when the rest was written ahead; a stop empties it.
        if let Some(realtime) = self.settings.realtime {
            let from = realtime
                .timeline
                .frame_at((realtime.clock)() + realtime.lead);
            if let Some(samples) = self.ring.since(from) {
                let mut packed = Vec::new();
                pack(&samples, sink.format().bits, &mut packed);
                if let Err(error) = sink.write(from, &packed) {
                    self.output_failed(&key, error);
                    return;
                }
                // With nothing more to come, it is told so, as the others were.
                if self.idle
                    && let Err(error) = sink.drain()
                {
                    self.output_failed(&key, error);
                    return;
                }
            }
        }
        if let Some(mut old) = self.sinks.insert(key, sink) {
            let _ = old.stop();
            let _ = old.finish();
        }
    }

    fn each_sink(&mut self, what: &str, f: impl Fn(&mut dyn Sink) -> std::io::Result<()>) {
        for (key, sink) in &mut self.sinks {
            if let Err(error) = f(sink.as_mut()) {
                tracing::warn!(sink = key, %error, "an output failed to {what}");
            }
        }
    }

    /// Moves the write head to one lead from now, or to when the outputs are ready if later,
    /// if it is behind that.
    fn anchor(&mut self) {
        if let Some(realtime) = self.settings.realtime {
            let earliest =
                ((realtime.clock)() + realtime.lead).max(self.not_before.take().unwrap_or(0));
            let due = realtime.timeline.frame_at(earliest);
            self.head = self.head.max(due);
            self.written.store(self.head, Ordering::SeqCst);
        }
    }

    /// After the outputs were cleared, what was written ahead is gone: the next audio goes one
    /// lead from now, not after it.
    fn rewind(&mut self) {
        if let Some(realtime) = self.settings.realtime {
            let due = realtime
                .timeline
                .frame_at((realtime.clock)() + realtime.lead);
            self.head = self.head.min(due);
            self.written.store(self.head, Ordering::SeqCst);
        }
    }

    /// After a live input kept the session waiting, the next audio may come too late for the
    /// outputs to play it when due. Within half the lead it is still in time; beyond that the
    /// outputs start afresh one lead from now, leaving a gap rather than playing late.
    fn catch_up(&mut self) {
        let Some(realtime) = self.settings.realtime else {
            return;
        };
        let now = (realtime.clock)();
        let late = realtime.timeline.frame_at(now + realtime.lead / 2);
        if self.head >= late {
            return;
        }
        let due = realtime.timeline.frame_at(now + realtime.lead);
        tracing::debug!(
            behind = due - self.head,
            "the input fell behind; starting afresh"
        );
        self.each_sink("clear", |sink| sink.clear());
        self.ring.clear();
        self.anchor();
    }

    fn seek(&mut self, position: Duration) {
        let Some(playing) = &mut self.current else {
            return;
        };
        let native = playing.decoder.format();
        let frame = (position.as_secs_f64() * f64::from(native.sample_rate)).round() as u64;
        match playing.decoder.seek(frame) {
            Ok(_) => {
                playing.converter = Converter::new(native, self.settings.format);
                let id = playing.id.clone();
                self.each_sink("clear", |sink| sink.clear());
                self.ring.clear();
                self.rewind();
                self.anchor();
                self.emit(Event::Seeked {
                    id,
                    position,
                    at: self.head,
                });
            }
            Err(error) => tracing::warn!(%error, "seek failed"),
        }
    }

    /// Opens the next track the source has, reporting and skipping ones that fail.
    fn open_next(&mut self) -> Option<Opened> {
        loop {
            let request = self.source.next()?;
            match open(
                &request.input,
                self.settings.format,
                self.settings.ffmpeg.as_deref(),
            ) {
                Ok(decoder) => {
                    return Some(Opened {
                        id: request.id,
                        decoder,
                        start: request.start,
                        loudness: request.loudness,
                        normalise: request.normalise,
                    });
                }
                Err(error) => self.emit(Event::TrackFailed {
                    id: request.id,
                    error: error.to_string(),
                }),
            }
        }
    }

    fn start_next(&mut self) {
        let opened = match self.prepared.take() {
            Some(opened) => Some(opened),
            None => self.open_next(),
        };
        let Some(mut opened) = opened else {
            if !self.idle {
                self.idle = true;
                self.continuous = false;
                self.each_sink("drain", |sink| sink.drain());
                self.emit(Event::Idle { at: self.head });
            }
            return;
        };
        self.idle = false;
        if !self.continuous {
            self.anchor();
        }
        self.continuous = true;
        let native = opened.decoder.format();
        if !opened.start.is_zero() {
            let frame = (opened.start.as_secs_f64() * f64::from(native.sample_rate)).round() as u64;
            if let Err(error) = opened.decoder.seek(frame) {
                tracing::warn!(%error, "cannot start mid-track; starting at the beginning");
                opened.start = Duration::ZERO;
            }
        }
        // The gain is settled now when the loudness is known, else at the track's first audio.
        let loudness = opened.loudness.or_else(|| opened.decoder.loudness());
        let normalised = opened
            .normalise
            .zip(loudness)
            .map(|(normalise, loudness)| applied(loudness, normalise));
        let settled = opened.normalise.is_none() || loudness.is_some();
        self.emit(Event::TrackStarted {
            id: opened.id.clone(),
            at: self.head,
            start: opened.start,
            normalised,
        });
        self.current = Some(Playing {
            id: opened.id,
            converter: Converter::new(native, self.settings.format),
            decoder: opened.decoder,
            factor: settled.then(|| factor(normalised.as_ref())),
            normalise: opened.normalise,
        });
        self.prepared = self.open_next();
    }

    fn play_chunk(&mut self) {
        let Some(playing) = &mut self.current else {
            return;
        };
        let mut decoded = Vec::new();
        let (samples, ended) = match playing.decoder.read(&mut decoded) {
            Ok(Read::Data) => {
                let factor = match playing.factor {
                    Some(factor) => factor,
                    None => {
                        let normalised = playing
                            .normalise
                            .zip(playing.decoder.loudness())
                            .map(|(normalise, loudness)| applied(loudness, normalise));
                        let factor = factor(normalised.as_ref());
                        playing.factor = Some(factor);
                        if let Some(applied) = normalised {
                            let id = playing.id.clone();
                            self.emit(Event::Normalised { id, applied });
                        }
                        factor
                    }
                };
                let Some(playing) = &mut self.current else {
                    return;
                };
                if factor != 1.0 {
                    for sample in &mut decoded {
                        *sample *= factor;
                    }
                }
                (playing.converter.push(&decoded), false)
            }
            Ok(Read::Pending) => {
                self.starved = true;
                return;
            }
            Ok(Read::End) => (playing.converter.finish(), true),
            Err(error) => {
                tracing::warn!(id = playing.id, %error, "decoding stopped early");
                (playing.converter.finish(), true)
            }
        };
        if std::mem::take(&mut self.starved) {
            self.catch_up();
        }
        self.write(&samples);
        if ended {
            let id = self.current.take().map(|p| p.id).unwrap_or_default();
            self.emit(Event::TrackEnded { id, at: self.head });
        }
    }

    fn write(&mut self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }
        self.keep_pace();
        let at = self.head;
        self.ring.push(at, samples);
        let mut by_format: HashMap<OutputFormat, Vec<u8>> = HashMap::new();
        let mut failed = Vec::new();
        for (key, sink) in &mut self.sinks {
            let format = sink.format();
            let packed = by_format.entry(format).or_insert_with(|| {
                let mut packed = Vec::new();
                pack(samples, format.bits, &mut packed);
                packed
            });
            if let Err(error) = sink.write(at, packed) {
                failed.push((key.clone(), error));
            }
        }
        for (key, error) in failed {
            self.sinks.remove(&key);
            self.output_failed(&key, error);
        }
        self.head += (samples.len() / usize::from(self.settings.format.channels)) as u64;
        self.written.store(self.head, Ordering::SeqCst);
    }

    /// Sinks pace a real-time session by taking audio no faster than they can hold it. Without
    /// one that does, the session waits rather than run more than a little past its lead.
    fn keep_pace(&self) {
        let Some(realtime) = self.settings.realtime else {
            return;
        };
        loop {
            let limit = realtime
                .timeline
                .frame_at((realtime.clock)() + realtime.lead + self.settings.max_ahead);
            if self.head <= limit {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
