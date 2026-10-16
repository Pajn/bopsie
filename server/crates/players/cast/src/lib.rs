//! The Cast `PlayerProvider`: Google Cast devices, found over mDNS and played by launching the
//! Sendspin receiver on them, a Cast app that connects back to the server as a Sendspin client.
//! The server keeps each device's Cast channel open, which keeps the receiver running while it
//! plays.

mod channel;
pub mod discovery;
pub mod endpoint;
pub mod message;
pub mod provider;
mod tls;

pub use endpoint::{CastEndpoint, Settings};
pub use provider::CastProvider;

/// Bopsie's Cast app: the Sendspin receiver, with a media session that a WiiM needs to keep
/// sounding it; see the README.
pub const RECEIVER_APP_ID: &str = "F59DDD2C";
