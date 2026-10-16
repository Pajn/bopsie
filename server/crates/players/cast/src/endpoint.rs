//! A Cast device as a player: played through the Sendspin receiver launched on it, which
//! connects back to the server as a Sendspin client that this endpoint hosts.

use std::io;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bopsie_audio::OutputFormat;
use bopsie_audio::sink::Sink;
use bopsie_audio::timeline::{Micros, Timeline};
use bopsie_core::clock;
use bopsie_players::{
    Capabilities, Controls, DeviceInfo, Endpoint, EndpointState, Facts, NowPlaying, Pairing,
    Players,
};
use bopsie_players_sendspin::{HostedClient, SendspinEndpoint};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::discovery::Sighting;

/// The formats the receiver offers before it has said so itself.
const FORMATS: [OutputFormat; 2] = [
    OutputFormat {
        sample_rate: 48_000,
        channels: 2,
        bits: 16,
    },
    OutputFormat {
        sample_rate: 44_100,
        channels: 2,
        bits: 16,
    },
];
/// Its lead before it has said what it needs.
const LEAD: Micros = 300_000;
/// How long a receiver that has just connected is given for its clock to settle on ours.
const SETTLE: Micros = 1_000_000;

/// How the provider plays to Cast devices.
#[derive(Debug, Clone)]
pub struct Settings {
    /// The Cast app that is the Sendspin receiver.
    pub app_id: String,
    /// The port of the server's Sendspin listener for clients without a token.
    pub sendspin_port: u16,
    /// The address receivers reach it at, when the listener is bound to one; else the address
    /// the server reached the device from.
    pub sendspin_host: Option<IpAddr>,
    /// How long a receiver may sit idle before it is stopped, to free the device.
    pub release_after: Duration,
    /// How long a receiver has to launch and connect.
    pub launch_timeout: Duration,
}

/// The app running on a device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RunningApp {
    pub app_id: String,
    pub session_id: String,
    pub transport_id: String,
}

/// What the server knows of a device over its Cast channel.
#[derive(Debug, Default)]
pub(crate) struct Status {
    /// The channel is open.
    pub connected: bool,
    pub volume: Option<u8>,
    pub muted: Option<bool>,
    pub app: Option<RunningApp>,
    /// A play asked for the receiver, and it has not been released since.
    pub wanted: bool,
    /// The receiver was seen running since it was last asked for.
    pub seen: bool,
    /// Another app took the device from the receiver: it is not launched again until a play
    /// asks for it.
    pub taken: bool,
    /// The server stopped the receiver, and the device may still list it meanwhile.
    pub stopping: bool,
    /// The receiver's transport the server told where to connect.
    pub configured: Option<String>,
    /// Where the receiver is told to connect.
    pub server_url: Option<String>,
    /// The receiver's latest report of itself.
    pub report: Option<serde_json::Value>,
}

pub(crate) enum Command {
    /// Launches the receiver, or tells a running one where to connect.
    Launch,
    /// Stops the receiver, if it runs.
    Stop,
    Volume(u8),
    Muted(bool),
    Shutdown,
}

/// Shared by an endpoint and its channel.
pub(crate) struct Shared {
    pub id: String,
    pub sighting: Mutex<Sighting>,
    pub discovered: AtomicBool,
    pub status: Mutex<Status>,
    /// The receiver's Sendspin client, while it is connected or since it last was.
    pub client: HostedClient,
    pub commands: mpsc::UnboundedSender<Command>,
    pub settings: Settings,
    pub players: Players,
    pub runtime: tokio::runtime::Handle,
    /// Stops the receiver once it has been idle long enough.
    pub release: Mutex<Option<JoinHandle<()>>>,
}

impl Shared {
    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    /// The receiver's Sendspin client, if it is connected and ready to play.
    pub fn live(&self) -> Option<Arc<SendspinEndpoint>> {
        self.client
            .borrow()
            .clone()
            .filter(|client| client.state().available)
    }

