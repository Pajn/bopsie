//! Spotify Connect: each speaker as a device in the Spotify app. Picking one plays Spotify to
//! the group the speaker is in, through the server, as a [`bopsie_sources::Source`].
//!
//! Every enabled speaker is announced unless its settings turn Spotify Connect off; players
//! that are apps, such as Bopsie's own, are not. The credentials of whoever picked a device
//! are kept in the data directory, so it reconnects after a restart.

pub mod account;
pub mod catalog;
mod device;
mod events;
pub mod media;

use std::collections::BTreeMap;
use std::net::{IpAddr, UdpSocket};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bopsie_core::config::SpotifyConfig;
use bopsie_players::{Change, Device, Devices};
use bopsie_sources::Sources;
use sha2::{Digest, Sha256};
use tokio::sync::broadcast::error::RecvError;

pub use account::{AccountError, AccountStatus, Endpoints, SpotifyAccount};
pub use device::ConnectDevice;

#[derive(Debug, Clone)]
pub struct Config {
    /// Where each device keeps the credentials it was given, under `spotify/`.
    pub data_dir: PathBuf,
    /// Stable across restarts, so each device keeps its identity in the Spotify app.
    pub server_id: String,
    /// The addresses the devices are announced at; empty for [`local_addresses`].
    pub addresses: Vec<IpAddr>,
    /// What speakers are called, unless their settings say: `[spotify]` in the config file.
    pub names: SpotifyConfig,
}

/// The Spotify Connect devices, one for each speaker that wants one.
#[derive(Clone)]
pub struct SpotifyConnect {
    inner: Arc<Inner>,
}

/// How long a speaker is seen before its device is announced, so that it is announced under
/// the name it settles on: some give a provisional one when they are first found.
const SETTLE: Duration = Duration::from_secs(10);
/// How long a speaker may be away before its device is withdrawn, so that one that drops off
/// the network for a few minutes does not come and go in the Spotify app.
const GRACE: Duration = Duration::from_secs(15 * 60);

struct Inner {
    config: Config,
    devices: Devices,
    sources: Sources,
    running: Mutex<BTreeMap<String, Arc<ConnectDevice>>>,
    /// When each speaker was first seen able to play.
    seen: Mutex<BTreeMap<String, Instant>>,
    /// Since when each announced speaker has been away.
    away: Mutex<BTreeMap<String, Instant>>,
}

impl SpotifyConnect {
    /// Announces a device for each speaker that wants one, and follows them as they come, go
    /// and change. Needs a Tokio runtime.
    pub fn start(mut config: Config, devices: Devices, sources: Sources) -> Self {
        if config.addresses.is_empty() {
            config.addresses = local_addresses();
        }
        tracing::info!(addresses = ?config.addresses, "announcing Spotify Connect devices");
        let connect = Self {
            inner: Arc::new(Inner {
                config,
                devices: devices.clone(),
                sources,
                running: Mutex::default(),
                seen: Mutex::default(),
                away: Mutex::default(),
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

    /// Checks player `id` again after `after`.
    fn recheck(&self, id: &str, after: Duration) {
        let (connect, id) = (self.clone(), id.to_owned());
        tokio::spawn(async move {
            tokio::time::sleep(after).await;
            connect.follow(&id);
        });
    }

    /// Starts, renames or stops the device for player `id` to match its settings. A speaker
    /// newly able to play is announced once it has been for [`SETTLE`]; one that went away
    /// keeps its device for [`GRACE`].
    fn follow(&self, id: &str) {
        let device = self.inner.devices.get(id);
        let available = device.as_ref().is_some_and(Device::available);
        let mut wanted = device
            .as_ref()
            .and_then(|device| wanted(device, &self.inner.config.names));
        let now = Instant::now();
        if available {
            self.inner.away.lock().unwrap().remove(id);
            let since = *self
                .inner
                .seen
                .lock()
                .unwrap()
                .entry(id.to_owned())
                .or_insert(now);
            let settling = SETTLE.saturating_sub(now - since);
            if !settling.is_zero() && !self.inner.running.lock().unwrap().contains_key(id) {
                self.recheck(id, settling);
                return;
            }
        } else {
            self.inner.seen.lock().unwrap().remove(id);
            if wanted.is_some() && self.inner.running.lock().unwrap().contains_key(id) {
                let since = *self
                    .inner
                    .away
                    .lock()
                    .unwrap()
                    .entry(id.to_owned())
                    .or_insert(now);
                let left = GRACE.saturating_sub(now - since);
                if !left.is_zero() {
                    self.recheck(id, left);
                    return;
                }
            }
            wanted = None;
        }
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
                self.inner.config.addresses.clone(),
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
/// app, that has not been turned off for Spotify Connect. Its name is its settings' Spotify
/// name, else `names`' pattern applied to its name.
fn wanted(device: &Device, names: &SpotifyConfig) -> Option<(String, u8)> {
    let primary = device.primary();
    (device.enabled && device.spotify_connect && !primary.state.app).then(|| {
        let name = device
            .spotify_name
            .clone()
            .unwrap_or_else(|| names.device_name(&device.name));
        (name, primary.state.volume.unwrap_or(50))
    })
}

/// The addresses of the network interface this host reaches the network from, the one its
/// default route leaves by: IPv4 and IPv6 alike, since phones ask over either. Empty without a
/// route, when every address is announced.
pub fn local_addresses() -> Vec<IpAddr> {
    let Some(local) = local_address() else {
        return Vec::new();
    };
    let Ok(interfaces) = nix::ifaddrs::getifaddrs() else {
        return vec![local];
    };
    let interfaces: Vec<_> = interfaces.collect();
    let address = |entry: &nix::ifaddrs::InterfaceAddress| -> Option<IpAddr> {
        let address = entry.address.as_ref()?;
        if let Some(v4) = address.as_sockaddr_in() {
            return Some(IpAddr::V4(v4.ip()));
        }
        address.as_sockaddr_in6().map(|v6| IpAddr::V6(v6.ip()))
    };
    let Some(interface) = interfaces
        .iter()
        .find(|entry| address(entry) == Some(local))
        .map(|entry| entry.interface_name.clone())
    else {
        return vec![local];
    };
    let mut addresses: Vec<IpAddr> = interfaces
        .iter()
        .filter(|entry| entry.interface_name == interface)
        .filter_map(address)
        .collect();
    addresses.dedup();
    addresses
}

/// The address this host reaches the network from: the one its default route leaves by. Found
/// by connecting a UDP socket, which sends nothing.
fn local_address() -> Option<IpAddr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    Some(socket.local_addr().ok()?.ip()).filter(|ip| !ip.is_unspecified())
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

    #[test]
    fn the_local_addresses_are_the_routes_interfaces() {
        let addresses = local_addresses();
        if let Some(local) = local_address() {
            assert!(addresses.contains(&local), "{addresses:?}");
        }
        for address in addresses {
            assert!(
                !address.is_unspecified() && !address.is_loopback(),
                "{address}"
            );
        }
    }
}
