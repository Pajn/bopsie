//! The subset of the Music Assistant HTTP and WebSocket API that Bopsie uses: routes, envelopes,
//! auth, the command table, event forwarding, the image proxy and the Sendspin proxy.

pub mod auth;
pub mod command;
pub mod commands;
mod http;
mod imageproxy;
pub mod info;
mod sendspin;
mod web;
mod ws;

pub use http::router;
pub use web::Web;

use std::sync::Arc;

use bopsie_core::Result;
use bopsie_core::config::Config;
use bopsie_core::db::{self, Database, Migrations};
use bopsie_core::events::EventBus;
use bopsie_core::logtail::Tail;
use bopsie_library::Library;
use bopsie_players::Players;
use bopsie_players_airplay::AirplayProvider;
use bopsie_players_linkplay::LinkplayProvider;
use bopsie_players_sendspin::SendspinServer;
use bopsie_queues::Playback;
use bopsie_sources::Sources;

use auth::Auth;
use command::Registry;

/// The tables this crate owns.
pub const MIGRATIONS: &[Migrations] = &[auth::MIGRATIONS];

/// The parts of the server the API serves.
pub struct Services {
    pub library: Library,
    pub events: EventBus,
    pub players: Players,
    pub playback: Playback,
    /// Audio from outside the queue, such as Spotify Connect.
    pub sources: Sources,
    /// AirPlay, when `cliairplay` is installed.
    pub airplay: Option<AirplayProvider>,
    /// LinkPlay, unless the config file turns it off.
    pub linkplay: Option<LinkplayProvider>,
    /// Where the web UI's files come from.
    pub web: Web,
    /// Whether ffmpeg was found, for the formats only it decodes.
    pub ffmpeg: bool,
    /// The log's latest lines.
    pub logs: Tail,
}

/// Everything a request handler can reach.
pub struct Api {
    pub auth: Auth,
    pub library: Library,
    pub events: EventBus,
    pub playback: Playback,
    pub sources: Sources,
    pub airplay: Option<AirplayProvider>,
    pub linkplay: Option<LinkplayProvider>,
    /// The Sendspin server role, which `/sendspin` and the direct listener hand clients to.
    pub sendspin: SendspinServer,
    /// Generated on first start and stable after, so clients can tell servers apart.
    pub server_id: String,
    pub config: Config,
    pub commands: Registry,
    pub ffmpeg: bool,
    pub logs: Tail,
    /// When the server started, in Unix seconds.
    pub started: u64,
    web: Web,
    thumbnails: imageproxy::Thumbnails,
}

impl Api {
    pub async fn new(
        db: Database,
        config: Config,
        services: Services,
        commands: Registry,
    ) -> Result<Arc<Self>> {
        let server_id = db
            .call(|conn| {
                Ok(db::setting_or_init(conn, "server_id", || {
                    uuid::Uuid::new_v4().simple().to_string()
                })?)
            })
            .await?;
        let sendspin = SendspinServer {
            players: services.players,
            server_id: server_id.clone(),
            name: config.server.name.clone(),
            silence_limit: bopsie_players_sendspin::SILENCE_LIMIT,
        };
        Ok(Arc::new(Self {
            auth: Auth::open(db).await?,
            library: services.library,
            events: services.events,
            playback: services.playback,
            sources: services.sources,
            airplay: services.airplay,
            linkplay: services.linkplay,
            sendspin,
            server_id,
            config,
            commands,
            ffmpeg: services.ffmpeg,
            logs: services.logs,
            started: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            web: services.web,
            thumbnails: imageproxy::Thumbnails::default(),
        }))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub async fn api() -> Arc<Api> {
        let db = Database::open_in_memory(&[MIGRATIONS, &[bopsie_library::MIGRATIONS]].concat())
            .unwrap();
        let events = EventBus::default();
        let players = Players::default();
        let devices = bopsie_players::Devices::new(
            players.clone(),
            Database::open_in_memory(&[bopsie_players::devices::MIGRATIONS]).unwrap(),
        )
        .await
        .unwrap();
        let services = Services {
            library: Library::new(db.clone(), events.clone(), Vec::new(), "artwork".into()),
            playback: bopsie_queues::Playback::start(
                devices,
                events.clone(),
                bopsie_queues::Config::default(),
            ),
            sources: Sources::default(),
            events,
            players,
            airplay: None,
            linkplay: None,
            web: Web::default(),
            ffmpeg: false,
            logs: Tail::default(),
        };
        Api::new(db, Config::default(), services, Registry::default())
            .await
            .unwrap()
    }
}
