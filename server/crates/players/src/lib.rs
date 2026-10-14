//! The `PlayerProvider` and `Output` traits, the device registry, endpoint linking, quirk rules,
//! the group planner, health and group sessions.
//!
//! Protocol crates implement the traits defined here and depend on this crate, never the reverse.

pub mod devices;
mod facts;
pub mod health;
pub mod linking;
pub mod planner;
pub mod quirks;
pub mod remote;
pub mod settings;

pub use devices::{Device, Devices, Member};
pub use facts::{Capabilities, Facts, NativeGroup, Pairing};
pub use remote::Remote;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use bopsie_audio::OutputFormat;
use bopsie_audio::sink::Sink;
use bopsie_audio::timeline::{Micros, Timeline};
use bopsie_core::config::PlayerConfig;
use bopsie_core::db::Database;
use tokio::sync::broadcast;

use crate::quirks::{Quirks, Resolved, Subject};
use crate::settings::{DeviceSettings, EndpointSettings, Layers, SavedDevice, SavedEndpoint};

/// The scopes saved settings are kept under.
const DEVICE: &str = "device";
const ENDPOINT: &str = "endpoint";

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
    /// The endpoint it follows in its vendor's own multiroom, if it does: then it plays only
    /// what that one plays.
    pub native_leader: Option<String>,
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
    /// Follows `leader`, an endpoint of the same native group family, in that multiroom. May
    /// block for seconds.
    fn join_native(&self, leader: &str) -> Result<(), String> {
        Err(format!("{} cannot follow {leader} natively", self.id()))
    }
    /// Stops following its native leader, or stops leading its native followers. May block
    /// for seconds.
    fn leave_native(&self) -> Result<(), String> {
        Ok(())
    }
    /// What the protocol knows of the endpoint beyond its state, for an admin to read when it
    /// misbehaves. Its shape is the protocol's own.
    fn diagnostics(&self) -> serde_json::Value {
        serde_json::Value::Null
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Added(String),
    Updated(String),
    Removed(String),
}

