//! A queue as Music Assistant keeps one, as a pure state machine. Operations change the queue
//! and say what the player must do; the session carries that out.
//!
//! The player owns the current item and the one handed to it next (`index_in_buffer`). Nothing
//! up to the higher of the two can be moved or removed, and new items go after it, so an edit
//! never pulls the next track out from under the player.

use std::path::PathBuf;
use std::time::Duration;

use bopsie_core::media::Track;
use rand::Rng;
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RepeatMode {
    #[default]
    Off,
    One,
    All,
}

/// How new items enter the queue: Music Assistant's `QueueOption`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueOption {
    /// Insert after what the player holds and play the first new item now.
    Play,
    /// Replace the whole queue and play it from the start.
    Replace,
    /// Insert after what the player holds, to play next.
    Next,
    /// Replace everything after what the player holds.
    ReplaceNext,
    /// Append.
    Add,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PlaybackState {
    #[default]
    Idle,
    Paused,
    Playing,
}

#[derive(Debug, Clone, PartialEq)]
pub struct QueueItem {
    pub queue_item_id: String,
    /// Order of insertion, which turning shuffle off restores.
    pub sort_index: u64,
    pub track: Track,
    /// The file the track plays from.
    pub path: PathBuf,
}

/// What the player must do after an operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    None,
    /// Play from this index now, abandoning what it holds.
    PlayFrom(usize),
    /// What follows the current item changed: drop the prepared track.
    NextChanged,
    /// Stop playing.
    Stop,
    /// Stop playing, keeping the position to resume from.
    Pause,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum QueueError {
    #[error("Item {0} not found in queue")]
    NotFound(String),
    #[error("{0} is already played/buffered")]
    Committed(usize),
}

#[derive(Debug, Clone, Default)]
pub struct Queue {
    pub items: Vec<QueueItem>,
    pub current_index: Option<usize>,
    /// The item the player was handed last: the current one or the one after.
    pub index_in_buffer: Option<usize>,
    pub shuffle_enabled: bool,
    pub repeat_mode: RepeatMode,
    pub state: PlaybackState,
    /// The queue played to its end; `current_index` rests on the last item.
    pub ended: bool,
    /// Where in the current item playback resumes.
    pub resume_position: Duration,
    next_sort_index: u64,
}

impl Queue {
    pub fn index_of(&self, queue_item_id: &str) -> Option<usize> {
        self.items
            .iter()
            .position(|item| item.queue_item_id == queue_item_id)
    }

    pub fn current(&self) -> Option<&QueueItem> {
        self.items.get(self.current_index?)
    }

    fn active(&self) -> bool {
        matches!(self.state, PlaybackState::Playing | PlaybackState::Paused)
    }

    /// The highest index the player owns, if it owns any.
    pub fn committed_index(&self) -> Option<usize> {
        match (self.current_index, self.index_in_buffer) {
            (None, buffered) => buffered,
            (current, None) => current,
            (Some(current), Some(buffered)) => Some(current.max(buffered)),
        }
    }

    /// The index after `index`, honouring repeat. A skip moves on even when repeating one.
    pub fn next_index(&self, index: usize, skip: bool) -> Option<usize> {
        if self.items.is_empty() {
            return None;
        }
        if self.repeat_mode == RepeatMode::One && !skip {
            return Some(index);
        }
        if index + 1 >= self.items.len() {
            return (self.repeat_mode == RepeatMode::All).then_some(0);
        }
        Some(index + 1)
    }

    fn make_items(&mut self, tracks: Vec<(Track, PathBuf)>) -> Vec<QueueItem> {
        tracks
            .into_iter()
            .map(|(track, path)| {
                self.next_sort_index += 1;
                QueueItem {
                    queue_item_id: uuid::Uuid::new_v4().simple().to_string(),
                    sort_index: self.next_sort_index,
                    track,
                    path,
                }
            })
            .collect()
    }

    /// Shuffles the items from `from` on, in place.
    fn shuffle_from(&mut self, from: usize, rng: &mut impl Rng) {
        if from < self.items.len() {
            self.items[from..].shuffle(rng);
        }
    }

    /// Points the current index at the first item when a queue that had none gains some.
    fn ensure_current(&mut self) {
        if self.current_index.is_none() && !self.items.is_empty() {
            self.current_index = Some(0);
        }
    }

    fn reset(&mut self) {
        self.items.clear();
        self.current_index = None;
        self.index_in_buffer = None;
        self.ended = false;
    }

