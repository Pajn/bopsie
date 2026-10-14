//! The LinkPlay `PlayerProvider`: WiiM, Audio Pro and other speakers built on LinkPlay's
//! modules, controlled over their HTTP API, played by a stream they pull from the server, and
//! grouped by their own multiroom.

pub mod discovery;
pub mod endpoint;
pub mod provider;
pub mod status;
pub mod stream;
mod tls;
pub mod transport;
pub mod upnp;

pub use endpoint::{LinkplayEndpoint, ServerAddress};
pub use provider::{Config, LinkplayProvider};