    /// Stops the receiver after it has been idle for the settings' `release_after`, unless
    /// something plays before then.
    pub fn release_later(self: &Arc<Self>) {
        let shared = self.clone();
        let after = shared.settings.release_after;
        let task = self.runtime.spawn(async move {
            tokio::time::sleep(after).await;
            tracing::debug!(device = shared.id, "releasing an idle Cast receiver");
            shared.send(Command::Stop);
        });
        if let Some(old) = self.release.lock().unwrap().replace(task) {
            old.abort();
        }
    }

    fn keep(&self) {
        if let Some(task) = self.release.lock().unwrap().take() {
            task.abort();
        }
    }
}

pub struct CastEndpoint {
    pub(crate) shared: Arc<Shared>,
}

impl CastEndpoint {
    fn release_later(&self) {
        self.shared.release_later();
    }

    fn keep(&self) {
        self.shared.keep();
    }
}

impl Endpoint for CastEndpoint {
    fn id(&self) -> &str {
        &self.shared.id
    }

    fn state(&self) -> EndpointState {
        let live = self.shared.live();
        let sighting = self.shared.sighting.lock().unwrap().clone();
        let status = self.shared.status.lock().unwrap();
        let mut endpoint = live.as_ref().map_or_else(
            || EndpointState {
                name: String::new(),
                provider: "sendspin",
                available: false,
                volume: None,
                muted: None,
                device: DeviceInfo {
                    model: None,
                    manufacturer: None,
                    software_version: None,
                },
                app: false,
                owner_token: None,
                facts: Facts::default(),
                capabilities: Capabilities {
                    formats: FORMATS.to_vec(),
                    lead: LEAD,
                    sync_group: true,
                    max_group_size: None,
                    volume: true,
                    mute: true,
                    pairing: Pairing::None,
                    native_group: None,
                    launched: true,
                    withdrawn: Vec::new(),
                },
                quirks: Default::default(),
                native_leader: None,
            },
            |client| client.state(),
        );
        endpoint.name = sighting.name;
        endpoint.available =
            live.is_some() || (self.shared.discovered.load(Ordering::SeqCst) && status.connected);
        endpoint.volume = status.volume.or(endpoint.volume);
        endpoint.muted = status.muted.or(endpoint.muted);
        endpoint.device = DeviceInfo {
            model: sighting.model,
            manufacturer: None,
            software_version: None,
        };
        endpoint.app = false;
        endpoint.owner_token = None;
        endpoint.facts = Facts {
            address: Some(sighting.address),
            ..Facts::default()
        };
        endpoint.capabilities.volume = true;
        endpoint.capabilities.mute = true;
        endpoint.capabilities.launched = true;
        drop(status);
        endpoint.quirks =
            self.shared
                .players
                .resolve_quirks(&self.shared.id, &endpoint, &Default::default());
        endpoint
    }

    /// Plays through the receiver's client. Without one, as when it rejoins a group that
    /// already plays, the receiver is launched, unless another app took the device, and the
    /// sink plays through it once it connects, dropping what comes before.
    fn output(&self, format: OutputFormat, timeline: Timeline) -> Box<dyn Sink> {
        if let Some(client) = self.shared.live() {
            return client.output(format, timeline);
        }
        let mut status = self.shared.status.lock().unwrap();
        if status.taken {
            return Box::new(Absent(format));
        }
        // Wanted from now, so that the sink waits for it rather than giving up.
        let launch = !std::mem::replace(&mut status.wanted, true);
        drop(status);
        if launch {
            self.release_later();
            self.shared.send(Command::Launch);
        }
        Box::new(Pending {
            deadline: Instant::now() + self.shared.settings.launch_timeout,
            shared: self.shared.clone(),
            format,
            timeline,
            client: None,
        })
    }

    fn set_volume(&self, volume: u8) {
        self.shared.send(Command::Volume(volume));
        self.shared.status.lock().unwrap().volume = Some(volume);
        self.shared.players.changed(&self.shared.id);
    }

    fn set_muted(&self, muted: bool) {
        self.shared.send(Command::Muted(muted));
        self.shared.status.lock().unwrap().muted = Some(muted);
        self.shared.players.changed(&self.shared.id);
    }

    fn set_now_playing(&self, now_playing: Option<&NowPlaying>) {
        if let Some(client) = self.shared.live() {
            client.set_now_playing(now_playing);
        }
    }