    /// Adds tracks as `option` says.
    pub fn enqueue(
        &mut self,
        tracks: Vec<(Track, PathBuf)>,
        option: QueueOption,
        rng: &mut impl Rng,
    ) -> Effect {
        if tracks.is_empty() {
            return Effect::None;
        }
        // A finished queue starts afresh, unless the new items continue it.
        let continues = self.ended && option == QueueOption::Add;
        let length_before = self.items.len();
        if self.ended && !continues && option != QueueOption::Replace {
            self.reset();
        }
        let base = if self.active() {
            self.committed_index().unwrap_or(0)
        } else {
            self.current_index.unwrap_or(0)
        };
        let insert_at = (base + 1).min(self.items.len());
        let new = self.make_items(tracks);
        let count = new.len();
        let shuffle = self.shuffle_enabled && count > 1;

        match option {
            QueueOption::Replace => {
                self.items = new;
                if shuffle {
                    self.shuffle_from(0, rng);
                }
                self.index_in_buffer = None;
                self.ended = false;
                self.current_index = Some(0);
                Effect::PlayFrom(0)
            }
            QueueOption::Next | QueueOption::ReplaceNext => {
                if option == QueueOption::ReplaceNext {
                    self.items.truncate(insert_at);
                }
                self.items.splice(insert_at..insert_at, new);
                if shuffle {
                    // The first new item still plays next; the rest mix into the tail.
                    self.shuffle_from(insert_at + 1, rng);
                }
                self.ensure_current();
                Effect::None
            }
            QueueOption::Play => {
                let at = if self.current_index.is_none() {
                    0
                } else {
                    insert_at
                };
                self.items.splice(at..at, new);
                if shuffle {
                    self.shuffle_from(at + 1, rng);
                }
                let index = at.min(self.items.len() - 1);
                self.current_index = Some(index);
                self.index_in_buffer = None;
                self.ended = false;
                Effect::PlayFrom(index)
            }
            QueueOption::Add => {
                if self.shuffle_enabled {
                    // Mix into the tail, leaving the item after the boundary where it is.
                    let from = if self.active() {
                        insert_at + 1
                    } else {
                        insert_at
                    };
                    let from = from.min(self.items.len());
                    self.items.extend(new);
                    self.shuffle_from(from, rng);
                } else {
                    self.items.extend(new);
                }
                if continues {
                    self.ended = false;
                    self.current_index = Some(length_before);
                    self.index_in_buffer = None;
                }
                self.ensure_current();
                Effect::None
            }
        }
    }

    pub fn play_index(&mut self, index: usize) -> Effect {
        if index >= self.items.len() {
            return Effect::None;
        }
        self.resume_position = Duration::ZERO;
        self.current_index = Some(index);
        self.index_in_buffer = None;
        self.ended = false;
        Effect::PlayFrom(index)
    }

    pub fn next_track(&mut self) -> Effect {
        let Some(current) = self.current_index else {
            return Effect::None;
        };
        match self.next_index(current, true) {
            Some(index) => self.play_index(index),
            None => Effect::None,
        }
    }

    /// The previous item, or the start of this one when more than five seconds have played.
    pub fn previous_track(&mut self, elapsed_secs: f64) -> Effect {
        let Some(current) = self.current_index else {
            return Effect::None;
        };
        let index = if elapsed_secs < 5.0 {
            current.saturating_sub(1)
        } else {
            current
        };
        self.play_index(index)
    }

    /// Moves an item `shift` places, down when positive. `0` moves it to play next.
    pub fn move_item(&mut self, queue_item_id: &str, shift: i64) -> Result<(), QueueError> {
        let index = self
            .index_of(queue_item_id)
            .ok_or_else(|| QueueError::NotFound(queue_item_id.to_owned()))?;
        let boundary = self.index_in_buffer.and(self.committed_index());
        if boundary.is_some_and(|boundary| index <= boundary) {
            return Err(QueueError::Committed(index));
        }
        let target = if shift == 0 && self.active() {
            boundary.map_or(0, |b| b + 1) as i64
        } else if shift == 0 {
            self.current_index.unwrap_or(0) as i64
        } else {
            index as i64 + shift
        };
        let lowest = boundary.map_or(self.current_index.unwrap_or(0), |b| b + 1) as i64;
        if target < lowest || target > self.items.len() as i64 {
            return Ok(());
        }
        let item = self.items.remove(index);
        let target = (target as usize).min(self.items.len());
        self.items.insert(target, item);
        Ok(())
    }

