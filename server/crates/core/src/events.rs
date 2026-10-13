//! The event bus. Components publish what changed; the API forwards every event to each
//! authenticated `/ws` connection as Music Assistant's `{event, object_id, data}`.

use serde::Serialize;
use serde_json::Value;
use tokio::sync::broadcast;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    MediaItemAdded,
    MediaItemUpdated,
    MediaItemDeleted,
    MusicSyncCompleted,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Event {
    pub event: EventType,
    pub object_id: Option<String>,
    pub data: Value,
}

/// Subscribers that fall this many events behind miss the oldest ones.
const CAPACITY: usize = 1024;

#[derive(Debug, Clone)]
pub struct EventBus {
    sender: broadcast::Sender<Event>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self {
            sender: broadcast::channel(CAPACITY).0,
        }
    }
}

impl EventBus {
    pub fn publish(&self, event: Event) {
        // Nobody listening is fine.
        let _ = self.sender.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.sender.subscribe()
    }
}
