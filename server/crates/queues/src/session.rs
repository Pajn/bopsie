//! A queue wired to a stream engine. The engine asks the queue for tracks in order; the queue
//! learns from the engine's events which item is playing, how far in, and when it has run
//! out.
//!
//! The engine writes ahead of what can be heard. Events about real-time sessions take effect
//! when what they describe becomes audible, so the current item changes when the listener hears
//! the new track, not when the engine starts writing it.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use bopsie_audio::PcmFormat;
use bopsie_audio::engine::{
    self, Command, EngineHandle, Event, Settings, Tagged, TrackRequest, TrackSource,
};
use bopsie_audio::sink::Sink;
use bopsie_audio::timeline::{Micros, Timeline};
use bopsie_core::clock;
use tokio::sync::{broadcast, mpsc};

use crate::state::{Effect, PlaybackState, Queue};

/// What changed in a session, for whoever reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// The item at this index became audible.
    Started(usize),
    /// Playback moved within the current item.
    Seeked,
    /// A track could not be played and was skipped.
    Failed {
        queue_item_id: String,
        error: String,
    },
    /// The queue played to its end.
    Finished,
    /// Playback was stopped or paused.
    Stopped,
    /// The output with this key failed and left the session.
    OutputFailed(String),
    /// The engine crashed: the session has none until one is attached again.
    Crashed(String),
}

/// Where playback is in the current item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Position {
    /// How far into the item playback was at `since`.
    pub offset: Duration,
    /// When `offset` was audible, while playback advances.
    pub since: Option<Micros>,
}

impl Position {
    pub fn at(&self, now: Micros) -> Duration {
        match self.since {
            Some(since) => self.offset + Duration::from_micros((now - since).max(0) as u64),
            None => self.offset,
        }
    }
}

/// Hands the queue's items to the engine, while the queue is meant to be playing.
struct Source {
    queue: Arc<Mutex<Queue>>,
}

impl TrackSource for Source {
    fn next(&mut self) -> Option<TrackRequest> {
        let mut queue = self.queue.lock().unwrap();
        if queue.state != PlaybackState::Playing {
            return None;
        }
        let (item, start) = queue.hand_over_next()?;
        Some(TrackRequest {
            id: item.queue_item_id.clone(),
            input: item.path.clone().into(),
            start,
        })
    }
}

struct Running {
    /// Tells this engine from the one that replaced it.
    id: u64,
    handle: EngineHandle,
    format: PcmFormat,
    timeline: Option<Timeline>,
}

#[derive(Clone)]
pub struct QueueSession {
    queue: Arc<Mutex<Queue>>,
    running: Arc<Mutex<Option<Running>>>,
    /// The epoch of the last `Restart` or `Stop`; older events are ignored.
    epoch: Arc<Mutex<u64>>,
    position: Arc<Mutex<Position>>,
    notices: broadcast::Sender<Notice>,
}

impl Default for QueueSession {
    fn default() -> Self {
        Self::new(Queue::default())
    }
}

impl QueueSession {
    /// A session for `queue`, resting where it says to resume, with no engine until one is
    /// attached.
    pub fn new(queue: Queue) -> Self {
        let position = Position {
            offset: queue.resume_position,
            since: None,
        };
        Self {
            queue: Arc::new(Mutex::new(queue)),
            running: Arc::default(),
            epoch: Arc::default(),
            position: Arc::new(Mutex::new(position)),
            notices: broadcast::channel(256).0,
        }
    }

    /// A session with an engine already attached. Needs a Tokio runtime.
    pub fn start(queue: Queue, settings: Settings, sinks: Vec<(String, Box<dyn Sink>)>) -> Self {
        let session = Self::new(queue);
        session.attach(settings, sinks);
        session
    }

    /// Starts an engine for the session, replacing any it had. Needs a Tokio runtime.
    pub fn attach(&self, settings: Settings, sinks: Vec<(String, Box<dyn Sink>)>) {
        let format = settings.format;
        let timeline = settings.realtime.map(|realtime| realtime.timeline);
        let mut handle = engine::spawn(
            settings,
            Box::new(Source {
                queue: self.queue.clone(),
            }),
            sinks,
        );
        let events = std::mem::replace(&mut handle.events, mpsc::unbounded_channel().1);
        static ENGINES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = ENGINES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        tokio::spawn(follow(events, self.clone(), timeline, id));
        if let Some(old) = self.running.lock().unwrap().replace(Running {
            id,
            handle,
            format,
            timeline,
        }) {
            old.handle.send(Command::Shutdown);
        }
    }

