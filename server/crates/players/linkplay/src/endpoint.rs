//! A LinkPlay device as the rest of the server sees it: its state, read by polling, since the
//! devices push nothing over HTTP; its volume; its own multiroom; and its output, a stream it
//! pulls from the server.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use bopsie_audio::OutputFormat;
use bopsie_audio::sink::Sink;
use bopsie_audio::timeline::{Micros, Timeline};
use bopsie_players::quirks::Quirks;
use bopsie_players::{
    Capabilities, DeviceInfo, Endpoint, EndpointState, Facts, NativeGroup, NowPlaying, Pairing,
    Players,
};
use tokio::runtime::Handle;

use crate::discovery::{self, Sighting};
use crate::status::{PlayerStatus, StatusEx, Transport};
use crate::stream::{LinkplaySink, StreamHub, Timing};
use crate::transport::{Api, ApiError};
use crate::upnp::Upnp;

/// The formats a device takes: CD and DAT quality, the most a native group carries.
pub const FORMATS: &[OutputFormat] = &[
    OutputFormat {
        sample_rate: 44_100,
        channels: 2,
        bits: 16,
    },
    OutputFormat {
        sample_rate: 48_000,
        channels: 2,
        bits: 16,
    },
];
/// How far ahead playback starts: room for the device's own start delay and a margin.
pub const LEAD: Micros = 3_500_000;
/// How often an idle device is read, and a playing one.
const IDLE_POLL: Duration = Duration::from_secs(10);
const PLAYING_POLL: Duration = Duration::from_secs(1);
/// How long a change of group is given to show on the devices.
const SETTLE: Duration = Duration::from_secs(5);

/// Where devices reach the server for streams.
#[derive(Debug, Clone)]
pub struct ServerAddress {
    /// The API's port.
    pub port: u16,
    /// The URL to use instead of the address the server reaches each device from.
    pub base_url: Option<String>,
}

pub(crate) type Directory = Mutex<HashMap<String, Arc<LinkplayEndpoint>>>;

/// What every device's endpoint shares with the provider.
#[derive(Clone)]
pub(crate) struct Context {
    pub players: Players,
    pub hub: StreamHub,
    pub server: ServerAddress,
    pub directory: Weak<Directory>,
}

#[derive(Debug, Default)]
struct State {
    /// From mDNS, until a status names it.
    announced_name: String,
    address: Option<IpAddr>,
    mac: Option<String>,
    uuid: Option<String>,
    discovered: bool,
    reachable: bool,
    status: Option<StatusEx>,
    volume: Option<u8>,
    muted: Option<bool>,
    playing: bool,
    /// Whether `getPlayerStatus` answers, once known.
    player_status: Option<bool>,
}

pub(crate) struct Shared {
    pub id: String,
    pub api: Api,
    pub upnp: Upnp,
    pub hub: StreamHub,
    pub runtime: Handle,
    pub timing: Timing,
    server: ServerAddress,
    state: Mutex<State>,
    players: Players,
    directory: Weak<Directory>,
    stopping: AtomicBool,
}

impl Shared {
    /// The URL the device pulls stream `token` from.
    pub fn stream_url(&self, token: &str) -> Option<String> {
        let base = match &self.server.base_url {
            Some(base) => base.trim_end_matches('/').to_owned(),
            None => {
                let device = self.state.lock().unwrap().address?;
                let socket = UdpSocket::bind(match device {
                    IpAddr::V4(_) => "0.0.0.0:0",
                    IpAddr::V6(_) => "[::]:0",
                })
                .ok()?;
                socket.connect(SocketAddr::new(device, 80)).ok()?;
                let ours = socket.local_addr().ok()?.ip();
                match ours {
                    IpAddr::V4(ip) => format!("http://{ip}:{}", self.server.port),
                    IpAddr::V6(ip) => format!("http://[{ip}]:{}", self.server.port),
                }
            }
        };
        Some(format!("{base}/linkplay/stream/{token}.wav"))
    }

    pub fn has_player_status(&self) -> bool {
        self.state.lock().unwrap().player_status != Some(false)
    }

    /// Transport, position, volume and mute: from `getPlayerStatus`, or over UPnP where the
    /// firmware lacks it.
    pub async fn player_status(&self) -> Result<PlayerStatus, ApiError> {
        if self.has_player_status() {
            match self.api.command("getPlayerStatus").await {
                Ok(json) => {
                    self.state.lock().unwrap().player_status = Some(true);
                    return PlayerStatus::parse(&json)
                        .ok_or_else(|| ApiError::Request("unreadable player status".into()));
                }
                Err(ApiError::UnknownCommand(_)) => {
                    self.state.lock().unwrap().player_status = Some(false);
                }
                Err(error) => return Err(error),
            }
        }
        Ok(PlayerStatus {
            transport: self.upnp.transport().await?,
            position_ms: self.upnp.position_ms().await.ok().flatten(),
            volume: self.upnp.volume().await.ok().flatten(),
            muted: self.upnp.muted().await.ok().flatten(),
            mode: None,
            title: None,
        })
    }

