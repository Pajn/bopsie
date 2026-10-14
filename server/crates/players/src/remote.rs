//! A speaker's own controls: its buttons, its remote, its app. Protocols report what was asked
//! for; playback carries it out on the group the speaker plays in.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// What a speaker's controls asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Remote {
    Play,
    Pause,
    PlayPause,
    Stop,
    Next,
    Previous,
    VolumeUp,
    VolumeDown,
    /// The speaker switched to another source, or was turned off: it no longer plays what the
    /// server sends.
    Released,
}

impl Remote {
    /// A command as `cliairplay` names it in its `[EVENT] remote command=` lines.
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "play" => Self::Play,
            "pause" => Self::Pause,
            "play_pause" => Self::PlayPause,
            "stop" => Self::Stop,
            "next" => Self::Next,
            "previous" => Self::Previous,
            _ => return None,
        })
    }

    /// How long a repeat of the command is taken for the same press. Some speakers send an
    /// unanswered command again at about ten a second; a skip that repeats would race through
    /// the queue.
    fn window(self) -> Duration {
        match self {
            Self::Next | Self::Previous => Duration::from_secs(2),
            Self::VolumeUp | Self::VolumeDown => Duration::from_millis(150),
            _ => Duration::from_millis(500),
        }
    }
}

/// Lets through the first of a burst of repeated commands from one endpoint.
#[derive(Debug, Default)]
pub struct Guard {
    last: HashMap<(String, Remote), Instant>,
}

impl Guard {
    /// Whether `remote` from `endpoint` at `now` is a new press rather than a repeat.
    pub fn admit(&mut self, endpoint: &str, remote: Remote, now: Instant) -> bool {
        let key = (endpoint.to_owned(), remote);
        if self
            .last
            .get(&key)
            .is_some_and(|last| now.saturating_duration_since(*last) < remote.window())
        {
            return false;
        }
        self.last.insert(key, now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_burst_of_repeats_is_one_press() {
        let mut guard = Guard::default();
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        assert!(guard.admit("ap1", Remote::Pause, at(0)));
        assert!(!guard.admit("ap1", Remote::Pause, at(100)));
        assert!(!guard.admit("ap1", Remote::Pause, at(400)));
        // Another speaker, or another command, is its own press.
        assert!(guard.admit("ap2", Remote::Pause, at(100)));
        assert!(guard.admit("ap1", Remote::Play, at(150)));
        assert!(guard.admit("ap1", Remote::Pause, at(600)));
        // Skips are held back longer.
        assert!(guard.admit("ap1", Remote::Next, at(0)));
        assert!(!guard.admit("ap1", Remote::Next, at(1500)));
        assert!(guard.admit("ap1", Remote::Next, at(2100)));
    }

    #[test]
    fn cliairplay_names() {
        assert_eq!(Remote::from_name("play_pause"), Some(Remote::PlayPause));
        assert_eq!(Remote::from_name("previous"), Some(Remote::Previous));
        assert_eq!(Remote::from_name("louder"), None);
    }
}
