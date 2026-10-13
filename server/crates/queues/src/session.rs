//! A queue wired to a stream engine. The engine asks the queue for tracks in order; the queue
//! learns from the engine's events which item is playing and when it has run out.

use std::sync::{Arc, Mutex};

use bopsie_audio::engine::{
    self, Command, EngineHandle, Event, Settings, TrackRequest, TrackSource,
};
use bopsie_audio::sink::Sink;
use tokio::sync::{broadcast, mpsc};

use crate::state::{Effect, PlaybackState, Queue};

/// What changed in a session, for whoever reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// The item at this index started playing.
    Started(usize),
    /// A track could not be played and was skipped.
    Failed {
        queue_item_id: String,
        error: String,
    },
    /// The queue played to its end.
    Finished,
}

/// Hands the queue's items to the engine, while the queue is meant to be playing.
struct Source {
    queue: Arc<Mutex<Queue>>,
}

impl TrackSource for Source {
    fn next(&mut self) -> Option<TrackRequest> {
        let mut queue = self.queue.lock().unwrap();
        if queue.state == PlaybackState::Idle {
            return None;
        }
        let item = queue.hand_over_next()?;
        Some(TrackRequest {
            id: item.queue_item_id.clone(),
            path: item.path.clone(),
            start: std::time::Duration::ZERO,
        })
    }
}

pub struct QueueSession {
    queue: Arc<Mutex<Queue>>,
    engine: std::sync::mpsc::Sender<Command>,
    /// The epoch of the last `Restart` or `Stop`; older events are ignored.
    epoch: Arc<Mutex<u64>>,
    notices: broadcast::Sender<Notice>,
    handle: EngineHandle,
}

impl QueueSession {
    /// Starts an engine for `queue`, idle until told to play. Needs a Tokio runtime.
    pub fn start(queue: Queue, settings: Settings, sinks: Vec<(String, Box<dyn Sink>)>) -> Self {
        let queue = Arc::new(Mutex::new(queue));
        let mut handle = engine::spawn(
            settings,
            Box::new(Source {
                queue: queue.clone(),
            }),
            sinks,
        );
        let engine = handle.commander();
        let epoch = Arc::new(Mutex::new(0));
        let (notices, _) = broadcast::channel(64);
        let events = std::mem::replace(&mut handle.events, mpsc::unbounded_channel().1);
        tokio::spawn(follow(
            events,
            queue.clone(),
            epoch.clone(),
            notices.clone(),
        ));
        Self {
            queue,
            engine,
            epoch,
            notices,
            handle,
        }
    }

    /// Runs `f` on the queue and carries out what it asks of the player.
    pub fn update<T>(&self, f: impl FnOnce(&mut Queue) -> (T, Effect)) -> T {
        let (value, effect) = {
            let mut queue = self.queue.lock().unwrap();
            let (value, effect) = f(&mut queue);
            match effect {
                Effect::PlayFrom(_) => queue.state = PlaybackState::Playing,
                Effect::Stop => queue.stopped(),
                Effect::None | Effect::NextChanged => {}
            }
            (value, effect)
        };
        self.apply(effect);
        value
    }

    fn next_epoch(&self) -> u64 {
        let mut epoch = self.epoch.lock().unwrap();
        *epoch += 1;
        *epoch
    }

    fn apply(&self, effect: Effect) {
        let command = match effect {
            Effect::None => return,
            Effect::PlayFrom(_) => Command::Restart {
                epoch: self.next_epoch(),
            },
            Effect::NextChanged => Command::NextChanged,
            Effect::Stop => Command::Stop {
                epoch: self.next_epoch(),
            },
        };
        let _ = self.engine.send(command);
    }

    /// Stops playing, keeping the queue as it is.
    pub fn stop(&self) {
        self.update(|_| ((), Effect::Stop));
    }

    /// A copy of the queue as it stands.
    pub fn snapshot(&self) -> Queue {
        self.queue.lock().unwrap().clone()
    }

    pub fn notices(&self) -> broadcast::Receiver<Notice> {
        self.notices.subscribe()
    }

    /// Ends the session, waiting until every sink has finished.
    pub fn shutdown(self) {
        self.handle.shutdown();
    }
}

async fn follow(
    mut events: mpsc::UnboundedReceiver<engine::Tagged>,
    queue: Arc<Mutex<Queue>>,
    epoch: Arc<Mutex<u64>>,
    notices: broadcast::Sender<Notice>,
) {
    while let Some(tagged) = events.recv().await {
        if tagged.epoch != *epoch.lock().unwrap() {
            continue;
        }
        let notice = {
            let mut queue = queue.lock().unwrap();
            match tagged.event {
                Event::TrackStarted { id, .. } => queue.index_of(&id).map(|index| {
                    queue.started(index);
                    Notice::Started(index)
                }),
                Event::TrackFailed { id, error } => Some(Notice::Failed {
                    queue_item_id: id,
                    error,
                }),
                Event::Idle { .. } if queue.state == PlaybackState::Playing => {
                    queue.finished();
                    Some(Notice::Finished)
                }
                Event::Idle { .. } | Event::TrackEnded { .. } | Event::Seeked { .. } => None,
            }
        };
        if let Some(notice) = notice {
            let _ = notices.send(notice);
        }
    }
}
