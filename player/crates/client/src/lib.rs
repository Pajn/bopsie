//! Bopsie's Sendspin player: signs in to a Bopsie server, connects to its `/sendspin` route and
//! plays what the server sends, reconnecting when the connection drops.
//!
//! The protocol, clock synchronization and synchronized output come from the `sendspin` crate.
//! This crate adds what is Bopsie's: sign-in, the authentication the server's `/sendspin` route
//! requires, reconnection, and a state model for user interfaces. It is the one client every
//! Bopsie player builds on; see `docs/plans/2026-10-18-shared-sendspin-client.md`.

mod address;
mod api;
mod output;
mod player;
#[cfg(test)]
mod server_tests;

pub use address::normalize_server_url;
pub use api::{SignIn, sign_in};
pub use player::{
    ConnectionState, Controls, NowPlaying, PRODUCT_NAME, Player, PlayerConfig, Progress, Remote,
    Repeat, Snapshot,
};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("not a server address: {0}")]
    InvalidAddress(String),
    #[error("could not reach the server: {0}")]
    Connection(String),
    #[error("the server has not been set up yet")]
    SetupRequired,
    #[error("this server is too old for this version of Bopsie")]
    ServerTooOld,
    #[error("{0}")]
    WrongCredentials(String),
    #[error("the server no longer accepts this sign-in")]
    Unauthorized,
    #[error("the server answered with error {code}: {details}")]
    Command { code: i64, details: String },
    #[error("unexpected answer from the server: {0}")]
    Protocol(String),
    #[error("could not start the player: {0}")]
    Runtime(String),
}
