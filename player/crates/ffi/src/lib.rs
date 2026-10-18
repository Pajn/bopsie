//! The UniFFI surface over `bopsie-player`: the only API the Swift and Kotlin apps see.
//!
//! The types here mirror the client's rather than exporting them, so that changes in the client,
//! or in the `sendspin` crate under it, stop at this crate. Everything an app needs to show is in
//! a [`Snapshot`], delivered to a [`PlayerObserver`] on every change.

use std::sync::{Arc, Mutex, Once};

use bopsie_player as client;

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum PlayerError {
    #[error("{message}")]
    InvalidAddress { message: String },
    #[error("{message}")]
    Connection { message: String },
    #[error("the server has not been set up yet")]
    SetupRequired,
    #[error("this server is too old for this version of Bopsie")]
    ServerTooOld,
    #[error("{message}")]
    WrongCredentials { message: String },
    #[error("the server no longer accepts this sign-in")]
    Unauthorized,
    #[error("{message}")]
    Other { message: String },
}

impl From<client::Error> for PlayerError {
    fn from(error: client::Error) -> Self {
        let message = error.to_string();
        match error {
            client::Error::InvalidAddress(_) => Self::InvalidAddress { message },
            client::Error::Connection(_) => Self::Connection { message },
            client::Error::SetupRequired => Self::SetupRequired,
            client::Error::ServerTooOld => Self::ServerTooOld,
            client::Error::WrongCredentials(message) => Self::WrongCredentials { message },
            client::Error::Unauthorized => Self::Unauthorized,
            client::Error::Command { .. }
            | client::Error::Protocol(_)
            | client::Error::Runtime(_) => Self::Other { message },
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct SignIn {
    pub server_url: String,
    pub server_id: String,
    pub user_id: String,
    pub username: String,
    pub token: String,
}

/// Normalizes a typed server address, as sign-in does.
#[uniffi::export]
pub fn normalize_server_url(input: String) -> Result<String, PlayerError> {
    Ok(client::normalize_server_url(&input)?)
}

/// Signs in and returns a long-lived token. `device_name` is what the server's list of sign-ins
/// shows.
#[uniffi::export(async_runtime = "tokio")]
pub async fn sign_in(
    server_url: String,
    username: String,
    password: String,
    device_name: String,
) -> Result<SignIn, PlayerError> {
    let signed_in = client::sign_in(&server_url, &username, &password, &device_name).await?;
    Ok(SignIn {
        server_url: signed_in.server_url,
        server_id: signed_in.server_id,
        user_id: signed_in.user_id,
        username: signed_in.username,
        token: signed_in.token,
    })
}

/// The `device_info.product_name` for desktop players.
#[uniffi::export]
pub fn desktop_product_name() -> String {
    client::PRODUCT_NAME.to_owned()
}

/// Sends the player's logs to standard error, filtered by `filter` (`RUST_LOG` syntax). Only the
/// first call has an effect.
#[uniffi::export]
pub fn init_logging(filter: String) {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::new(filter))
            .with_ansi(false)
            .try_init();
    });
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct PlayerConfig {
    pub server_url: String,
    pub token: String,
    pub client_id: String,
    pub name: String,
    pub product_name: String,
    pub software_version: Option<String>,
    pub volume: u8,
    pub muted: bool,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct Snapshot {
    pub connection: ConnectionState,
    pub playing: bool,
    pub now_playing: Option<NowPlaying>,
    pub controls: Option<Controls>,
    pub volume: u8,
    pub muted: bool,
}

#[derive(Debug, Clone, uniffi::Enum)]
pub enum ConnectionState {
    Connecting,
    Connected,
    Reconnecting { attempt: u32, error: String },
    Unauthorized,
    Stopped,
}

#[derive(Debug, Clone, uniffi::Record)]
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

#[derive(Debug, Clone, uniffi::Record)]
pub struct Progress {
    pub position_ms: u64,
    pub duration_ms: u64,
    pub speed: f64,
    pub received_at_ms: u64,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct Controls {
    pub volume: u8,
    pub muted: bool,
    pub repeat: Repeat,
    pub shuffle: bool,
    pub seek_max_ms: Option<u64>,
    pub supported_commands: Vec<String>,
}

#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum Repeat {
    Off,
    One,
    All,
}

#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum Remote {
    Play,
    Pause,
    Stop,
    Next,
    Previous,
    Volume { volume: u8 },
    Mute { muted: bool },
    Repeat { repeat: Repeat },
    Shuffle { shuffle: bool },
    Seek { position_ms: u64 },
}

/// Implemented by the app to hear about every change to the player's state. Called on a thread of
/// the player's, not the app's main thread.
#[uniffi::export(with_foreign)]
pub trait PlayerObserver: Send + Sync {
    fn changed(&self, snapshot: Snapshot);
}

/// A running player. It connects when created and keeps reconnecting until stopped.
#[derive(uniffi::Object)]
pub struct PlayerHandle {
    player: Mutex<Option<client::Player>>,
}

#[uniffi::export]
impl PlayerHandle {
    #[uniffi::constructor]
    pub fn start(
        config: PlayerConfig,
        observer: Arc<dyn PlayerObserver>,
    ) -> Result<Arc<Self>, PlayerError> {
        let player = client::Player::start(client::PlayerConfig {
            server_url: config.server_url,
            token: config.token,
            client_id: config.client_id,
            name: config.name,
            product_name: config.product_name,
            software_version: config.software_version,
            volume: config.volume,
            muted: config.muted,
        })?;
        let mut changes = player.watch();
        observer.changed(changes.borrow_and_update().clone().into());
        std::thread::Builder::new()
            .name("bopsie-player-observer".to_owned())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .build()
                    .expect("a runtime without drivers builds");
                runtime.block_on(async {
                    while changes.changed().await.is_ok() {
                        let snapshot = changes.borrow_and_update().clone();
                        observer.changed(snapshot.into());
                    }
                });
            })
            .map_err(|e| PlayerError::Other {
                message: e.to_string(),
            })?;
        Ok(Arc::new(Self {
            player: Mutex::new(Some(player)),
        }))
    }

    pub fn snapshot(&self) -> Option<Snapshot> {
        self.with_player(|player| player.snapshot().into())
    }

    /// Sends a command to the player's group.
    pub fn remote(&self, remote: Remote) {
        self.with_player(|player| player.remote(remote.into()));
    }

    /// Sets this player's own volume, 0 to 100.
    pub fn set_volume(&self, volume: u8) {
        self.with_player(|player| player.set_volume(volume));
    }

    pub fn set_muted(&self, muted: bool) {
        self.with_player(|player| player.set_muted(muted));
    }

    /// Says goodbye to the server and waits for the player to finish. Later calls do nothing.
    pub fn stop(&self) {
        let player = self.player.lock().expect("player lock").take();
        if let Some(player) = player {
            player.stop();
        }
    }
}

