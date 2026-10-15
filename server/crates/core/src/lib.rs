//! Ids, media and player models, events, errors, the `Timeline`, config loading and the event
//! bus. Every other crate depends on this one; it depends on none of them.

pub mod backoff;
pub mod clock;
pub mod config;
pub mod db;
pub mod error;
pub mod events;
pub mod logtail;
pub mod media;
pub mod pace;
pub mod quirks;

pub use error::{Error, ErrorCode, Result};