    /// The running engine's format and timeline, if one is attached.
    pub fn engine(&self) -> Option<(PcmFormat, Option<Timeline>)> {
        self.running
            .lock()
            .unwrap()
            .as_ref()
            .map(|r| (r.format, r.timeline))
    }

    fn send(&self, command: Command) {
        if let Some(running) = self.running.lock().unwrap().as_ref() {
            running.handle.send(command);
        }
    }

    pub fn add_sink(&self, key: String, sink: Box<dyn Sink>) {
        self.send(Command::AddSink { key, sink });
    }

    pub fn remove_sink(&self, key: &str) {
        self.send(Command::RemoveSink {
            key: key.to_owned(),
        });
    }

    /// Puts `queue` in place of an idle one, resting where it says to resume.
    pub fn replace(&self, queue: Queue) {
        *self.position.lock().unwrap() = Position {
            offset: queue.resume_position,
            since: None,
        };
        *self.queue.lock().unwrap() = queue;
    }

    /// Runs `f` on the queue and carries out what it asks of the player.
    pub fn update<T>(&self, f: impl FnOnce(&mut Queue) -> (T, Effect)) -> T {
        let (value, effect) = self.change(f);
        self.apply(effect);
        value
    }

    /// Runs `f` on the queue like [`QueueSession::update`], but holds back a start it asks
    /// for until [`QueueSession::release`], so that the outputs can get ready first. Returns
    /// the held start's epoch.
    pub fn update_held<T>(&self, f: impl FnOnce(&mut Queue) -> (T, Effect)) -> (T, Option<u64>) {
        let (value, effect) = self.change(f);
        if let Effect::PlayFrom(_) = effect {
            return (value, Some(self.next_epoch()));
        }
        self.apply(effect);
        (value, None)
    }

    /// Starts playback held back by [`QueueSession::update_held`], no earlier than
    /// `not_before`, unless something else has happened since.
    pub fn release(&self, epoch: u64, not_before: Option<Micros>) {
        // Held while sending, so that a stop cannot come between the check and the start.
        let current = self.epoch.lock().unwrap();
        if *current == epoch {
            self.send(Command::Restart { epoch, not_before });
        }
    }

    /// Gives up a start held back by [`QueueSession::update_held`], unless something else has
    /// happened since.
    pub fn abandon(&self, epoch: u64) {
        if *self.epoch.lock().unwrap() == epoch {
            self.stop();
        }
    }

    fn change<T>(&self, f: impl FnOnce(&mut Queue) -> (T, Effect)) -> (T, Effect) {
        let mut queue = self.queue.lock().unwrap();
        let (value, effect) = f(&mut queue);
        match effect {
            Effect::PlayFrom(_) => queue.state = PlaybackState::Playing,
            Effect::Stop => queue.stopped(),
            Effect::Pause | Effect::None | Effect::NextChanged => {}
        }
        (value, effect)
    }

    fn next_epoch(&self) -> u64 {
        let mut epoch = self.epoch.lock().unwrap();
        *epoch += 1;
        *epoch
    }

    fn apply(&self, effect: Effect) {
        match effect {
            Effect::None => {}
            Effect::PlayFrom(_) => {
                let epoch = self.next_epoch();
                self.send(Command::Restart {
                    epoch,
                    not_before: None,
                });
            }
            Effect::NextChanged => self.send(Command::NextChanged),
            Effect::Stop | Effect::Pause => {
                let epoch = self.next_epoch();
                {
                    let mut position = self.position.lock().unwrap();
                    if effect == Effect::Stop {
                        *position = Position::default();
                    } else {
                        position.offset = position.at(clock::now());
                        position.since = None;
                    }
                }
                self.send(Command::Stop { epoch });
                let _ = self.notices.send(Notice::Stopped);
            }
        }
    }