impl PlayerHandle {
    fn with_player<T>(&self, f: impl FnOnce(&client::Player) -> T) -> Option<T> {
        self.player.lock().expect("player lock").as_ref().map(f)
    }
}

impl From<client::Snapshot> for Snapshot {
    fn from(snapshot: client::Snapshot) -> Self {
        Self {
            connection: match snapshot.connection {
                client::ConnectionState::Connecting => ConnectionState::Connecting,
                client::ConnectionState::Connected => ConnectionState::Connected,
                client::ConnectionState::Reconnecting { attempt, error } => {
                    ConnectionState::Reconnecting { attempt, error }
                }
                client::ConnectionState::Unauthorized => ConnectionState::Unauthorized,
                client::ConnectionState::Stopped => ConnectionState::Stopped,
            },
            playing: snapshot.playing,
            now_playing: snapshot.now_playing.map(|now| NowPlaying {
                title: now.title,
                artist: now.artist,
                album_artist: now.album_artist,
                album: now.album,
                artwork_url: now.artwork_url,
                year: now.year,
                track: now.track,
                progress: now.progress.map(|progress| Progress {
                    position_ms: progress.position_ms,
                    duration_ms: progress.duration_ms,
                    speed: progress.speed,
                    received_at_ms: progress.received_at_ms,
                }),
            }),
            controls: snapshot.controls.map(|controls| Controls {
                volume: controls.volume,
                muted: controls.muted,
                repeat: controls.repeat.into(),
                shuffle: controls.shuffle,
                seek_max_ms: controls.seek_max_ms,
                supported_commands: controls.supported_commands,
            }),
            volume: snapshot.volume,
            muted: snapshot.muted,
        }
    }
}

impl From<client::Repeat> for Repeat {
    fn from(repeat: client::Repeat) -> Self {
        match repeat {
            client::Repeat::Off => Self::Off,
            client::Repeat::One => Self::One,
            client::Repeat::All => Self::All,
        }
    }
}

impl From<Repeat> for client::Repeat {
    fn from(repeat: Repeat) -> Self {
        match repeat {
            Repeat::Off => Self::Off,
            Repeat::One => Self::One,
            Repeat::All => Self::All,
        }
    }
}

impl From<Remote> for client::Remote {
    fn from(remote: Remote) -> Self {
        match remote {
            Remote::Play => Self::Play,
            Remote::Pause => Self::Pause,
            Remote::Stop => Self::Stop,
            Remote::Next => Self::Next,
            Remote::Previous => Self::Previous,
            Remote::Volume { volume } => Self::Volume(volume),
            Remote::Mute { muted } => Self::Mute(muted),
            Remote::Repeat { repeat } => Self::Repeat(repeat.into()),
            Remote::Shuffle { shuffle } => Self::Shuffle(shuffle),
            Remote::Seek { position_ms } => Self::Seek { position_ms },
        }
    }
}