    fn set_group(&self, playing: bool, group_id: &str) {
        if let Some(client) = self.shared.live() {
            client.set_group(playing, group_id);
        }
        if playing {
            self.keep();
        } else if self.shared.status.lock().unwrap().wanted {
            self.release_later();
        }
    }

    fn set_controls(&self, controls: &Controls) {
        if let Some(client) = self.shared.live() {
            client.set_controls(controls);
        }
    }

    /// Launches the receiver, and waits for it to connect.
    fn prepare(&self) -> Option<Micros> {
        self.shared.status.lock().unwrap().taken = false;
        self.keep();
        self.release_later();
        if self.shared.live().is_some() {
            return None;
        }
        self.shared.send(Command::Launch);
        let deadline = Instant::now() + self.shared.settings.launch_timeout;
        while Instant::now() < deadline {
            if let Some(client) = self.shared.live() {
                return Some(clock::now() + client.state().capabilities.lead + SETTLE);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        tracing::warn!(
            device = self.shared.id,
            "the Cast receiver did not connect in time"
        );
        None
    }

    fn diagnostics(&self) -> serde_json::Value {
        let sighting = self.shared.sighting.lock().unwrap().clone();
        let status = self.shared.status.lock().unwrap();
        serde_json::json!({
            "address": sighting.address.to_string(),
            "port": sighting.port,
            "discovered": self.shared.discovered.load(Ordering::SeqCst),
            "connected": status.connected,
            "app": status.app.as_ref().map(|app| serde_json::json!({
                "app_id": app.app_id,
                "session_id": app.session_id,
                "transport_id": app.transport_id,
            })),
            "receiver_app_id": self.shared.settings.app_id,
            "wanted": status.wanted,
            "server_url": status.server_url,
            "receiver": status.report,
            "client": self.shared.client.borrow().as_ref().map(|client| client.diagnostics()),
        })
    }
}

/// A sink for a receiver being launched: what it is given before the receiver connects is
/// dropped, as it would be too late by then anyway, and it plays through the receiver's client
/// from then on. It never waits, as the engine writes every output of the group in turn.
struct Pending {
    shared: Arc<Shared>,
    format: OutputFormat,
    timeline: Timeline,
    client: Option<Box<dyn Sink>>,
    /// Gives up once the receiver has had this long to connect.
    deadline: Instant,
}

impl Pending {
    /// The receiver's client, once it is connected; an error once it will not be.
    fn client(&mut self) -> io::Result<Option<&mut Box<dyn Sink>>> {
        if self.client.is_none() {
            if let Some(client) = self.shared.live() {
                self.client = Some(client.output(self.format, self.timeline));
            } else {
                let status = self.shared.status.lock().unwrap();
                let given_up = status.taken || !status.wanted;
                drop(status);
                if given_up || Instant::now() >= self.deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::NotConnected,
                        "the Cast receiver did not connect",
                    ));
                }
            }
        }
        Ok(self.client.as_mut())
    }
}

impl Sink for Pending {
    fn format(&self) -> OutputFormat {
        self.format
    }

    fn write(&mut self, at: u64, data: &[u8]) -> io::Result<()> {
        match self.client()? {
            Some(client) => client.write(at, data),
            None => Ok(()),
        }
    }

    fn clear(&mut self) -> io::Result<()> {
        self.client.as_mut().map_or(Ok(()), |client| client.clear())
    }

    fn stop(&mut self) -> io::Result<()> {
        self.client.as_mut().map_or(Ok(()), |client| client.stop())
    }

    fn drain(&mut self) -> io::Result<()> {
        self.client.as_mut().map_or(Ok(()), |client| client.drain())
    }

    fn finish(&mut self) -> io::Result<()> {
        self.client
            .as_mut()
            .map_or(Ok(()), |client| client.finish())
    }
}

/// A sink for a receiver that is not connected: it plays nothing, and says so.
struct Absent(OutputFormat);

impl Sink for Absent {
    fn format(&self) -> OutputFormat {
        self.0
    }

    fn write(&mut self, _at: u64, _data: &[u8]) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::NotConnected,
            "the Cast receiver is not connected",
        ))
    }

    fn finish(&mut self) -> io::Result<()> {
        Ok(())
    }
}
