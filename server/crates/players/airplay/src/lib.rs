//! The AirPlay `PlayerProvider`: mDNS discovery of `_raop` and `_airplay`, pairing, the
//! `cliairplay` supervisor and the shared PTP daemon.

pub mod dacp;
pub mod discovery;
pub mod endpoint;
pub mod pidfile;
pub mod provider;
pub mod sink;
pub mod speaker;
pub mod status;
pub mod supervisor;
pub mod txt;

pub use provider::{AirplayProvider, Config, MIGRATIONS};
