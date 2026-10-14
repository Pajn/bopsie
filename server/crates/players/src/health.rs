//! How well an endpoint is playing, from what the server sees of it: speakers report almost
//! nothing, so outputs report events and they are folded into a level.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use serde::Serialize;

/// Something an output saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    /// Connected to the speaker.
    Connected,
    /// The speaker's clock follows ours.
    ClockReady,
    /// The speaker started on time.
    Started,
    /// Connecting failed.
    ConnectFailed,
    /// The speaker stopped following our clock, or never did.
    ClockStalled,
    /// The session ended without being asked to.
    Exited,
    /// The speaker started later than asked.
    LateStart,
    /// Audio reached the player too late to play.
    LateChunk,
    /// The player's connection dropped without it saying goodbye.
    Dropped,
}

enum Kind {
    /// Things working: what failed has recovered.
    Working,
    /// Things failing, until something works again.
    Failure,
    /// Something went wrong and passed.
    Problem,
}

impl Event {
    fn kind(self) -> Kind {
        match self {
            Self::Connected | Self::ClockReady | Self::Started => Kind::Working,
            Self::ConnectFailed | Self::ClockStalled | Self::Exited => Kind::Failure,
            Self::LateStart | Self::LateChunk | Self::Dropped => Kind::Problem,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    #[default]
    Good,
    Degraded,
    Failing,
}

/// How far back problems count.
pub const WINDOW: Duration = Duration::from_secs(10 * 60);
/// Problems in the window that make an endpoint degraded.
pub const DEGRADED_AT: usize = 3;
/// Events kept per endpoint.
const KEPT: usize = 256;

/// The level `recent` events, oldest first, add up to at `now`: failing while the latest
/// failure has not been followed by anything working; degraded with [`DEGRADED_AT`] problems
/// or failures in the last [`WINDOW`]; else good.
pub fn assess(recent: &[(Instant, Event)], now: Instant) -> Level {
    let failing = recent
        .iter()
        .rev()
        .find_map(|(_, event)| match event.kind() {
            Kind::Failure => Some(true),
            Kind::Working => Some(false),
            Kind::Problem => None,
        })
        .unwrap_or(false);
    if failing {
        return Level::Failing;
    }
    let troubles = recent
        .iter()
        .filter(|(at, event)| {
            now.saturating_duration_since(*at) <= WINDOW && !matches!(event.kind(), Kind::Working)
        })
        .count();
    if troubles >= DEGRADED_AT {
        Level::Degraded
    } else {
        Level::Good
    }
}

/// An endpoint's health: its level and how often each event happened.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Health {
    pub level: Level,
    pub counts: BTreeMap<Event, u64>,
}

/// An endpoint's events.
#[derive(Debug, Default)]
pub(crate) struct Log {
    /// Oldest first.
    recent: Vec<(Instant, Event)>,
    counts: BTreeMap<Event, u64>,
}

impl Log {
    /// Adds `event`, and says whether that changed the level.
    pub fn record(&mut self, event: Event, now: Instant) -> bool {
        let before = self.level(now);
        if self.recent.len() == KEPT {
            self.recent.remove(0);
        }
        self.recent.push((now, event));
        *self.counts.entry(event).or_default() += 1;
        self.level(now) != before
    }

    fn level(&self, now: Instant) -> Level {
        assess(&self.recent, now)
    }

    /// Newest first, each with how long before `now` it happened.
    pub fn history(&self, now: Instant) -> Vec<(Duration, Event)> {
        self.recent
            .iter()
            .rev()
            .map(|(at, event)| (now.saturating_duration_since(*at), *event))
            .collect()
    }

    pub fn health(&self, now: Instant) -> Health {
        Health {
            level: self.level(now),
            counts: self.counts.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels() {
        let now = Instant::now();
        let ago = |s: u64| now - Duration::from_secs(s);
        use Event::*;
        type Case = (&'static str, Vec<(Instant, Event)>, Level);
        let cases: [Case; 8] = [
            ("nothing seen", vec![], Level::Good),
            (
                "working",
                vec![(ago(5), Connected), (ago(4), ClockReady)],
                Level::Good,
            ),
            (
                "a failure not yet recovered",
                vec![(ago(5), Connected), (ago(4), ConnectFailed)],
                Level::Failing,
            ),
            (
                "a failure, then problems, still not recovered",
                vec![(ago(5), ClockStalled), (ago(4), LateChunk)],
                Level::Failing,
            ),
            (
                "a failure that recovered",
                vec![(ago(5), Exited), (ago(4), Connected)],
                Level::Good,
            ),
            (
                "problems in the window",
                vec![
                    (ago(30), LateStart),
                    (ago(20), LateChunk),
                    (ago(10), Dropped),
                ],
                Level::Degraded,
            ),
            (
                "recovered failures count as problems",
                vec![
                    (ago(30), ConnectFailed),
                    (ago(29), Connected),
                    (ago(20), Exited),
                    (ago(19), Connected),
                    (ago(10), LateStart),
                ],
                Level::Degraded,
            ),
            (
                "problems from long ago",
                vec![
                    (ago(3600), LateStart),
                    (ago(3500), LateChunk),
                    (ago(10), LateChunk),
                ],
                Level::Good,
            ),
        ];
        for (case, events, expected) in cases {
            assert_eq!(assess(&events, now), expected, "{case}");
        }
    }

    #[test]
    fn a_log_counts_and_says_when_the_level_changes() {
        let mut log = Log::default();
        let now = Instant::now();
        assert!(!log.record(Event::Connected, now));
        assert!(log.record(Event::ConnectFailed, now));
        assert!(!log.record(Event::ConnectFailed, now));
        assert!(log.record(Event::Connected, now));
        let health = log.health(now);
        assert_eq!(health.level, Level::Good);
        assert_eq!(health.counts[&Event::ConnectFailed], 2);
        assert_eq!(health.counts[&Event::Connected], 2);
    }

    #[test]
    fn a_full_log_keeps_the_latest() {
        let mut log = Log::default();
        let now = Instant::now();
        for _ in 0..KEPT {
            log.record(Event::Connected, now);
        }
        log.record(Event::ConnectFailed, now);
        assert_eq!(log.health(now).level, Level::Failing);
        log.record(Event::Connected, now);
        assert_eq!(log.health(now).level, Level::Good);
    }
}
