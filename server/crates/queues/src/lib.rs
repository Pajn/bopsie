//! One queue per device with Music Assistant semantics, and the bridge from a queue to the stream
//! engine.

// Music Assistant's player model is wider than `json!` expands by default.
#![recursion_limit = "256"]

pub mod model;
pub mod playback;
pub mod session;
pub mod state;
pub mod store;

pub use playback::{Config, Group, Playback, QueueCommand};
pub use session::{Notice, Position, QueueSession};
pub use state::{Effect, PlaybackState, Queue, QueueError, QueueItem, QueueOption, RepeatMode};