    /// Removes an item. Items the player holds stay, as in Music Assistant.
    pub fn delete_item(&mut self, queue_item_id: &str) -> Result<(), QueueError> {
        let index = self
            .index_of(queue_item_id)
            .ok_or_else(|| QueueError::NotFound(queue_item_id.to_owned()))?;
        let boundary = self.index_in_buffer.and(self.committed_index());
        if boundary.is_some_and(|boundary| index <= boundary) {
            return Ok(());
        }
        self.items.remove(index);
        if let Some(current) = self.current_index
            && index < current
        {
            self.current_index = Some(current - 1);
        }
        Ok(())
    }

    /// Empties the queue and switches shuffle off.
    pub fn clear(&mut self) -> Effect {
        self.reset();
        self.shuffle_enabled = false;
        self.state = PlaybackState::Idle;
        Effect::Stop
    }

    /// Shuffles what follows the player's items, or restores their order.
    pub fn set_shuffle(&mut self, enabled: bool, rng: &mut impl Rng) -> Effect {
        if self.shuffle_enabled == enabled {
            return Effect::None;
        }
        self.shuffle_enabled = enabled;
        let from = self
            .committed_index()
            .map_or(0, |i| i + 1)
            .min(self.items.len());
        if enabled {
            self.shuffle_from(from, rng);
        } else {
            self.items[from..].sort_by_key(|item| item.sort_index);
        }
        Effect::None
    }

    pub fn set_repeat(&mut self, mode: RepeatMode) -> Effect {
        if self.repeat_mode == mode {
            return Effect::None;
        }
        self.repeat_mode = mode;
        // The next track depends on repeat, so the one handed over may be wrong now.
        self.index_in_buffer = self.current_index;
        Effect::NextChanged
    }

    /// The player wants the item after the one it was handed last: the current item first,
    /// from where it was left. Returns the item and where in it to start.
    pub fn hand_over_next(&mut self) -> Option<(&QueueItem, Duration)> {
        let (index, start) = match self.index_in_buffer {
            None => {
                self.ensure_current();
                let start = std::mem::take(&mut self.resume_position);
                (self.current_index?, start)
            }
            Some(buffered) => (self.next_index(buffered, false)?, Duration::ZERO),
        };
        self.index_in_buffer = Some(index);
        Some((self.items.get(index)?, start))
    }

    /// Pauses at `position` in the current item.
    pub fn pause(&mut self, position: Duration) -> Effect {
        if self.state != PlaybackState::Playing {
            return Effect::None;
        }
        // The player still holds what it was handed, as Music Assistant counts it, so those
        // items stay put until playback starts again.
        self.state = PlaybackState::Paused;
        self.resume_position = position;
        Effect::Pause
    }

    /// Plays from where the queue is: the paused position, the current item, or the start of
    /// a queue that played to its end.
    pub fn play(&mut self) -> Effect {
        if self.state == PlaybackState::Playing || self.items.is_empty() {
            return Effect::None;
        }
        if self.ended {
            self.ended = false;
            self.resume_position = Duration::ZERO;
            return self.play_index(0);
        }
        let index = self.current_index.unwrap_or(0);
        let resume = self.resume_position;
        let effect = self.play_index(index);
        self.resume_position = resume;
        effect
    }

    /// The player started playing the item at `index`.
    pub fn started(&mut self, index: usize) {
        self.current_index = Some(index);
        self.state = PlaybackState::Playing;
        self.ended = false;
    }

    /// The player played everything it was given and has nothing next.
    pub fn finished(&mut self) {
        self.state = PlaybackState::Idle;
        self.index_in_buffer = None;
        if !self.items.is_empty() {
            self.ended = true;
            self.current_index = Some(self.items.len() - 1);
        }
    }

