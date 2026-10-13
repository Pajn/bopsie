//! One Sendspin client: the handshake, its state, time replies, and the endpoint that streams
//! to it.

use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket};
use bopsie_audio::OutputFormat;
use bopsie_audio::sink::Sink;
use bopsie_audio::timeline::{Micros, Timeline};
use bopsie_core::clock;
use bopsie_players::{DeviceInfo, Endpoint, EndpointState, NowPlaying, Players};
use futures::{SinkExt, StreamExt};
use tokio::runtime::Handle;
use tokio::sync::mpsc;

use crate::sink::SendspinSink;
use crate::wire::{self, ClientHello, ClientMessage, Codec, Metadata, Progress};

/// How long a client has to say hello.
const HELLO_TIMEOUT: Duration = Duration::from_secs(10);
/// The least lead the server gives any client, whatever it asks for.
const MIN_LEAD_MS: u32 = 150;
/// The roles this server takes on with a client, of those it supports.
const ROLES: &[&str] = &["player@v1", "metadata@v1"];

pub(crate) enum Outgoing {
    Text(String),
    Binary(Vec<u8>),
    /// A time reply, stamped as it leaves.
    Time {
        client_transmitted: i64,
        received: Micros,
    },
}

/// What the server knows of a client, from its hello and its latest state.
#[derive(Debug, Clone)]
pub(crate) struct ClientInfo {
    pub name: String,
    pub hello: ClientHello,
    pub connected: bool,
    pub available: bool,
    pub volume: Option<u8>,
    pub muted: Option<bool>,
    pub static_delay_ms: u32,
    pub required_lead_time_ms: u32,
    pub min_buffer_ms: u32,
    pub owner_token: Option<String>,
}

impl ClientInfo {
    pub fn buffer_capacity(&self) -> u64 {
        self.hello
            .player_support()
            .map_or(1_048_576, |support| support.buffer_capacity)
    }

    pub fn lead(&self) -> Micros {
        let ms = self
            .required_lead_time_ms
            .max(self.min_buffer_ms)
            .max(MIN_LEAD_MS);
        Micros::from(ms) * 1_000
    }
}

/// Shared by a client's connection, its endpoint and its sinks.
pub(crate) struct Shared {
    pub id: String,
    pub out: mpsc::UnboundedSender<Outgoing>,
    pub info: Mutex<ClientInfo>,
    /// Counts streams started, so a delayed `stream/end` does not end a newer stream.
    pub generation: AtomicU64,
    pub runtime: Handle,
    players: Players,
}

impl Shared {
    pub fn send(&self, text: String) {
        let _ = self.out.send(Outgoing::Text(text));
    }
}

pub struct SendspinEndpoint {
    shared: Arc<Shared>,
}

impl Endpoint for SendspinEndpoint {
    fn id(&self) -> &str {
        &self.shared.id
    }

