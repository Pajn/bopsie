//! A Sendspin player connected to a Bopsie server, reconnecting until it is stopped.
//!
//! The player runs on a thread of its own with a single-threaded runtime, since the audio output
//! is not `Send` on every platform. Callers hold a [`Player`]: they send it commands and watch its
//! [`Snapshot`], from any thread and without a runtime of their own.

use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt};
use sendspin::ProtocolClientBuilder;
use sendspin::protocol::client::{Connection, Controller, WsSender};
use sendspin::protocol::messages::{
    ClientState, ControllerState, GoodbyeReason, Message, MetadataState, PlaybackState,
    PlayerCommand, PlayerCommandType, PlayerState, PlayerStateCommand, PlayerV1Support, RepeatMode,
};
use serde_json::json;
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

use crate::Error;
use crate::address::socket_url;
use crate::output::{Output, supported_formats};

/// The `device_info.product_name` the server registers as a player of its own, with a queue,
/// that can be played to.
pub const PRODUCT_NAME: &str = "Desktop Application";

/// The most not-yet-played audio, header and payload, the player accepts: about five seconds of
/// 48 kHz 24-bit stereo PCM.
const BUFFER_CAPACITY: u32 = 1_048_576;
/// How far ahead of its play time the player needs a chunk.
const LEAD_TIME_MS: u32 = 300;
/// The close code the server's `/sendspin` route uses for every authentication failure.
const AUTH_FAILED: u16 = 4001;

#[derive(Debug, Clone)]
pub struct PlayerConfig {
    /// A normalized server address, as [`crate::sign_in`] returns it.
    pub server_url: String,
    pub token: String,
    /// Identifies this player across connections; the server uses it as the player and queue
    /// id. Generate it once and keep it.
    pub client_id: String,
    /// The player's name as the server shows it.
    pub name: String,
    pub product_name: String,
    pub software_version: Option<String>,
    pub volume: u8,
    pub muted: bool,
}

/// The player's state as it last changed.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub connection: ConnectionState,
    /// Whether the player's group is playing.
    pub playing: bool,
    pub now_playing: Option<NowPlaying>,
    /// The group's controls, once the server has sent them.
    pub controls: Option<Controls>,
    /// This player's own volume, 0 to 100.
    pub volume: u8,
    pub muted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    Connecting,
    Connected,
    /// The connection dropped or could not be made; the next try is scheduled.
    Reconnecting {
        attempt: u32,
        error: String,
    },
    /// The server rejected the token. Nothing more happens until a new sign-in.
    Unauthorized,
    Stopped,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NowPlaying {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album_artist: Option<String>,
    pub album: Option<String>,
    pub artwork_url: Option<String>,
    pub year: Option<u32>,
    pub track: Option<u32>,
    pub progress: Option<Progress>,
}

/// Where the track was when the server last said, and how fast it moves from there.
#[derive(Debug, Clone, PartialEq)]
pub struct Progress {
    pub position_ms: u64,
    pub duration_ms: u64,
    /// 1.0 at normal speed, 0.0 when paused.
    pub speed: f64,
    /// When the position was received, in milliseconds since the Unix epoch.
    pub received_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Controls {
    pub volume: u8,
    pub muted: bool,
    pub repeat: Repeat,
    pub shuffle: bool,
    /// The track's length, when the server allows seeking in it.
    pub seek_max_ms: Option<u64>,
    /// The controller commands the server accepts now.
    pub supported_commands: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Repeat {
    Off,
    One,
    All,
}

/// A command for the player's group, sent through the controller role.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Remote {
    Play,
    Pause,
    Stop,
    Next,
    Previous,
    Volume(u8),
    Mute(bool),
    Repeat(Repeat),
    Shuffle(bool),
    Seek { position_ms: u64 },
}

enum Command {
    Remote(Remote),
    SetVolume(u8),
    SetMuted(bool),
    Stop,
}

/// A running player. Dropping it stops the player without waiting.
pub struct Player {
    commands: mpsc::UnboundedSender<Command>,
    state: watch::Receiver<Snapshot>,
    thread: Option<JoinHandle<()>>,
}

impl Player {
    /// Starts the player on a thread of its own. It connects at once.
    pub fn start(config: PlayerConfig) -> Result<Self, Error> {
        let (commands, receiver) = mpsc::unbounded_channel();
        let (state, watcher) = watch::channel(Snapshot {
            connection: ConnectionState::Connecting,
            playing: false,
            now_playing: None,
            controls: None,
            volume: config.volume.min(100),
            muted: config.muted,
        });
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| Error::Runtime(e.to_string()))?;
        let thread = std::thread::Builder::new()
            .name("bopsie-player".to_owned())
            .spawn(move || runtime.block_on(run(config, receiver, state)))
            .map_err(|e| Error::Runtime(e.to_string()))?;
        Ok(Self {
            commands,
            state: watcher,
            thread: Some(thread),
        })
    }

