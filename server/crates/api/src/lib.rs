//! The subset of the Music Assistant HTTP and WebSocket API that Bopsie uses: routes, envelopes,
//! auth, the command table, event forwarding, the image proxy and the Sendspin proxy.

pub mod auth;
pub mod command;
pub mod commands;
mod http;
mod imageproxy;
pub mod info;
mod sendspin;
mod ws;

pub use http::router;

use std::sync::Arc;

use bopsie_core::Result;
use bopsie_core::config::ServerConfig;
use bopsie_core::db::{self, Database, Migrations};
use bopsie_core::events::EventBus;
use bopsie_library::Library;
use bopsie_players::Players;
use bopsie_players_airplay::AirplayProvider;
use bopsie_players_sendspin::SendspinServer;
use bopsie_queues::Playback;

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
    /// AirPlay, when `cliairplay` is installed.
    pub airplay: Option<AirplayProvider>,
}

/// Everything a request handler can reach.
pub struct Api {
    pub auth: Auth,
    pub library: Library,
    pub events: EventBus,
    pub playback: Playback,
    pub airplay: Option<AirplayProvider>,
    /// The Sendspin server role, which `/sendspin` and the direct listener hand clients to.
    pub sendspin: SendspinServer,
    /// Generated on first start and stable after, so clients can tell servers apart.
    pub server_id: String,
    pub config: ServerConfig,
    pub commands: Registry,
    thumbnails: imageproxy::Thumbnails,
}

impl Api {
    pub async fn new(
        db: Database,
        config: ServerConfig,
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
            name: config.name.clone(),
            silence_limit: bopsie_players_sendspin::SILENCE_LIMIT,
        };
        Ok(Arc::new(Self {
            auth: Auth::open(db).await?,
            library: services.library,
            events: services.events,
            playback: services.playback,
            airplay: services.airplay,
            sendspin,
            server_id,
            config,
            commands,
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
            events,
            players,
            airplay: None,
        };
        Api::new(db, ServerConfig::default(), services, Registry::default())
            .await
            .unwrap()
    }
}
