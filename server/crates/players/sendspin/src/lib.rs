//! The Sendspin `PlayerProvider`: the server role of the Sendspin protocol on the legacy wire,
//! with discovery, per-client state, buffer accounting and the chunk scheduler.

mod connection;
pub mod pacer;
mod sink;
pub mod wire;

pub use connection::{SILENCE_LIMIT, SendspinEndpoint, SendspinServer};