    /// Playback stopped. What the player was handed stays held, as in Music Assistant, until
    /// playback starts again.
    pub fn stopped(&mut self) {
        self.state = PlaybackState::Idle;
        self.resume_position = Duration::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use bopsie_core::media::{ItemFields, MediaType, Metadata};
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    use super::*;

    fn track(name: &str) -> (Track, PathBuf) {
        let track = Track {
            fields: ItemFields {
                item_id: name.to_owned(),
                provider: "library".to_owned(),
                name: name.to_owned(),
                version: String::new(),
                sort_name: name.to_lowercase(),
                uri: format!("library://track/{name}"),
                external_ids: Vec::new(),
                is_playable: true,
                media_type: MediaType::Track,
                provider_mappings: Vec::new(),
                metadata: Metadata::default(),
                favorite: false,
                position: None,
                date_added: None,
            },
            duration: 1,
            artists: Vec::new(),
            last_played: 0,
            album: None,
            disc_number: 0,
            track_number: 0,
            audio_metadata: None,
        };
        (track, PathBuf::from(format!("{name}.wav")))
    }

    fn tracks(names: &str) -> Vec<(Track, PathBuf)> {
        names.chars().map(|c| track(&c.to_string())).collect()
    }

    fn names(queue: &Queue) -> String {
        queue
            .items
            .iter()
            .map(|i| i.track.fields.name.as_str())
            .collect()
    }

    fn rng() -> StdRng {
        StdRng::seed_from_u64(7)
    }

    /// A queue of `names` playing its first item, with the second handed over.
    fn playing(names: &str) -> Queue {
        let mut queue = Queue::default();
        queue.enqueue(tracks(names), QueueOption::Replace, &mut rng());
        queue.hand_over_next();
        queue.started(0);
        queue.hand_over_next();
        queue
    }

    fn id(queue: &Queue, name: &str) -> String {
        queue
            .items
            .iter()
            .find(|i| i.track.fields.name == name)
            .unwrap()
            .queue_item_id
            .clone()
    }

    #[test]
    fn play_on_an_empty_queue_starts_at_the_first_item() {
        let mut queue = Queue::default();
        assert_eq!(
            queue.enqueue(tracks("ab"), QueueOption::Play, &mut rng()),
            Effect::PlayFrom(0)
        );
        assert_eq!(names(&queue), "ab");
        assert_eq!(queue.current_index, Some(0));
    }

    #[test]
    fn enqueue_options_on_a_playing_queue() {
        let cases = [
            // The player holds a (current) and b (handed over).
            (QueueOption::Play, "abxycd", Effect::PlayFrom(2)),
            (QueueOption::Next, "abxycd", Effect::None),
            (QueueOption::ReplaceNext, "abxy", Effect::None),
            (QueueOption::Add, "abcdxy", Effect::None),
            (QueueOption::Replace, "xy", Effect::PlayFrom(0)),
        ];
        for (option, expected, effect) in cases {
            let mut queue = playing("abcd");
            assert_eq!(
                queue.enqueue(tracks("xy"), option, &mut rng()),
                effect,
                "{option:?}"
            );
            assert_eq!(names(&queue), expected, "{option:?}");
        }
    }

    #[test]
    fn next_on_an_idle_queue_goes_after_the_current_item() {
        let mut queue = Queue::default();
        queue.enqueue(tracks("abc"), QueueOption::Add, &mut rng());
        assert_eq!(queue.current_index, Some(0));
        queue.enqueue(tracks("x"), QueueOption::Next, &mut rng());
        assert_eq!(names(&queue), "axbc");
        assert_eq!(queue.current_index, Some(0));
    }

    #[test]
    fn a_finished_queue_starts_afresh_unless_added_to() {
        let mut queue = playing("ab");
        queue.finished();
        assert!(queue.ended);
        assert_eq!(queue.current_index, Some(1));

        let mut fresh = queue.clone();
        fresh.enqueue(tracks("x"), QueueOption::Next, &mut rng());
        assert_eq!(names(&fresh), "x");

        queue.enqueue(tracks("xy"), QueueOption::Add, &mut rng());
        assert_eq!(names(&queue), "abxy");
        assert_eq!(
            queue.current_index,
            Some(2),
            "continues from the first added item"
        );
        assert!(!queue.ended);
    }

    #[test]
    fn the_player_is_handed_items_in_order_with_repeat() {
        let mut queue = Queue::default();
        queue.enqueue(tracks("abc"), QueueOption::Replace, &mut rng());
        let handed = |queue: &mut Queue| {
            queue
                .hand_over_next()
                .map(|(i, _)| i.track.fields.name.clone())
        };
        assert_eq!(handed(&mut queue).as_deref(), Some("a"));
        assert_eq!(handed(&mut queue).as_deref(), Some("b"));
        assert_eq!(handed(&mut queue).as_deref(), Some("c"));
        assert_eq!(handed(&mut queue), None);

        queue.set_repeat(RepeatMode::All);
        queue.index_in_buffer = Some(2);
        assert_eq!(handed(&mut queue).as_deref(), Some("a"), "wraps");
        queue.set_repeat(RepeatMode::One);
        queue.index_in_buffer = Some(1);
        assert_eq!(handed(&mut queue).as_deref(), Some("b"), "repeats");
    }

    #[test]
    fn changing_repeat_takes_back_the_handed_over_item() {
        let mut queue = playing("abc");
        assert_eq!(queue.index_in_buffer, Some(1));
        assert_eq!(queue.set_repeat(RepeatMode::One), Effect::NextChanged);
        assert_eq!(queue.index_in_buffer, Some(0));
        assert_eq!(queue.hand_over_next().unwrap().0.track.fields.name, "a");
        assert_eq!(queue.set_repeat(RepeatMode::One), Effect::None, "unchanged");
    }

    #[test]
    fn skipping_moves_on_even_when_repeating_one() {
        let mut queue = playing("abc");
        queue.set_repeat(RepeatMode::One);
        assert_eq!(queue.next_track(), Effect::PlayFrom(1));
        assert_eq!(queue.previous_track(1.0), Effect::PlayFrom(0));
        assert_eq!(
            queue.previous_track(1.0),
            Effect::PlayFrom(0),
            "stays at the start"
        );
        queue.current_index = Some(2);
        assert_eq!(
            queue.previous_track(9.0),
            Effect::PlayFrom(2),
            "restarts after five seconds"
        );
    }

    #[test]
    fn what_the_player_holds_cannot_be_moved_or_removed() {
        let mut queue = playing("abcde");
        let a = id(&queue, "a");
        let b = id(&queue, "b");
        assert_eq!(queue.move_item(&a, 1), Err(QueueError::Committed(0)));
        assert_eq!(queue.move_item(&b, 1), Err(QueueError::Committed(1)));
        queue.delete_item(&b).unwrap();
        assert_eq!(names(&queue), "abcde", "ignored");
        assert!(matches!(
            queue.move_item("nope", 1),
            Err(QueueError::NotFound(_))
        ));
    }

    #[test]
    fn moving_items_after_the_player() {
        let mut queue = playing("abcde");
        // e up two places: to play right after what the player holds.
        queue.move_item(&id(&queue, "e"), -2).unwrap();
        assert_eq!(names(&queue), "abecd");
        // Not past the boundary.
        queue.move_item(&id(&queue, "d"), -4).unwrap();
        assert_eq!(names(&queue), "abecd");
        queue.move_item(&id(&queue, "c"), 1).unwrap();
        assert_eq!(names(&queue), "abedc");
        // 0 moves it to play next.
        queue.move_item(&id(&queue, "c"), 0).unwrap();
        assert_eq!(names(&queue), "abced");
    }

    #[test]
    fn removing_items() {
        let mut queue = playing("abcd");
        queue.delete_item(&id(&queue, "c")).unwrap();
        assert_eq!(names(&queue), "abd");
    }

    #[test]
    fn shuffle_mixes_only_the_tail_and_unshuffle_restores_it() {
        let mut queue = playing("abcdefgh");
        queue.set_shuffle(true, &mut rng());
        assert_eq!(&names(&queue)[..2], "ab");
        assert_ne!(names(&queue), "abcdefgh");
        queue.set_shuffle(false, &mut rng());
        assert_eq!(names(&queue), "abcdefgh");
    }

    #[test]
    fn play_next_with_shuffle_keeps_the_first_new_item_next() {
        let mut queue = playing("abcd");
        queue.set_shuffle(true, &mut rng());
        queue.enqueue(tracks("xyz"), QueueOption::Next, &mut rng());
        assert_eq!(&names(&queue)[..3], "abx");
    }

    #[test]
    fn pausing_and_playing_resume_where_they_left_off() {
        let mut queue = playing("abc");
        assert_eq!(queue.pause(Duration::from_secs(7)), Effect::Pause);
        assert_eq!(queue.state, PlaybackState::Paused);
        assert_eq!(queue.index_in_buffer, Some(1), "still held");
        assert_eq!(
            queue.pause(Duration::from_secs(9)),
            Effect::None,
            "already paused"
        );

        assert_eq!(queue.play(), Effect::PlayFrom(0));
        let (item, start) = queue.hand_over_next().unwrap();
        assert_eq!(
            (item.track.fields.name.as_str(), start),
            ("a", Duration::from_secs(7))
        );
        let (item, start) = queue.hand_over_next().unwrap();
        assert_eq!(
            (item.track.fields.name.as_str(), start),
            ("b", Duration::ZERO)
        );
    }

    #[test]
    fn playing_a_finished_queue_starts_it_over() {
        let mut queue = playing("ab");
        queue.finished();
        assert_eq!(queue.play(), Effect::PlayFrom(0));
        assert!(!queue.ended);
    }

    #[test]
    fn clearing_stops_and_switches_shuffle_off() {
        let mut queue = playing("abc");
        queue.set_shuffle(true, &mut rng());
        assert_eq!(queue.clear(), Effect::Stop);
        assert!(queue.items.is_empty());
        assert!(!queue.shuffle_enabled);
        assert_eq!(queue.current_index, None);
    }
}
