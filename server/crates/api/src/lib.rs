//! The subset of the Music Assistant HTTP and WebSocket API that Bopsie uses: routes, envelopes,
//! auth, the command table, event forwarding, the image proxy and the Sendspin proxy.

pub mod auth;
pub mod command;
pub mod commands;
mod http;
pub mod info;
mod ws;

pub use http::router;

use std::sync::Arc;

use bopsie_core::Result;
use bopsie_core::config::ServerConfig;
use bopsie_core::db::{self, Database, Migrations};

use auth::Auth;
use command::Registry;

/// The tables this crate owns.
pub const MIGRATIONS: &[Migrations] = &[auth::MIGRATIONS];

/// Everything a request handler can reach.
pub struct Api {
    pub auth: Auth,
    /// Generated on first start and stable after, so clients can tell servers apart.
    pub server_id: String,
    pub config: ServerConfig,
    pub commands: Registry,
}

impl Api {
    pub async fn new(db: Database, config: ServerConfig, commands: Registry) -> Result<Arc<Self>> {
        let server_id = db
            .call(|conn| {
                Ok(db::setting_or_init(conn, "server_id", || {
                    uuid::Uuid::new_v4().simple().to_string()
                })?)
            })
            .await?;
        Ok(Arc::new(Self {
            auth: Auth::open(db).await?,
            server_id,
            config,
            commands,
        }))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub async fn api() -> Arc<Api> {
        let db = Database::open_in_memory(MIGRATIONS).unwrap();
        Api::new(db, ServerConfig::default(), Registry::default())
            .await
            .unwrap()
    }
}