    /// Pauses where playback is now.
    pub fn pause(&self) {
        let position = self.position(clock::now());
        self.update(|queue| ((), queue.pause(position)));
    }

    /// Stops playing, keeping the queue as it is.
    pub fn stop(&self) {
        self.update(|_| ((), Effect::Stop));
    }

    /// Moves to `position` in the current item.
    pub fn seek(&self, position: Duration) {
        let state = self.queue.lock().unwrap().state;
        if state == PlaybackState::Playing {
            self.send(Command::Seek(position));
            return;
        }
        self.queue.lock().unwrap().resume_position = position;
        *self.position.lock().unwrap() = Position {
            offset: position,
            since: None,
        };
        let _ = self.notices.send(Notice::Seeked);
    }

    /// How far into the current item playback is at `now`.
    pub fn position(&self, now: Micros) -> Duration {
        self.position.lock().unwrap().at(now)
    }

    /// A copy of the queue as it stands.
    pub fn snapshot(&self) -> Queue {
        self.queue.lock().unwrap().clone()
    }

    pub fn notices(&self) -> broadcast::Receiver<Notice> {
        self.notices.subscribe()
    }

    /// Ends the session's engine, waiting until every sink has finished.
    pub fn shutdown(self) {
        let running = self.running.lock().unwrap().take();
        if let Some(running) = running {
            running.handle.shutdown();
        }
    }
}

async fn follow(
    mut events: mpsc::UnboundedReceiver<Tagged>,
    session: QueueSession,
    timeline: Option<Timeline>,
    engine: u64,
) {
    let current = || *session.epoch.lock().unwrap();
    while let Some(tagged) = events.recv().await {
        // What befalls the engine and its outputs holds whatever the epoch.
        match &tagged.event {
            Event::OutputFailed { key, .. } => {
                let _ = session.notices.send(Notice::OutputFailed(key.clone()));
                continue;
            }
            Event::Crashed { error } => {
                let mut running = session.running.lock().unwrap();
                if running.as_ref().is_some_and(|r| r.id == engine) {
                    *running = None;
                    drop(running);
                    let _ = session.notices.send(Notice::Crashed(error.clone()));
                }
                return;
            }
            _ => {}
        }
        if tagged.epoch != current() {
            continue;
        }
        // Real-time sessions report each change when it can be heard.
        let when = match &tagged.event {
            Event::TrackStarted { at, .. } | Event::Seeked { at, .. } | Event::Idle { at } => {
                timeline.map(|t| t.instant_of(*at))
            }
            Event::TrackEnded { .. }
            | Event::TrackFailed { .. }
            | Event::OutputFailed { .. }
            | Event::Crashed { .. } => None,
        };
        if let Some(when) = when {
            let wait = when - clock::now();
            if wait > 0 {
                tokio::time::sleep(Duration::from_micros(wait as u64)).await;
            }
            if tagged.epoch != current() {
                continue;
            }
        }
        let since = when.unwrap_or_else(clock::now);
        let notice = {
            let mut queue = session.queue.lock().unwrap();
            match tagged.event {
                Event::TrackStarted { id, start, .. } => queue.index_of(&id).map(|index| {
                    queue.started(index);
                    *session.position.lock().unwrap() = Position {
                        offset: start,
                        since: Some(since),
                    };
                    Notice::Started(index)
                }),
                Event::Seeked { position, .. } => {
                    *session.position.lock().unwrap() = Position {
                        offset: position,
                        since: Some(since),
                    };
                    Some(Notice::Seeked)
                }
                Event::TrackFailed { id, error } => Some(Notice::Failed {
                    queue_item_id: id,
                    error,
                }),
                Event::Idle { .. } if queue.state == PlaybackState::Playing => {
                    queue.finished();
                    *session.position.lock().unwrap() = Position::default();
                    Some(Notice::Finished)
                }
                Event::Idle { .. }
                | Event::TrackEnded { .. }
                | Event::OutputFailed { .. }
                | Event::Crashed { .. } => None,
            }
        };
        if let Some(notice) = notice {
            let _ = session.notices.send(notice);
        }
    }
}