    fn snapshot(&self, state: &State) -> EndpointState {
        let status = state.status.as_ref();
        let name = status
            .map(|s| s.name.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| state.announced_name.clone());
        let mut endpoint = EndpointState {
            name,
            provider: "linkplay",
            available: state.discovered && state.reachable,
            volume: state.volume,
            muted: state.muted,
            device: DeviceInfo {
                model: status.and_then(|s| s.project.clone()),
                manufacturer: None,
                software_version: status.and_then(|s| s.firmware.clone()),
            },
            app: false,
            owner_token: None,
            facts: Facts {
                mac: state
                    .mac
                    .as_ref()
                    .map(|m| m.replace([':', '-'], "").to_lowercase()),
                uuid: state.uuid.clone(),
                address: state.address,
                ..Facts::default()
            },
            capabilities: Capabilities {
                formats: FORMATS.to_vec(),
                lead: LEAD,
                // The device starts when it has buffered, not at an instant it is given.
                sync_group: false,
                max_group_size: None,
                volume: true,
                mute: true,
                pairing: Pairing::None,
                native_group: status
                    .and_then(|s| s.multiroom_major())
                    .map(|version| NativeGroup {
                        family: "linkplay",
                        version,
                    }),
            },
            quirks: Default::default(),
            native_leader: status
                .filter(|s| s.follower)
                .and_then(|s| s.master_uuid.as_deref())
                .map(discovery::id_of_uuid),
        };
        endpoint.quirks = self
            .players
            .resolve_quirks(&self.id, &endpoint, &Quirks::default());
        endpoint
    }

    fn changed(&self) {
        self.players.changed(&self.id);
    }

    /// Reads the device afresh. Returns whether anything changed.
    async fn refresh(&self) -> bool {
        let before = self.snapshot(&self.state.lock().unwrap());
        match self.api.command("getStatusEx").await {
            Ok(json) => {
                let status = StatusEx::parse(&json);
                let mut state = self.state.lock().unwrap();
                state.reachable = status.is_some();
                if let Some(status) = status {
                    state.uuid = state.uuid.clone().or(status.uuid.clone());
                    state.mac = state.mac.clone().or(status.mac.clone());
                    state.status = Some(status);
                }
            }
            Err(error) => {
                tracing::debug!(device = self.id, %error, "cannot read the device");
                self.state.lock().unwrap().reachable = false;
            }
        }
        if self.state.lock().unwrap().reachable
            && let Ok(player) = self.player_status().await
        {
            let mut state = self.state.lock().unwrap();
            state.volume = player.volume.or(state.volume);
            state.muted = player.muted.or(state.muted);
            state.playing = player.transport == Some(Transport::Playing);
        }
        let after = self.snapshot(&self.state.lock().unwrap());
        after != before
    }

    /// The endpoint with player id `id`, if this provider has it.
    fn other(&self, id: &str) -> Option<Arc<LinkplayEndpoint>> {
        self.directory.upgrade()?.lock().unwrap().get(id).cloned()
    }

    fn address(&self) -> Option<IpAddr> {
        self.state.lock().unwrap().address
    }

    fn block_on<T>(&self, future: impl Future<Output = T>) -> T {
        self.runtime.block_on(future)
    }

