//! Devices: what the user sees, names and groups, each reached over one or more endpoints.
//!
//! Endpoints are linked into devices afresh whenever devices are asked for, so the answer is
//! never behind the endpoints. A device's id is kept in the database, so that a speaker keeps
//! its id, and so its queue, whichever of its endpoints is found first after a restart.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use bopsie_core::db::{Database, Migrations};
use tokio::sync::broadcast;

use crate::health::Health;
use crate::linking::{assign, link};
use crate::{Change, Endpoint, EndpointState, Players};

pub const MIGRATIONS: Migrations = Migrations {
    component: "players",
    steps: &[
        "
        CREATE TABLE device_endpoints (
            endpoint_id TEXT PRIMARY KEY,
            device_id TEXT NOT NULL
        ) STRICT;
    ",
        "
        -- Settings saved from the web UI, as JSON: a device's or an endpoint's, by its id.
        CREATE TABLE player_settings (
            scope TEXT NOT NULL,
            id TEXT NOT NULL,
            settings TEXT NOT NULL,
            PRIMARY KEY (scope, id)
        ) STRICT;
    ",
    ],
};

/// Protocols in order of preference, as Music Assistant ranks them: a device is first
/// reached over the earliest it has.
pub const PROTOCOLS: &[&str] = &["airplay", "sendspin"];

/// One of a device's endpoints, as it is now.
#[derive(Clone)]
pub struct Member {
    pub id: String,
    pub endpoint: Arc<dyn Endpoint>,
    pub state: EndpointState,
    pub health: Health,
    /// Turned off in the device's settings: it plays nothing, and its state says unavailable.
    pub disabled: bool,
}

/// A device as it is now.
#[derive(Clone)]
pub struct Device {
    pub id: String,
    /// The name from its settings, else its preferred endpoint's.
    pub name: String,
    /// Unless its settings disable it.
    pub enabled: bool,
    /// The loudest it may be set, from its settings.
    pub max_volume: Option<u8>,
    /// In order of protocol preference.
    pub members: Vec<Member>,
}

impl Device {
    /// Whether any of its endpoints can play.
    pub fn available(&self) -> bool {
        self.enabled && self.members.iter().any(|m| m.state.available)
    }

    /// The endpoint the device is shown as: its preferred one that can play, else its
    /// preferred one.
    pub fn primary(&self) -> &Member {
        self.members
            .iter()
            .find(|m| m.state.available)
            .unwrap_or(&self.members[0])
    }

    pub fn member(&self, endpoint_id: &str) -> Option<&Member> {
        self.members.iter().find(|m| m.id == endpoint_id)
    }
}

struct Inner {
    players: Players,
    db: Database,
    /// Each endpoint's device id, as last assigned.
    known: Mutex<BTreeMap<String, String>>,
    /// Each device's endpoints, as last announced.
    announced: Mutex<BTreeMap<String, Vec<String>>>,
    changes: broadcast::Sender<Change>,
}

#[derive(Clone)]
pub struct Devices {
    inner: Arc<Inner>,
}

/// Where `provider` comes in [`PROTOCOLS`].
fn rank(provider: &str) -> usize {
    PROTOCOLS
        .iter()
        .position(|p| *p == provider)
        .unwrap_or(PROTOCOLS.len())
}

