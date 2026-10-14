//! The `PlayerProvider` and `Output` traits, the device registry, endpoint linking, quirk rules,
//! the group planner, health and group sessions.
//!
//! Protocol crates implement the traits defined here and depend on this crate, never the reverse.

mod facts;
pub mod quirks;

pub use facts::{Capabilities, Facts, Pairing};

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use bopsie_audio::OutputFormat;
use bopsie_audio::sink::Sink;
use bopsie_audio::timeline::{Micros, Timeline};
use bopsie_core::config::PlayerConfig;
use tokio::sync::broadcast;

use crate::quirks::{Quirks, Resolved, Subject};

/// What a player says about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub model: Option<String>,
    pub manufacturer: Option<String>,
    pub software_version: Option<String>,
}

/// An endpoint's state as the rest of the server sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointState {
    pub name: String,
    /// The protocol, such as `sendspin`.
    pub provider: &'static str,
    pub available: bool,
    pub volume: Option<u8>,
    pub muted: Option<bool>,
    pub device: DeviceInfo,
    /// An app's own player, which only its user sees and controls.
    pub app: bool,
    /// The token the app connected with, whose user owns this player.
    pub owner_token: Option<String>,
    pub facts: Facts,
    pub capabilities: Capabilities,
    /// Its quirks, from [`Players::resolve_quirks`].
    pub quirks: Resolved,
}

/// What is playing, for endpoints that show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NowPlaying {
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub year: Option<i32>,
    pub track_number: Option<u32>,
    pub artwork_url: Option<String>,
    /// Milliseconds.
    pub duration: u64,
    /// Milliseconds into the track.
    pub position: u64,
    pub playing: bool,
}

/// One protocol reachable at one address: a player the server can stream to.
pub trait Endpoint: Send + Sync {
    fn id(&self) -> &str;
    fn state(&self) -> EndpointState;
    /// A sink that plays a session's audio, packed as `format`, on `timeline`.
    fn output(&self, format: OutputFormat, timeline: Timeline) -> Box<dyn Sink>;
    fn set_volume(&self, volume: u8);
    fn set_muted(&self, muted: bool);
    /// What is playing, or `None` when nothing is.
    fn set_now_playing(&self, now_playing: Option<&NowPlaying>);
    /// Whether the group the endpoint is in is playing, and which group that is.
    fn set_group(&self, playing: bool, group_id: &str);
    /// Gets ready to play, and says when it can start, on the server's clock, if that needs
    /// longer than its lead: a speaker that has to connect, or whose clock has yet to follow
    /// ours. May block for seconds.
    fn prepare(&self) -> Option<Micros> {
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Added(String),
    Updated(String),
}

/// Every endpoint the server knows. An endpoint that disconnects stays, unavailable, so that
/// it keeps its queue and comes back under the same id.
#[derive(Clone)]
pub struct Players {
    endpoints: Arc<Mutex<BTreeMap<String, Arc<dyn Endpoint>>>>,
    changes: broadcast::Sender<Change>,
    /// The config file's settings, by player id.
    settings: Arc<BTreeMap<String, PlayerConfig>>,
}

impl Default for Players {
    fn default() -> Self {
        Self::new(BTreeMap::new())
    }
}

impl Players {
    /// A registry with the config file's settings for each player.
    pub fn new(settings: BTreeMap<String, PlayerConfig>) -> Self {
        Self {
            endpoints: Arc::default(),
            changes: broadcast::channel(256).0,
            settings: Arc::new(settings),
        }
    }

    /// The config file's settings for `id`.
    pub fn settings(&self, id: &str) -> PlayerConfig {
        self.settings.get(id).cloned().unwrap_or_default()
    }

    /// The quirks of endpoint `id`, whose state is `state`, over its protocol's `defaults`.
    pub fn resolve_quirks(&self, id: &str, state: &EndpointState, defaults: &Quirks) -> Resolved {
        quirks::resolve(
            defaults,
            quirks::shipped(),
            &Subject {
                provider: state.provider,
                device: &state.device,
                facts: &state.facts,
            },
            self.settings.get(id).map(|settings| &settings.quirks),
        )
    }

    /// Adds an endpoint, or replaces the one with the same id, as when a player reconnects.
    pub fn register(&self, endpoint: Arc<dyn Endpoint>) {
        let id = endpoint.id().to_owned();
        let known = self
            .endpoints
            .lock()
            .unwrap()
            .insert(id.clone(), endpoint)
            .is_some();
        let change = if known {
            Change::Updated(id)
        } else {
            Change::Added(id)
        };
        let _ = self.changes.send(change);
    }

    /// Announces that an endpoint's state changed.
    pub fn changed(&self, id: &str) {
        let _ = self.changes.send(Change::Updated(id.to_owned()));
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn Endpoint>> {
        self.endpoints.lock().unwrap().get(id).cloned()
    }

    pub fn all(&self) -> Vec<Arc<dyn Endpoint>> {
        self.endpoints.lock().unwrap().values().cloned().collect()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Change> {
        self.changes.subscribe()
    }
}