    /// Polls until `holds` is true of the device's status, or [`SETTLE`] passes.
    async fn settle(&self, holds: impl Fn(&StatusEx) -> bool) -> bool {
        let deadline = Instant::now() + SETTLE;
        while Instant::now() < deadline {
            if let Ok(json) = self.api.command("getStatusEx").await
                && let Some(status) = StatusEx::parse(&json)
            {
                let done = holds(&status);
                self.state.lock().unwrap().status = Some(status);
                if done {
                    return true;
                }
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        false
    }
}

pub struct LinkplayEndpoint {
    shared: Arc<Shared>,
}

impl LinkplayEndpoint {
    /// A device found as `sighting`, reached through `api` and `upnp`. Reads it in the
    /// background from now on. Needs a Tokio runtime.
    pub(crate) fn new(
        sighting: &Sighting,
        id: String,
        api: Api,
        upnp: Upnp,
        context: Context,
    ) -> Arc<Self> {
        let Context {
            players,
            hub,
            server,
            directory,
        } = context;
        let shared = Arc::new(Shared {
            id,
            api,
            upnp,
            hub,
            runtime: Handle::current(),
            timing: Timing::default(),
            server,
            state: Mutex::new(State {
                announced_name: sighting.name.clone(),
                address: Some(sighting.address),
                mac: sighting.mac.clone(),
                uuid: sighting.uuid.clone(),
                discovered: true,
                ..State::default()
            }),
            players,
            directory,
            stopping: AtomicBool::new(false),
        });
        let polled = Arc::downgrade(&shared);
        tokio::spawn(async move {
            loop {
                let Some(shared) = polled.upgrade() else {
                    return;
                };
                if shared.stopping.load(Ordering::SeqCst) {
                    return;
                }
                if shared.refresh().await {
                    shared.changed();
                }
                let wait = if shared.state.lock().unwrap().playing {
                    PLAYING_POLL
                } else {
                    IDLE_POLL
                };
                drop(shared);
                tokio::time::sleep(wait).await;
            }
        });
        Arc::new(Self { shared })
    }

    /// Discovery found the device again, perhaps at a new address.
    pub fn rediscovered(&self, sighting: &Sighting) {
        let mut state = self.shared.state.lock().unwrap();
        state.discovered = true;
        state.address = Some(sighting.address);
        state.announced_name = sighting.name.clone();
        drop(state);
        self.shared.changed();
    }

    /// The device left the network.
    pub fn lost(&self) {
        self.shared.state.lock().unwrap().discovered = false;
        self.shared.changed();
    }

    /// Stops reading the device.
    pub fn shutdown(&self) {
        self.shared.stopping.store(true, Ordering::SeqCst);
    }

    /// Reads the device now, rather than at the next poll.
    pub async fn refresh(&self) {
        if self.shared.refresh().await {
            self.shared.changed();
        }
    }

    /// How long the device is expected to take from `play` to audible, in milliseconds, as
    /// its last start measured.
    pub fn start_delay_ms(&self) -> i64 {
        self.shared.timing.delay_ms.load(Ordering::SeqCst)
    }

    pub fn status(&self) -> Option<StatusEx> {
        self.shared.state.lock().unwrap().status.clone()
    }

    fn command(&self, command: String) {
        let shared = self.shared.clone();
        self.shared.runtime.spawn(async move {
            if let Err(error) = shared.api.command(&command).await {
                tracing::warn!(device = shared.id, command, %error, "the device refused");
            }
        });
    }
}

impl Endpoint for LinkplayEndpoint {
    fn id(&self) -> &str {
        &self.shared.id
    }

    fn state(&self) -> EndpointState {
        let state = self.shared.state.lock().unwrap();
        self.shared.snapshot(&state)
    }

    fn output(&self, format: OutputFormat, timeline: Timeline) -> Box<dyn Sink> {
        Box::new(LinkplaySink::new(self.shared.clone(), format, timeline))
    }

    fn set_volume(&self, volume: u8) {
        let volume = volume.min(100);
        self.shared.state.lock().unwrap().volume = Some(volume);
        self.command(format!("setPlayerCmd:vol:{volume}"));
        self.shared.changed();
    }

    fn set_muted(&self, muted: bool) {
        self.shared.state.lock().unwrap().muted = Some(muted);
        self.command(format!("setPlayerCmd:mute:{}", u8::from(muted)));
        self.shared.changed();
    }

    fn set_now_playing(&self, _now_playing: Option<&NowPlaying>) {
        // The device shows what the stream says of itself.
    }

    fn set_group(&self, _playing: bool, _group_id: &str) {}

    fn join_native(&self, leader: &str) -> Result<(), String> {
        let leading = self
            .shared
            .other(leader)
            .ok_or_else(|| format!("{leader} is not a LinkPlay device here"))?;
        let address = leading
            .shared
            .address()
            .ok_or_else(|| format!("{leader} has no address"))?;
        let uuid = leading.status().and_then(|s| s.uuid);
        let shared = &self.shared;
        shared.block_on(async {
            shared
                .api
                .command(&format!(
                    "ConnectMasterAp:JoinGroupMaster:eth{address}:wifi0.0.0.0"
                ))
                .await
                .map_err(|e| e.to_string())?;
            let joined = shared
                .settle(|s| {
                    s.follower
                        && match &uuid {
                            Some(uuid) => s.master_uuid.as_deref() == Some(uuid.as_str()),
                            None => s.master_ip.as_deref() == Some(address.to_string().as_str()),
                        }
                })
                .await;
            if joined {
                Ok(())
            } else {
                Err("the device did not join".to_owned())
            }
        })?;
        shared.changed();
        leading.shared.changed();
        Ok(())
    }

    fn leave_native(&self) -> Result<(), String> {
        let shared = &self.shared;
        let status = self.status();
        let following = status
            .as_ref()
            .filter(|s| s.follower)
            .and_then(|s| s.master_uuid.as_deref().map(discovery::id_of_uuid))
            .and_then(|leader| shared.other(&leader));
        let address = shared.address();
        shared.block_on(async {
            match (&following, address) {
                // Removed by its leader, which works whatever the follower's firmware.
                (Some(leader), Some(address)) => leader
                    .shared
                    .api
                    .command(&format!("multiroom:SlaveKickout:{address}"))
                    .await
                    .map_err(|e| e.to_string())?,
                _ => shared
                    .api
                    .command("multiroom:Ungroup")
                    .await
                    .map_err(|e| e.to_string())?,
            };
            if shared.settle(|s| !s.follower).await {
                Ok(())
            } else {
                Err("the device did not leave its group".to_owned())
            }
        })?;
        shared.changed();
        if let Some(leader) = following {
            leader.shared.changed();
        }
        Ok(())
    }
}
