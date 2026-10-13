//! Ids, media and player models, events, errors, the `Timeline`, config loading and the event
//! bus. Every other crate depends on this one; it depends on none of them.

pub mod config;
pub mod db;
pub mod error;
pub mod events;
pub mod media;

pub use error::{Error, ErrorCode, Result};
