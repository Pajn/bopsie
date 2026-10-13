//! The stream engine: one thread per session that decodes tracks one after another onto the
//! session's timeline and hands the audio to every sink.
//!
//! Tracks follow each other with no gap: the next track's first frame is written right after
//! the last frame of the one before, and the next track is opened as soon as the current one
//! starts, so it is ready before it is due. Audio is packed once per distinct output format
//! and shared by the sinks that take it.

use std::collections::HashMap;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Moves within the current track.
    Seek(Duration),
    /// Abandons the current and prepared tracks and asks the source again, as after the
    /// listener picks another track.
    Restart,
    /// Forgets the prepared next track, because what follows the current one changed.
    NextChanged,
    /// Stops playing. A later `Restart` resumes from the source.
    Stop,
    /// Ends the session.
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The track's first frame is at timeline frame `at`.
    TrackStarted { id: String, at: u64 },
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
    /// Nothing is left to play.
    Idle { at: u64 },
}

pub struct Settings {
    /// The session's format: every track is converted to it.
    pub format: PcmFormat,
    /// Used for files Symphonia cannot play gaplessly, when installed.
    pub ffmpeg: Option<PathBuf>,
    /// How much recent audio to keep for outputs that join late.
    pub ring_frames: usize,
}

pub struct EngineHandle {
    commands: mpsc::Sender<Command>,
    pub events: async_mpsc::UnboundedReceiver<Event>,
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

/// Starts a session on its own thread.
pub fn spawn(
    settings: Settings,
    source: Box<dyn TrackSource>,
    sinks: Vec<Box<dyn Sink>>,
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

struct Group {
    format: OutputFormat,
    sinks: Vec<usize>,
}

struct Engine {
    settings: Settings,
    source: Box<dyn TrackSource>,
    sinks: Vec<Box<dyn Sink>>,
    groups: Vec<Group>,
    events: async_mpsc::UnboundedSender<Event>,
    ring: Ring,
    /// The timeline frame the next audio is written at.
    head: u64,
    current: Option<Playing>,
    prepared: Option<Opened>,
    idle: bool,
}

impl Engine {
    fn new(
        settings: Settings,
        source: Box<dyn TrackSource>,
        sinks: Vec<Box<dyn Sink>>,
        events: async_mpsc::UnboundedSender<Event>,
    ) -> Self {
        let mut by_format: HashMap<OutputFormat, Vec<usize>> = HashMap::new();
        for (index, sink) in sinks.iter().enumerate() {
            let format = sink.format();
            assert_eq!(
                format.sample_rate, settings.format.sample_rate,
                "sinks take the session's rate"
            );
            by_format.entry(format).or_default().push(index);
        }
        let groups = by_format
            .into_iter()
            .map(|(format, sinks)| Group { format, sinks })
            .collect();
        let ring = Ring::new(settings.format.channels, settings.ring_frames);
        Self {
            settings,
            source,
            sinks,
            groups,
            events,
            ring,
            head: 0,
            current: None,
            prepared: None,
            idle: false,
        }
    }

    fn emit(&self, event: Event) {
        let _ = self.events.send(event);
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
        for sink in &mut self.sinks {
            if let Err(error) = sink.finish() {
                tracing::warn!(%error, "an output failed to finish");
            }
        }
    }

    /// Returns `false` when the session ends.
    fn handle(&mut self, command: Command) -> bool {
        match command {
            Command::Seek(position) => self.seek(position),
            Command::Restart => {
                self.current = None;
                self.prepared = None;
                self.idle = false;
                self.flush();
            }
            Command::NextChanged => {
                self.prepared = None;
                self.idle = false;
            }
            Command::Stop => {
                self.current = None;
                self.prepared = None;
                self.idle = true;
                self.flush();
                self.emit(Event::Idle { at: self.head });
            }
            Command::Shutdown => return false,
        }
        true
    }

    fn flush(&mut self) {
        self.ring.clear();
        for sink in &mut self.sinks {
            if let Err(error) = sink.flush() {
                tracing::warn!(%error, "an output failed to flush");
            }
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
                self.flush();
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
                self.emit(Event::Idle { at: self.head });
            }
            return;
        };
        self.idle = false;
        let native = opened.decoder.format();
        if !opened.start.is_zero() {
            let frame = (opened.start.as_secs_f64() * f64::from(native.sample_rate)).round() as u64;
            if let Err(error) = opened.decoder.seek(frame) {
                tracing::warn!(%error, "cannot start mid-track; starting at the beginning");
            }
        }
        self.emit(Event::TrackStarted {
            id: opened.id.clone(),
            at: self.head,
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
        let at = self.head;
        self.ring.push(at, samples);
        let mut packed = Vec::new();
        for group in &self.groups {
            packed.clear();
            pack(samples, group.format.bits, &mut packed);
            for &index in &group.sinks {
                if let Err(error) = self.sinks[index].write(at, &packed) {
                    tracing::warn!(%error, "an output failed to take audio");
                }
            }
        }
        self.head += (samples.len() / usize::from(self.settings.format.channels)) as u64;
    }
}
