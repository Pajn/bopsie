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
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use tokio::sync::mpsc as async_mpsc;

use crate::convert::Converter;
use crate::decode::{self, Decoder, DecoderKind, FfmpegDecoder, SymphoniaDecoder};
use crate::format::{OutputFormat, PcmFormat};
use crate::pack::pack;
use crate::ring::Ring;
use crate::sink::Sink;
use crate::timeline::{Micros, Timeline};

/// How far beyond its lead a real-time session may run ahead of real time.
const MAX_AHEAD: Micros = 10_000_000;

/// A track to play, and where in it to start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackRequest {
    /// Identifies the track in events, such as a queue item id.
    pub id: String,
    pub path: PathBuf,
    pub start: Duration,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tagged {
    pub epoch: u64,
    pub event: Event,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The track, from `start` within it, begins at timeline frame `at`.
    TrackStarted {
        id: String,
        at: u64,
        start: Duration,
    },
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
}

pub struct EngineHandle {
    commands: mpsc::Sender<Command>,
    pub events: async_mpsc::UnboundedReceiver<Tagged>,
    thread: Option<JoinHandle<()>>,
}

impl EngineHandle {
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
    let thread = std::thread::Builder::new()
        .name("stream-engine".to_owned())
        .spawn(move || Engine::new(settings, source, sinks, events).run(inbox))
        .expect("spawning the engine thread");
    EngineHandle {
        commands,
        events: outbox,
        thread: Some(thread),
    }
}

/// Opens a decoder for `path` that produces audio the session can take.
pub fn open(
    path: &Path,
    format: PcmFormat,
    ffmpeg: Option<&Path>,
) -> Result<Box<dyn Decoder>, decode::DecodeError> {
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
}

struct Playing {
    id: String,
    decoder: Box<dyn Decoder>,
    converter: Converter,
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
}

impl Engine {
    fn new(
        settings: Settings,
        source: Box<dyn TrackSource>,
        sinks: Vec<(String, Box<dyn Sink>)>,
        events: async_mpsc::UnboundedSender<Tagged>,
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

    fn add_sink(&mut self, key: String, mut sink: Box<dyn Sink>) {
        assert_eq!(
            sink.format().sample_rate,
            self.settings.format.sample_rate,
            "sinks take the session's rate"
        );
        // While playing, the newcomer gets what is still to play of the recent audio.
        if self.current.is_some()
            && let Some(realtime) = self.settings.realtime
        {
            let from = realtime
                .timeline
                .frame_at((realtime.clock)() + realtime.lead);
            if let Some(samples) = self.ring.since(from) {
                let mut packed = Vec::new();
                pack(&samples, sink.format().bits, &mut packed);
                if let Err(error) = sink.write(from, &packed) {
                    tracing::warn!(sink = key, %error, "a joining output failed to take audio");
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
        }
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
                &request.path,
                self.settings.format,
                self.settings.ffmpeg.as_deref(),
            ) {
                Ok(decoder) => {
                    return Some(Opened {
                        id: request.id,
                        decoder,
                        start: request.start,
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
        self.emit(Event::TrackStarted {
            id: opened.id.clone(),
            at: self.head,
            start: opened.start,
        });
        self.current = Some(Playing {
            id: opened.id,
            converter: Converter::new(native, self.settings.format),
            decoder: opened.decoder,
        });
        self.prepared = self.open_next();
    }

    fn play_chunk(&mut self) {
        let Some(playing) = &mut self.current else {
            return;
        };
        let mut decoded = Vec::new();
        let (samples, ended) = match playing.decoder.read(&mut decoded) {
            Ok(true) => (playing.converter.push(&decoded), false),
            Ok(false) => (playing.converter.finish(), true),
            Err(error) => {
                tracing::warn!(id = playing.id, %error, "decoding stopped early");
                (playing.converter.finish(), true)
            }
        };
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
                tracing::warn!(sink = key, %error, "an output failed; dropping it");
                failed.push(key.clone());
            }
        }
        for key in failed {
            self.sinks.remove(&key);
        }
        self.head += (samples.len() / usize::from(self.settings.format.channels)) as u64;
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
                .frame_at((realtime.clock)() + realtime.lead + MAX_AHEAD);
            if self.head <= limit {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