    pub fn snapshot(&self) -> Snapshot {
        self.state.borrow().clone()
    }

    /// A receiver that sees every change to the player's state.
    pub fn watch(&self) -> watch::Receiver<Snapshot> {
        self.state.clone()
    }

    /// Sends a command to the group. Ignored while the player is not connected, or when the server
    /// does not offer it.
    pub fn remote(&self, remote: Remote) {
        let _ = self.commands.send(Command::Remote(remote));
    }

    /// Sets this player's own volume, 0 to 100, and tells the server.
    pub fn set_volume(&self, volume: u8) {
        let _ = self.commands.send(Command::SetVolume(volume.min(100)));
    }

    pub fn set_muted(&self, muted: bool) {
        let _ = self.commands.send(Command::SetMuted(muted));
    }

    /// Says goodbye to the server and waits for the player to finish.
    pub fn stop(mut self) {
        let _ = self.commands.send(Command::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Stop);
    }
}

/// How a session ended.
enum Ended {
    Stopped,
    Lost(String),
}

async fn run(
    config: PlayerConfig,
    mut commands: mpsc::UnboundedReceiver<Command>,
    state: watch::Sender<Snapshot>,
) {
    let mut attempt = 0;
    loop {
        let levels = {
            let snapshot = state.borrow();
            (snapshot.volume, snapshot.muted)
        };
        let ended = match connect(&config, levels).await {
            Ok(connection) => {
                attempt = 0;
                state.send_modify(|s| s.connection = ConnectionState::Connected);
                session(connection, &mut commands, &state).await
            }
            Err(Error::Unauthorized) => {
                state.send_modify(|s| {
                    s.connection = ConnectionState::Unauthorized;
                    s.playing = false;
                    s.now_playing = None;
                    s.controls = None;
                });
                while let Some(command) = commands.recv().await {
                    if !apply_offline(command, &state) {
                        break;
                    }
                }
                break;
            }
            Err(error) => Ended::Lost(error.to_string()),
        };
        let Ended::Lost(error) = ended else {
            break;
        };
        attempt += 1;
        tracing::warn!(attempt, error, "the connection to the server was lost");
        state.send_modify(|s| {
            s.connection = ConnectionState::Reconnecting { attempt, error };
            s.playing = false;
        });
        let retry = tokio::time::sleep(reconnect_delay(attempt - 1));
        tokio::pin!(retry);
        let stopped = loop {
            tokio::select! {
                () = &mut retry => break false,
                command = commands.recv() => {
                    if !command.is_some_and(|command| apply_offline(command, &state)) {
                        break true;
                    }
                }
            }
        };
        if stopped {
            break;
        }
    }
    state.send_modify(|s| {
        s.connection = ConnectionState::Stopped;
        s.playing = false;
    });
}

/// Backoff before reconnection attempt `attempt` (0-based): 1, 2, 4, 8, 16, then 30 seconds, as
/// the app's API client waits.
fn reconnect_delay(attempt: u32) -> Duration {
    Duration::from_secs(2u64.saturating_pow(attempt).min(30))
}

/// Applies a command while there is no connection. False when the player should stop.
fn apply_offline(command: Command, state: &watch::Sender<Snapshot>) -> bool {
    match command {
        Command::Stop => return false,
        Command::Remote(_) => {}
        Command::SetVolume(volume) => state.send_modify(|s| s.volume = volume),
        Command::SetMuted(muted) => state.send_modify(|s| s.muted = muted),
    }
    true
}