impl Devices {
    /// The devices of `players`' endpoints, with device ids from `db`. Announces device
    /// changes as endpoints change. Needs a Tokio runtime.
    pub async fn new(players: Players, db: Database) -> bopsie_core::Result<Self> {
        let known = db
            .call(|conn| {
                let mut statement =
                    conn.prepare("SELECT endpoint_id, device_id FROM device_endpoints")?;
                let rows = statement
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                    .collect::<rusqlite::Result<BTreeMap<String, String>>>()?;
                Ok(rows)
            })
            .await?;
        let devices = Self {
            inner: Arc::new(Inner {
                players: players.clone(),
                db,
                known: Mutex::new(known),
                announced: Mutex::default(),
                changes: broadcast::channel(256).0,
            }),
        };
        devices.announce(None);
        let mut changes = players.subscribe();
        let follower = devices.clone();
        tokio::spawn(async move {
            loop {
                match changes.recv().await {
                    Ok(Change::Added(id) | Change::Updated(id) | Change::Removed(id)) => {
                        follower.announce(Some(&id));
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => follower.announce(None),
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        });
        Ok(devices)
    }

    pub fn players(&self) -> &Players {
        &self.inner.players
    }

    /// Every device, by id.
    pub fn all(&self) -> Vec<Device> {
        let mut endpoints: Vec<Member> = self
            .inner
            .players
            .all()
            .into_iter()
            .map(|endpoint| {
                let disabled = !self
                    .inner
                    .players
                    .endpoint_settings(endpoint.id())
                    .enabled
                    .value;
                let mut state = endpoint.state();
                state.available &= !disabled;
                Member {
                    id: endpoint.id().to_owned(),
                    state,
                    health: self.inner.players.health(endpoint.id()),
                    endpoint,
                    disabled,
                }
            })
            .collect();
        endpoints
            .sort_by(|a, b| (rank(a.state.provider), &a.id).cmp(&(rank(b.state.provider), &b.id)));
        let states: Vec<&EndpointState> = endpoints.iter().map(|m| &m.state).collect();
        let groups: Vec<Vec<String>> = link(&states)
            .into_iter()
            .map(|group| group.into_iter().map(|i| endpoints[i].id.clone()).collect())
            .collect();
        let assigned = {
            let mut known = self.inner.known.lock().unwrap();
            let assigned = assign(&groups, &known);
            let mut changed = Vec::new();
            for (device, members) in &assigned {
                for endpoint in members {
                    if known.get(endpoint) != Some(device) {
                        known.insert(endpoint.clone(), device.clone());
                        changed.push((endpoint.clone(), device.clone()));
                    }
                }
            }
            self.remember(changed);
            assigned
        };
        let mut by_id: BTreeMap<String, Member> =
            endpoints.into_iter().map(|m| (m.id.clone(), m)).collect();
        let mut devices: Vec<Device> = assigned
            .into_iter()
            .map(|(id, members)| {
                let members: Vec<Member> = members
                    .iter()
                    .filter_map(|endpoint| by_id.remove(endpoint))
                    .collect();
                let settings = self.inner.players.device_settings(&id);
                Device {
                    name: settings
                        .name
                        .value
                        .unwrap_or_else(|| members[0].state.name.clone()),
                    enabled: settings.enabled.value,
                    max_volume: settings.max_volume.value,
                    id,
                    members,
                }
            })
            .collect();
        devices.sort_by(|a, b| a.id.cmp(&b.id));
        devices
    }

    pub fn get(&self, id: &str) -> Option<Device> {
        self.all().into_iter().find(|device| device.id == id)
    }

    /// The device `endpoint_id` belongs to.
    pub fn device_of(&self, endpoint_id: &str) -> Option<Device> {
        self.all()
            .into_iter()
            .find(|device| device.member(endpoint_id).is_some())
    }

    /// Device changes: one is added, updated or removed, as when two devices turn out to be
    /// one.
    pub fn subscribe(&self) -> broadcast::Receiver<Change> {
        self.inner.changes.subscribe()
    }

    /// Writes new device ids for endpoints to the database.
    fn remember(&self, changed: Vec<(String, String)>) {
        if changed.is_empty() {
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let db = self.inner.db.clone();
        runtime.spawn(async move {
            let written = db
                .call(move |conn| {
                    for (endpoint, device) in &changed {
                        conn.execute(
                            "INSERT INTO device_endpoints (endpoint_id, device_id) VALUES (?1, ?2)
                             ON CONFLICT (endpoint_id) DO UPDATE SET device_id = excluded.device_id",
                            (endpoint, device),
                        )?;
                    }
                    Ok(())
                })
                .await;
            if let Err(error) = written {
                tracing::warn!(%error, "cannot remember device ids");
            }
        });
    }

    /// Announces what changed since last time, and that endpoint `changed`'s device did.
    fn announce(&self, changed: Option<&str>) {
        let devices = self.all();
        let now: BTreeMap<String, Vec<String>> = devices
            .iter()
            .map(|d| {
                (
                    d.id.clone(),
                    d.members.iter().map(|m| m.id.clone()).collect(),
                )
            })
            .collect();
        let mut announced = self.inner.announced.lock().unwrap();
        let send = |change| {
            let _ = self.inner.changes.send(change);
        };
        for id in announced.keys() {
            if !now.contains_key(id) {
                send(Change::Removed(id.clone()));
            }
        }
        for (id, members) in &now {
            match announced.get(id) {
                None => send(Change::Added(id.clone())),
                Some(before) if before != members => send(Change::Updated(id.clone())),
                Some(_)
                    if changed.is_some_and(|endpoint| members.iter().any(|m| m == endpoint)) =>
                {
                    send(Change::Updated(id.clone()))
                }
                Some(_) => {}
            }
        }
        *announced = now;
    }
}
