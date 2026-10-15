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

/// The Sendspin receiver's published Cast app.
pub const RECEIVER_APP_ID: &str = "FAB7FE2B";
