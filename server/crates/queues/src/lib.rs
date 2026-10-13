//! One queue per device with Music Assistant semantics, and the bridge from a queue to the stream
//! engine.

pub mod session;
pub mod state;

pub use session::{Notice, QueueSession};
pub use state::{Effect, PlaybackState, Queue, QueueError, QueueItem, QueueOption, RepeatMode};