    fn state(&self) -> EndpointState {
        let info = self.shared.info.lock().unwrap();
        let formats = info
            .hello
            .player_support()
            .map(|support| {
                support
                    .supported_formats
                    .iter()
                    .filter(|f| f.codec == Codec::Pcm && matches!(f.bit_depth, 16 | 24 | 32))
                    .map(|f| OutputFormat {
                        sample_rate: f.sample_rate,
                        channels: f.channels,
                        bits: f.bit_depth,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let device = info.hello.device_info.as_ref();
        EndpointState {
            name: info.name.clone(),
            provider: "sendspin",
            available: info.connected && info.available,
            volume: info.volume,
            muted: info.muted,
            device: DeviceInfo {
                model: device.and_then(|d| d.product_name.clone()),
                manufacturer: device.and_then(|d| d.manufacturer.clone()),
                software_version: device.and_then(|d| d.software_version.clone()),
            },
            app: info.hello.is_app(),
            owner_token: info.owner_token.clone(),
            formats,
            lead: info.lead(),
        }
    }

    fn output(&self, format: OutputFormat, timeline: Timeline) -> Box<dyn Sink> {
        Box::new(SendspinSink::new(self.shared.clone(), format, timeline))
    }

    fn set_volume(&self, volume: u8) {
        self.shared.send(wire::volume_command(volume));
        self.shared.info.lock().unwrap().volume = Some(volume);
        self.shared.players.changed(&self.shared.id);
    }

    fn set_muted(&self, muted: bool) {
        self.shared.send(wire::mute_command(muted));
        self.shared.info.lock().unwrap().muted = Some(muted);
        self.shared.players.changed(&self.shared.id);
    }

    fn set_now_playing(&self, now_playing: Option<&NowPlaying>) {
        let metadata = match now_playing {
            Some(now) => Metadata {
                timestamp: clock::now(),
                title: Some(now.title.clone()),
                artist: now.artist.clone(),
                album_artist: now.album_artist.clone(),
                album: now.album.clone(),
                artwork_url: now.artwork_url.clone(),
                year: now.year,
                track: now.track_number,
                progress: Some(Progress {
                    track_progress: now.position,
                    track_duration: now.duration,
                    playback_speed: if now.playing { 1000 } else { 0 },
                }),
                repeat: None,
                shuffle: None,
            },
            None => Metadata {
                timestamp: clock::now(),
                ..Metadata::default()
            },
        };
        self.shared.send(wire::server_state_metadata(&metadata));
    }

    fn set_group(&self, playing: bool, group_id: &str) {
        self.shared.send(wire::group_update(playing, group_id));
    }
}

/// The Sendspin server role: answers clients and registers them as players.
#[derive(Clone)]
pub struct SendspinServer {
    pub players: Players,
    pub server_id: String,
    pub name: String,
}

impl SendspinServer {
    /// Speaks Sendspin on `socket` until the client leaves. `owner_token` is the token the
    /// client authenticated with on the API's `/sendspin`, if it did.
    pub async fn serve(&self, socket: WebSocket, owner_token: Option<String>) {
        let (mut sink, mut stream) = socket.split();
        let (out, mut outbox) = mpsc::unbounded_channel::<Outgoing>();
        let writer = tokio::spawn(async move {
            while let Some(outgoing) = outbox.recv().await {
                let message = match outgoing {
                    Outgoing::Text(text) => Message::Text(text.into()),
                    Outgoing::Binary(bytes) => Message::Binary(bytes.into()),
                    Outgoing::Time {
                        client_transmitted,
                        received,
                    } => Message::Text(
                        wire::server_time(client_transmitted, received, clock::now()).into(),
                    ),
                };
                if sink.send(message).await.is_err() {
                    break;
                }
            }
            let _ = sink.close().await;
        });

        let hello = match tokio::time::timeout(HELLO_TIMEOUT, next_hello(&mut stream)).await {
            Ok(Some(hello)) => hello,
            _ => {
                tracing::debug!("a Sendspin client left without a hello");
                drop(out);
                let _ = writer.await;
                return;
            }
        };
        let Some(id) = hello.client_id.clone() else {
            tracing::debug!("a Sendspin client said hello without a client_id");
            return;
        };
        let roles: Vec<&str> = ROLES
            .iter()
            .copied()
            .filter(|role| hello.supported_roles.iter().any(|r| r == role))
            .collect();
        let _ = out.send(Outgoing::Text(wire::server_hello(
            &self.server_id,
            &self.name,
            &roles,
        )));

        let shared = Arc::new(Shared {
            id: id.clone(),
            out,
            info: Mutex::new(ClientInfo {
                name: hello.name.clone(),
                hello: hello.clone(),
                connected: true,
                available: false,
                volume: None,
                muted: None,
                static_delay_ms: 0,
                required_lead_time_ms: 0,
                min_buffer_ms: 0,
                owner_token,
            }),
            generation: AtomicU64::new(0),
            runtime: Handle::current(),
            players: self.players.clone(),
        });
        let mut registered = false;
        tracing::info!(client = id, name = hello.name, "Sendspin client connected");

        while let Some(Ok(message)) = stream.next().await {
            let received = clock::now();
            let Message::Text(text) = message else {
                if matches!(message, Message::Close(_)) {
                    break;
                }
                continue;
            };
            let message = match wire::parse(&text) {
                Ok(message) => message,
                Err(error) => {
                    tracing::debug!(client = id, %error, "ignoring a message");
                    continue;
                }
            };
            match message {
                ClientMessage::Time { client_transmitted } => {
                    let _ = shared.out.send(Outgoing::Time {
                        client_transmitted,
                        received,
                    });
                }
                ClientMessage::State(state) => {
                    {
                        let mut info = shared.info.lock().unwrap();
                        info.available = state.is_available();
                        if let Some(player) = state.player {
                            info.volume = player.volume.or(info.volume);
                            info.muted = player.muted.or(info.muted);
                            info.static_delay_ms =
                                player.static_delay_ms.unwrap_or(info.static_delay_ms);
                            info.required_lead_time_ms = player
                                .required_lead_time_ms
                                .unwrap_or(info.required_lead_time_ms);
                            info.min_buffer_ms = player.min_buffer_ms.unwrap_or(info.min_buffer_ms);
                        }
                    }
                    if registered {
                        self.players.changed(&id);
                    } else {
                        registered = true;
                        self.players.register(Arc::new(SendspinEndpoint {
                            shared: shared.clone(),
                        }));
                    }
                }
                ClientMessage::Goodbye { reason } => {
                    tracing::info!(client = id, reason, "Sendspin client said goodbye");
                    break;
                }
                ClientMessage::Hello(_) | ClientMessage::Command(_) | ClientMessage::Other(_) => {}
            }
        }

        shared.info.lock().unwrap().connected = false;
        if registered {
            self.players.changed(&id);
        }
        tracing::info!(client = id, "Sendspin client disconnected");
        writer.abort();
    }
}

async fn next_hello(stream: &mut futures::stream::SplitStream<WebSocket>) -> Option<ClientHello> {
    while let Some(Ok(message)) = stream.next().await {
        if let Message::Text(text) = message
            && let Ok(ClientMessage::Hello(hello)) = wire::parse(&text)
        {
            return Some(*hello);
        }
    }
    None
}