/// Opens `/sendspin`, authenticates with the token, and completes the Sendspin handshake.
async fn connect(config: &PlayerConfig, (volume, muted): (u8, bool)) -> Result<Connection, Error> {
    let url = socket_url(&config.server_url, "/sendspin")?;
    let (mut socket, _) = connect_async(url)
        .await
        .map_err(|e| Error::Connection(e.to_string()))?;
    let auth = json!({ "type": "auth", "token": config.token }).to_string();
    socket
        .send(WsMessage::Text(auth.into()))
        .await
        .map_err(|e| Error::Connection(e.to_string()))?;
    match socket.next().await {
        Some(Ok(WsMessage::Text(text)))
            if serde_json::from_str::<serde_json::Value>(&text)
                .is_ok_and(|reply| reply["type"] == "auth_ok") => {}
        Some(Ok(WsMessage::Close(Some(frame)))) if frame.code == CloseCode::from(AUTH_FAILED) => {
            return Err(Error::Unauthorized);
        }
        Some(Ok(other)) => {
            return Err(Error::Protocol(format!("expected auth_ok, got {other:?}")));
        }
        Some(Err(e)) => return Err(Error::Connection(e.to_string())),
        None => return Err(Error::Connection("closed during authentication".to_owned())),
    }

    let client = ProtocolClientBuilder::builder()
        .client_id(config.client_id.clone())
        .name(config.name.clone())
        .product_name(Some(config.product_name.clone()))
        .manufacturer(Some("Bopsie".to_owned()))
        .software_version(config.software_version.clone())
        .player_v1_support(PlayerV1Support {
            supported_formats: supported_formats(),
            buffer_capacity: BUFFER_CAPACITY,
            supported_commands: vec!["volume".to_owned(), "mute".to_owned()],
        })
        .initial_player_state(PlayerState {
            volume: Some(volume),
            muted: Some(muted),
            static_delay_ms: Some(0),
            required_lead_time_ms: Some(LEAD_TIME_MS),
            min_buffer_ms: Some(LEAD_TIME_MS),
            supported_commands: Some(vec![PlayerStateCommand::SetStaticDelay]),
        })
        .metadata()
        .controller()
        .build()
        .accept(socket)
        .await
        .map_err(|e| Error::Connection(e.to_string()))?;
    Ok(client.split())
}

async fn session(
    connection: Connection,
    commands: &mut mpsc::UnboundedReceiver<Command>,
    state: &watch::Sender<Snapshot>,
) -> Ended {
    let Connection {
        mut messages,
        mut audio,
        clock_sync,
        sender,
        controller,
        guard,
        ..
    } = connection;
    let (volume, muted) = {
        let snapshot = state.borrow();
        (snapshot.volume, snapshot.muted)
    };
    let mut output = Output::new(clock_sync, volume, muted);
    loop {
        tokio::select! {
            message = messages.recv() => match message {
                Some(message) => handle_message(message, &mut output, &sender, state).await,
                None => return Ended::Lost("the server closed the connection".to_owned()),
            },
            chunk = audio.recv() => match chunk {
                Some(chunk) => output.push(chunk),
                None => return Ended::Lost("the server closed the connection".to_owned()),
            },
            command = commands.recv() => match command {
                Some(Command::Remote(remote)) => {
                    if let Some(controller) = &controller
                        && let Err(error) = send_remote(controller, remote).await
                    {
                        tracing::warn!(%error, ?remote, "could not send a command");
                    }
                }
                Some(Command::SetVolume(volume)) => {
                    output.set_volume(volume);
                    state.send_modify(|s| s.volume = volume);
                    report_levels(&sender, state).await;
                }
                Some(Command::SetMuted(muted)) => {
                    output.set_muted(muted);
                    state.send_modify(|s| s.muted = muted);
                    report_levels(&sender, state).await;
                }
                Some(Command::Stop) | None => {
                    drop(output);
                    if let Err(error) = guard.disconnect(GoodbyeReason::Shutdown).await {
                        tracing::debug!(%error, "goodbye did not reach the server");
                    }
                    return Ended::Stopped;
                }
            },
        }
    }
}

async fn handle_message(
    message: Message,
    output: &mut Output,
    sender: &WsSender,
    state: &watch::Sender<Snapshot>,
) {
    match message {
        Message::StreamStart(start) => {
            if let Some(player) = &start.player {
                output.start(player);
            }
        }
        Message::StreamClear(clear) => {
            if has_player_role(clear.roles.as_deref()) {
                output.clear();
            }
        }
        Message::StreamEnd(end) => {
            if has_player_role(end.roles.as_deref()) {
                output.end();
            }
        }
        Message::GroupUpdate(update) => {
            if let Some(playback) = update.playback_state {
                state.send_modify(|s| s.playing = playback == PlaybackState::Playing);
            }
        }
        Message::ServerState(server_state) => {
            if let Some(metadata) = server_state.metadata {
                let now_playing = now_playing(metadata);
                state.send_modify(|s| s.now_playing = now_playing);
            }
            if let Some(controller) = server_state.controller {
                state.send_modify(|s| s.controls = Some(controls(controller)));
            }
        }
        Message::ServerCommand(command) => {
            if let Some(command) = command.player {
                player_command(command, output, sender, state).await;
            }
        }
        _ => {}
    }
}

