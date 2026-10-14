//! Sources: audio from outside the queue that plays to a player's group, such as a Spotify
//! Connect session. A source is announced for one player; when it starts, the group that
//! player is in plays it in place of its queue, and the group's controls act on it.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bopsie_audio::decode::Decoder;
use tokio::sync::broadcast;

/// What a source plays.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceMedia {
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub artwork_url: Option<String>,
    /// Milliseconds.
    pub duration_ms: u64,
    /// The source's own name for it, such as a `spotify:track:` URI.
    pub uri: Option<String>,
}

/// How a source stands.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceState {
    /// It has a session, as when someone picked it in the Spotify app.
    pub connected: bool,
    pub playing: bool,
    pub media: Option<SourceMedia>,
    /// Milliseconds into the media, as of `at`.
    pub position_ms: u64,
    /// When `position_ms` was read, on the server's clock.
    pub at: i64,
}

impl SourceState {
    /// Milliseconds into the media at `now`, on the server's clock.
    pub fn position_at(&self, now: i64) -> u64 {
        if !self.playing {
            return self.position_ms;
        }
        let elapsed = u64::try_from((now - self.at) / 1_000).unwrap_or(0);
        let position = self.position_ms + elapsed;
        match self.media.as_ref().map(|m| m.duration_ms) {
            Some(duration) if duration > 0 => position.min(duration),
            _ => position,
        }
    }
}

/// What the group's controls ask of a source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    Play,
    Pause,
    Next,
    Previous,
    /// Milliseconds into the media.
    Seek(u64),
    /// The group's volume, 0 to 100.
    Volume(u8),
    /// The group plays something else now: the source stops playing to it.
    Release,
}

pub trait Source: Send + Sync {
    /// Such as `spotify_connect--kitchen`.
    fn id(&self) -> &str;
    /// The player it is announced for.
    fn player(&self) -> &str;
    /// What it is called, such as `Spotify`.
    fn name(&self) -> String;
    fn state(&self) -> SourceState;
    /// Its audio from now on, as a live input. Taking it again ends the one taken before.
    fn input(&self) -> Box<dyn Decoder>;
    fn control(&self, control: Control);
    /// The last `unheard` of the audio taken was never heard, as when the group paused with
    /// it still on its way to the speakers: the source goes back by that much, and by what it
    /// still holds itself, so that playing on picks up where the listener left off.
    fn unheard(&self, unheard: Duration);
}

/// What happened to a source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Happened {
    /// It started playing, or played on after a pause.
    Playing,
    Paused,
    /// It jumped within its media; what it gives from now on starts there.
    Seeked,
    /// Its media changed, as one track follows another.
    MediaChanged,
    /// Its own volume control moved, as the Spotify app's slider: 0 to 100.
    Volume(u8),
    /// It stopped playing to the group, as when the listener picked another device.
    Ended,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceEvent {
    pub source: String,
    pub happened: Happened,
}

/// Every source the server has.
#[derive(Clone)]
pub struct Sources {
    sources: Arc<Mutex<BTreeMap<String, Arc<dyn Source>>>>,
    events: broadcast::Sender<SourceEvent>,
}

impl Default for Sources {
    fn default() -> Self {
        Self {
            sources: Arc::default(),
            events: broadcast::channel(256).0,
        }
    }
}

impl Sources {
    pub fn register(&self, source: Arc<dyn Source>) {
        self.sources
            .lock()
            .unwrap()
            .insert(source.id().to_owned(), source);
    }

    /// Forgets a source, which ends what it played.
    pub fn remove(&self, id: &str) {
        if self.sources.lock().unwrap().remove(id).is_some() {
            self.report(id, Happened::Ended);
        }
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn Source>> {
        self.sources.lock().unwrap().get(id).cloned()
    }

    pub fn all(&self) -> Vec<Arc<dyn Source>> {
        self.sources.lock().unwrap().values().cloned().collect()
    }

    /// The sources announced for `player`.
    pub fn of_player(&self, player: &str) -> Vec<Arc<dyn Source>> {
        self.sources
            .lock()
            .unwrap()
            .values()
            .filter(|source| source.player() == player)
            .cloned()
            .collect()
    }

    /// Source `id` says what happened to it.
    pub fn report(&self, id: &str, happened: Happened) {
        let _ = self.events.send(SourceEvent {
            source: id.to_owned(),
            happened,
        });
    }

    pub fn subscribe(&self) -> broadcast::Receiver<SourceEvent> {
        self.events.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_playing_position_runs_on_to_the_end() {
        let state = SourceState {
            playing: true,
            media: Some(SourceMedia {
                duration_ms: 10_000,
                ..SourceMedia::default()
            }),
            position_ms: 4_000,
            at: 1_000_000,
            ..SourceState::default()
        };
        assert_eq!(state.position_at(3_000_000), 6_000);
        assert_eq!(state.position_at(60_000_000), 10_000);
        let paused = SourceState {
            playing: false,
            ..state
        };
        assert_eq!(paused.position_at(60_000_000), 4_000);
    }
}