/// Every endpoint the server knows. An endpoint that disconnects stays, unavailable, so that
/// it keeps its queue and comes back under the same id.
#[derive(Clone)]
pub struct Players {
    endpoints: Arc<Mutex<BTreeMap<String, Arc<dyn Endpoint>>>>,
    changes: broadcast::Sender<Change>,
    /// The config file's settings and the saved ones over them.
    settings: Arc<RwLock<Layers>>,
    /// Where saved settings are kept, once [`Players::load`] is called.
    db: Arc<OnceLock<Database>>,
    /// What outputs saw of each endpoint.
    logs: Arc<Mutex<BTreeMap<String, health::Log>>>,
    /// What speakers' own controls asked for, by endpoint.
    remotes: broadcast::Sender<(String, Remote)>,
    guard: Arc<Mutex<remote::Guard>>,
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
            settings: Arc::new(RwLock::new(Layers {
                file: settings,
                ..Layers::default()
            })),
            db: Arc::default(),
            logs: Arc::default(),
            remotes: broadcast::channel(64).0,
            guard: Arc::default(),
        }
    }

    /// Records what an output saw of endpoint `id`, announcing a change in its health level.
    pub fn record(&self, id: &str, event: health::Event) {
        tracing::debug!(endpoint = id, ?event, "telemetry");
        let changed = self
            .logs
            .lock()
            .unwrap()
            .entry(id.to_owned())
            .or_default()
            .record(event, std::time::Instant::now());
        if changed {
            self.changed(id);
        }
    }

    pub fn health(&self, id: &str) -> health::Health {
        self.logs
            .lock()
            .unwrap()
            .get(id)
            .map(|log| log.health(std::time::Instant::now()))
            .unwrap_or_default()
    }

    /// Endpoint `id`'s own controls asked for `remote`. A quick repeat of the same command is
    /// taken for the same press and dropped.
    pub fn remote(&self, id: &str, remote: Remote) {
        if !self
            .guard
            .lock()
            .unwrap()
            .admit(id, remote, std::time::Instant::now())
        {
            tracing::debug!(endpoint = id, ?remote, "a repeat of the same press");
            return;
        }
        tracing::info!(endpoint = id, ?remote, "the speaker's own controls");
        let _ = self.remotes.send((id.to_owned(), remote));
    }

    /// What speakers' own controls ask for, as they ask.
    pub fn remotes(&self) -> broadcast::Receiver<(String, Remote)> {
        self.remotes.subscribe()
    }

    /// What outputs saw of `id` lately, newest first, each with how long ago.
    pub fn history(&self, id: &str) -> Vec<(std::time::Duration, health::Event)> {
        self.logs
            .lock()
            .unwrap()
            .get(id)
            .map(|log| log.history(std::time::Instant::now()))
            .unwrap_or_default()
    }

    /// Reads the settings saved in `db`, and keeps them there from now on.
    pub async fn load(&self, db: Database) -> bopsie_core::Result<()> {
        let rows = db
            .call(|conn| {
                let mut statement =
                    conn.prepare("SELECT scope, id, settings FROM player_settings")?;
                let rows = statement
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
                    .collect::<rusqlite::Result<Vec<(String, String, String)>>>()?;
                Ok(rows)
            })
            .await?;
        {
            let mut layers = self.settings.write().unwrap();
            for (scope, id, json) in rows {
                let parsed = match scope.as_str() {
                    DEVICE => serde_json::from_str(&json).map(|s| {
                        layers.devices.insert(id.clone(), s);
                    }),
                    _ => serde_json::from_str(&json).map(|s| {
                        layers.endpoints.insert(id.clone(), s);
                    }),
                };
                if let Err(error) = parsed {
                    tracing::warn!(id, %error, "ignoring unreadable saved settings");
                }
            }
        }
        let _ = self.db.set(db);
        Ok(())
    }

    /// Device `id`'s settings: saved, else the config file's, else the defaults.
    pub fn device_settings(&self, id: &str) -> DeviceSettings {
        self.settings.read().unwrap().device(id)
    }

    /// Endpoint `id`'s settings: saved, else the config file's, else the defaults.
    pub fn endpoint_settings(&self, id: &str) -> EndpointSettings {
        self.settings.read().unwrap().endpoint(id)
    }

    /// What was saved for device `id`, without the config file's.
    pub fn saved_device(&self, id: &str) -> SavedDevice {
        let layers = self.settings.read().unwrap();
        layers.devices.get(id).cloned().unwrap_or_default()
    }

    /// What was saved for endpoint `id`, without the config file's.
    pub fn saved_endpoint(&self, id: &str) -> SavedEndpoint {
        let layers = self.settings.read().unwrap();
        layers.endpoints.get(id).cloned().unwrap_or_default()
    }

    /// Saves device `id`'s settings, replacing what was saved. Callers announce the change
    /// on the device's endpoints, which only [`Devices`] knows.
    pub async fn save_device(&self, id: &str, saved: SavedDevice) -> bopsie_core::Result<()> {
        self.persist(DEVICE, id, &saved, saved == SavedDevice::default())
            .await?;
        self.settings
            .write()
            .unwrap()
            .devices
            .insert(id.to_owned(), saved);
        Ok(())
    }

    /// Saves endpoint `id`'s settings, replacing what was saved, and announces the change.
    pub async fn save_endpoint(&self, id: &str, saved: SavedEndpoint) -> bopsie_core::Result<()> {
        self.persist(ENDPOINT, id, &saved, saved == SavedEndpoint::default())
            .await?;
        self.settings
            .write()
            .unwrap()
            .endpoints
            .insert(id.to_owned(), saved);
        self.changed(id);
        Ok(())
    }

    async fn persist(
        &self,
        scope: &'static str,
        id: &str,
        saved: &impl serde::Serialize,
        empty: bool,
    ) -> bopsie_core::Result<()> {
        let Some(db) = self.db.get() else {
            return Ok(());
        };
        let id = id.to_owned();
        let json = serde_json::to_string(saved).map_err(|_| bopsie_core::Error::unexpected())?;
        db.call(move |conn| {
            if empty {
                conn.execute(
                    "DELETE FROM player_settings WHERE scope = ?1 AND id = ?2",
                    (scope, &id),
                )?;
            } else {
                conn.execute(
                    "INSERT INTO player_settings (scope, id, settings) VALUES (?1, ?2, ?3)
                     ON CONFLICT (scope, id) DO UPDATE SET settings = excluded.settings",
                    (scope, &id, &json),
                )?;
            }
            Ok(())
        })
        .await?;
        Ok(())
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
            &self
                .endpoint_settings(id)
                .quirks
                .iter()
                .map(|(quirks, source)| (quirks, source.clone()))
                .collect::<Vec<_>>(),
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