fn has_player_role(roles: Option<&[String]>) -> bool {
    roles.is_none_or(|roles| roles.iter().any(|role| role == "player"))
}

async fn player_command(
    command: PlayerCommand,
    output: &mut Output,
    sender: &WsSender,
    state: &watch::Sender<Snapshot>,
) {
    match command.command {
        PlayerCommandType::Volume => {
            if let Some(volume) = command.volume {
                output.set_volume(volume);
                state.send_modify(|s| s.volume = volume.min(100));
                report_levels(sender, state).await;
            }
        }
        PlayerCommandType::Mute => {
            if let Some(muted) = command.mute {
                output.set_muted(muted);
                state.send_modify(|s| s.muted = muted);
                report_levels(sender, state).await;
            }
        }
        PlayerCommandType::SetStaticDelay => {
            if let Some(delay) = command.static_delay_ms {
                output.set_static_delay(delay);
            }
        }
        PlayerCommandType::Unknown => {}
    }
}

/// Tells the server this player's volume and mute, after either changed.
async fn report_levels(sender: &WsSender, state: &watch::Sender<Snapshot>) {
    let (volume, muted) = {
        let snapshot = state.borrow();
        (snapshot.volume, snapshot.muted)
    };
    let message = Message::ClientState(ClientState {
        state: None,
        player: Some(PlayerState {
            volume: Some(volume),
            muted: Some(muted),
            ..PlayerState::default()
        }),
    });
    if let Err(error) = sender.send_message(message).await {
        tracing::warn!(%error, "could not report the volume");
    }
}

async fn send_remote(
    controller: &Controller,
    remote: Remote,
) -> Result<(), sendspin::error::Error> {
    match remote {
        Remote::Play => controller.play().await,
        Remote::Pause => controller.pause().await,
        Remote::Stop => controller.stop().await,
        Remote::Next => controller.next().await,
        Remote::Previous => controller.previous().await,
        Remote::Volume(volume) => controller.set_volume(volume.min(100)).await,
        Remote::Mute(muted) => controller.set_mute(muted).await,
        Remote::Repeat(repeat) => controller.repeat(repeat_mode(repeat)).await,
        Remote::Shuffle(shuffle) => controller.shuffle(shuffle).await,
        Remote::Seek { position_ms } => controller.seek(position_ms).await,
    }
}

#[allow(deprecated)] // `repeat` and `shuffle` are read from the controller state instead.
fn now_playing(metadata: MetadataState) -> Option<NowPlaying> {
    let MetadataState {
        title,
        artist,
        album_artist,
        album,
        artwork_url,
        year,
        track,
        progress,
        ..
    } = metadata;
    if title.is_none() && artist.is_none() && album.is_none() {
        return None;
    }
    let received_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64);
    Some(NowPlaying {
        title,
        artist,
        album_artist,
        album,
        artwork_url,
        year,
        track,
        progress: progress.map(|progress| Progress {
            position_ms: progress.track_progress.max(0) as u64,
            duration_ms: progress.track_duration.max(0) as u64,
            speed: f64::from(progress.playback_speed) / 1000.0,
            received_at_ms,
        }),
    })
}

fn controls(controller: ControllerState) -> Controls {
    Controls {
        volume: controller.volume.min(100),
        muted: controller.muted,
        repeat: match controller.repeat {
            Some(RepeatMode::One) => Repeat::One,
            Some(RepeatMode::All) => Repeat::All,
            Some(RepeatMode::Off) | None => Repeat::Off,
        },
        shuffle: controller.shuffle.unwrap_or(false),
        seek_max_ms: controller.seek_max_ms,
        supported_commands: controller.supported_commands,
    }
}

fn repeat_mode(repeat: Repeat) -> RepeatMode {
    match repeat {
        Repeat::Off => RepeatMode::Off,
        Repeat::One => RepeatMode::One,
        Repeat::All => RepeatMode::All,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconnects_back_off_to_thirty_seconds() {
        let delays: Vec<u64> = (0..7).map(|n| reconnect_delay(n).as_secs()).collect();
        assert_eq!(delays, [1, 2, 4, 8, 16, 30, 30]);
    }

    #[test]
    fn stream_messages_without_roles_concern_the_player() {
        assert!(has_player_role(None));
        assert!(has_player_role(Some(&["player".to_owned()])));
        assert!(!has_player_role(Some(&["artwork".to_owned()])));
    }
}
