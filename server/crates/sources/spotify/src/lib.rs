//! Spotify Connect: each speaker as a device in the Spotify app. Picking one plays Spotify to
//! the group the speaker is in, through the server, as a [`bopsie_sources::Source`].
//!
//! Every enabled speaker is announced unless its settings turn Spotify Connect off; players
//! that are apps, such as Bopsie's own, are not. The credentials of whoever picked a device
//! are kept in the data directory, so it reconnects after a restart.

mod device;
mod events;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use bopsie_players::{Change, Device, Devices};
use bopsie_sources::Sources;
use sha2::{Digest, Sha256};
use tokio::sync::broadcast::error::RecvError;

pub use device::ConnectDevice;

#[derive(Debug, Clone)]
pub struct Config {
    /// Where each device keeps the credentials it was given, under `spotify/`.
    pub data_dir: PathBuf,
    /// Stable across restarts, so each device keeps its identity in the Spotify app.
    pub server_id: String,
}

/// The Spotify Connect devices, one for each speaker that wants one.
#[derive(Clone)]
pub struct SpotifyConnect {
    inner: Arc<Inner>,
}

struct Inner {
    config: Config,
    devices: Devices,
    sources: Sources,
    running: Mutex<BTreeMap<String, Arc<ConnectDevice>>>,
}

impl SpotifyConnect {
    /// Announces a device for each speaker that wants one, and follows them as they come, go
    /// and change. Needs a Tokio runtime.
    pub fn start(config: Config, devices: Devices, sources: Sources) -> Self {
        let connect = Self {
            inner: Arc::new(Inner {
                config,
                devices: devices.clone(),
                sources,
                running: Mutex::default(),
            }),
        };
        let mut changes = devices.subscribe();
        for device in devices.all() {
            connect.follow(&device.id);
        }
        let follower = connect.clone();
        tokio::spawn(async move {
            loop {
                match changes.recv().await {
                    Ok(Change::Added(id) | Change::Updated(id) | Change::Removed(id)) => {
                        follower.follow(&id);
                    }
                    Err(RecvError::Lagged(_)) => {
                        for device in follower.inner.devices.all() {
                            follower.follow(&device.id);
                        }
                    }
                    Err(RecvError::Closed) => break,
                }
            }
        });
        connect
    }

    /// Starts, renames or stops the device for player `id` to match its settings.
    fn follow(&self, id: &str) {
        let wanted = self
            .inner
            .devices
            .get(id)
            .and_then(|device| wanted(&device));
        let mut running = self.inner.running.lock().unwrap();
        let current = running.get(id).map(|device| device.name().to_owned());
        if current == wanted.as_ref().map(|(name, _)| name.clone()) {
            return;
        }
        if let Some(old) = running.remove(id) {
            tracing::info!(player = id, "no longer a Spotify Connect device as it was");
            old.shutdown();
        }
        if let Some((name, volume)) = wanted {
            let device = ConnectDevice::start(
                id,
                &name,
                volume,
                device_id(&self.inner.config.server_id, id),
                self.inner
                    .config
                    .data_dir
                    .join("spotify")
                    .join(&device_id(&self.inner.config.server_id, id)[..16]),
                &self.inner.sources,
            );
            running.insert(id.to_owned(), device);
        }
    }

    /// Stops every device.
    pub fn shutdown(&self) {
        for (_, device) in std::mem::take(&mut *self.inner.running.lock().unwrap()) {
            device.shutdown();
        }
    }
}

/// The name and volume a device is announced with, if it should be: an enabled speaker, not an
/// app, that has not been turned off for Spotify Connect, and that could play when it started.
fn wanted(device: &Device) -> Option<(String, u8)> {
    let primary = device.primary();
    (device.enabled && device.spotify_connect && !primary.state.app && device.available())
        .then(|| (device.name.clone(), primary.state.volume.unwrap_or(50)))
}

/// A device id unique to this server and player, and the same across restarts.
fn device_id(server_id: &str, player: &str) -> String {
    hex::encode(&Sha256::digest(format!("{server_id}/{player}"))[..20])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_ids_are_stable_and_distinct() {
        assert_eq!(
            device_id("server", "kitchen"),
            device_id("server", "kitchen")
        );
        assert_ne!(
            device_id("server", "kitchen"),
            device_id("server", "bedroom")
        );
        assert_ne!(
            device_id("server", "kitchen"),
            device_id("other", "kitchen")
        );
        assert_eq!(device_id("server", "kitchen").len(), 40);
    }
}
