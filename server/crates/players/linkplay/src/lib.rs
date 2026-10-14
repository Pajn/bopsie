//! The LinkPlay `PlayerProvider`: WiiM, Audio Pro and other speakers built on LinkPlay's
//! modules, controlled over their HTTP API, played by a stream they pull from the server, and
//! grouped by their own multiroom.

pub mod status;
mod tls;
pub mod transport;
