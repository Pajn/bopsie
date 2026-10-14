//! Server info: the first frame on every `/ws` connection, and the body of `GET /info`.

use bopsie_core::Result;
use serde::Serialize;

use crate::Api;

/// The Music Assistant schema version this server reports. Clients gate features on it, so it
/// is the version of the Music Assistant release whose API this server follows (2.10.6), not a
/// measure of how much of that API is implemented.
pub const SCHEMA_VERSION: u32 = 65;
/// The oldest schema a client may assume: 28 is where authentication arrived.
pub const MIN_SUPPORTED_SCHEMA_VERSION: u32 = 28;

/// Music Assistant's `ServerInfoMessage`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ServerInfo {
    pub server_id: String,
    pub server_version: &'static str,
    pub schema_version: u32,
    pub min_supported_schema_version: u32,
    /// Deprecated in Music Assistant, which still sends it as an alias of `internal_url`.
    pub base_url: Option<String>,
    pub homeassistant_addon: bool,
    pub onboard_done: bool,
    pub name: String,
    pub status: &'static str,
    pub internal_url: Option<String>,
    pub external_url: Option<String>,
    pub has_remote_access: bool,
}

pub async fn server_info(api: &Api) -> Result<ServerInfo> {
    Ok(ServerInfo {
        server_id: api.server_id.clone(),
        server_version: env!("CARGO_PKG_VERSION"),
        schema_version: SCHEMA_VERSION,
        min_supported_schema_version: MIN_SUPPORTED_SCHEMA_VERSION,
        base_url: api.config.server.base_url.clone(),
        homeassistant_addon: false,
        // There is no onboarding beyond creating the first user.
        onboard_done: api.auth.has_users().await?,
        name: api.config.server.name.clone(),
        status: "running",
        internal_url: api.config.server.base_url.clone(),
        external_url: None,
        has_remote_access: false,
    })
}
