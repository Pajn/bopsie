//! What of playback outlasts a restart: each queue, which devices follow which, and each
//! device's volume. Queues keep their items by track URI, so that a track is shown as the
//! library knows it at the next start, and one the library lost is dropped.

use std::collections::BTreeMap;
use std::time::Duration;

use bopsie_audio::engine::Playable;
use bopsie_core::db::{Database, Migrations};
use bopsie_core::media::Track;
use serde::{Deserialize, Serialize};

use crate::state::{PlaybackState, Queue, QueueItem, RepeatMode};

pub const MIGRATIONS: Migrations = Migrations {
    component: "queues",
    steps: &["
        CREATE TABLE queues (
            queue_id TEXT PRIMARY KEY,
            saved TEXT NOT NULL
        ) STRICT;
        CREATE TABLE groups (
            member TEXT PRIMARY KEY,
            leader TEXT NOT NULL
        ) STRICT;
        CREATE TABLE volumes (
            device TEXT PRIMARY KEY,
            volume INTEGER NOT NULL,
            muted INTEGER NOT NULL
        ) STRICT;
    "],
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedItem {
    pub queue_item_id: String,
    pub sort_index: u64,
    pub uri: String,
}

/// A queue as it is kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedQueue {
    pub items: Vec<SavedItem>,
    pub current_index: Option<usize>,
    pub shuffle_enabled: bool,
    pub repeat_mode: RepeatMode,
    /// Where in the current item it was.
    pub position_ms: u64,
}

impl SavedQueue {
    pub fn of(queue: &Queue, position: Duration) -> Self {
        Self {
            items: queue
                .items
                .iter()
                .map(|item| SavedItem {
                    queue_item_id: item.queue_item_id.clone(),
                    sort_index: item.sort_index,
                    uri: item.track.fields.uri.clone(),
                })
                .collect(),
            current_index: queue.current_index,
            shuffle_enabled: queue.shuffle_enabled,
            repeat_mode: queue.repeat_mode,
            position_ms: position.as_millis() as u64,
        }
    }

    /// The queue again, idle where it was, with each item's track as `resolve` finds it now.
    /// Items whose track is gone are dropped; if the current one is, the queue rests on the
    /// next that is left, from its start.
    pub fn restore(&self, mut resolve: impl FnMut(&str) -> Option<(Track, Playable)>) -> Queue {
        let mut current = None;
        let mut position = Duration::from_millis(self.position_ms);
        let mut items = Vec::new();
        for (index, saved) in self.items.iter().enumerate() {
            let Some((track, playable)) = resolve(&saved.uri) else {
                if Some(index) == self.current_index {
                    position = Duration::ZERO;
                }
                continue;
            };
            if current.is_none() && self.current_index.is_some_and(|c| index >= c) {
                current = Some(items.len());
            }
            items.push(QueueItem {
                queue_item_id: saved.queue_item_id.clone(),
                sort_index: saved.sort_index,
                track,
                input: playable.input,
                loudness: playable.loudness,
            });
        }
        if current.is_none() {
            position = Duration::ZERO;
        }
        let mut queue = Queue::restored(items, current, self.shuffle_enabled, self.repeat_mode);
        queue.resume_position = position;
        queue.state = PlaybackState::Idle;
        queue
    }
}

/// Everything kept.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Saved {
    pub queues: BTreeMap<String, SavedQueue>,
    /// Each member's leader.
    pub groups: BTreeMap<String, String>,
    pub volumes: BTreeMap<String, (u8, bool)>,
}

pub async fn load(db: &Database) -> bopsie_core::Result<Saved> {
    Ok(db
        .call(|conn| {
            let mut saved = Saved::default();
            let mut statement = conn.prepare("SELECT queue_id, saved FROM queues")?;
            let rows = statement.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            for row in rows {
                let (id, json) = row?;
                match serde_json::from_str(&json) {
                    Ok(queue) => {
                        saved.queues.insert(id, queue);
                    }
                    Err(error) => tracing::warn!(queue = id, %error, "cannot read a saved queue"),
                }
            }
            let mut statement = conn.prepare("SELECT member, leader FROM groups")?;
            saved.groups = statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
            let mut statement = conn.prepare("SELECT device, volume, muted FROM volumes")?;
            saved.volumes = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        (row.get::<_, u8>(1)?, row.get::<_, bool>(2)?),
                    ))
                })?
                .collect::<rusqlite::Result<_>>()?;
            Ok(saved)
        })
        .await?)
}

pub async fn save_queue(db: &Database, id: String, queue: SavedQueue) -> bopsie_core::Result<()> {
    let json = serde_json::to_string(&queue).expect("a saved queue serializes");
    db.call(move |conn| {
        conn.execute(
            "INSERT INTO queues (queue_id, saved) VALUES (?1, ?2)
             ON CONFLICT (queue_id) DO UPDATE SET saved = excluded.saved",
            (&id, &json),
        )?;
        Ok(())
    })
    .await?;
    Ok(())
}

pub async fn save_groups(
    db: &Database,
    groups: BTreeMap<String, String>,
) -> bopsie_core::Result<()> {
    db.call(move |conn| {
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM groups", [])?;
        for (member, leader) in &groups {
            tx.execute(
                "INSERT INTO groups (member, leader) VALUES (?1, ?2)",
                (member, leader),
            )?;
        }
        tx.commit()?;
        Ok(())
    })
    .await?;
    Ok(())
}

pub async fn save_volumes(
    db: &Database,
    volumes: BTreeMap<String, (u8, bool)>,
) -> bopsie_core::Result<()> {
    db.call(move |conn| {
        let tx = conn.transaction()?;
        for (device, (volume, muted)) in &volumes {
            tx.execute(
                "INSERT INTO volumes (device, volume, muted) VALUES (?1, ?2, ?3)
                 ON CONFLICT (device) DO UPDATE SET volume = excluded.volume, muted = excluded.muted",
                (device, volume, muted),
            )?;
        }
        tx.commit()?;
        Ok(())
    })
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use bopsie_core::media::{ItemFields, MediaType, Metadata};

    fn track(uri: &str) -> Track {
        Track {
            fields: ItemFields {
                item_id: uri.to_owned(),
                provider: "library".to_owned(),
                name: uri.to_owned(),
                version: String::new(),
                sort_name: uri.to_owned(),
                uri: uri.to_owned(),
                external_ids: Vec::new(),
                is_playable: true,
                media_type: MediaType::Track,
                provider_mappings: Vec::new(),
                metadata: Metadata::default(),
                favorite: false,
                position: None,
                date_added: None,
            },
            duration: 0,
            artists: Vec::new(),
            last_played: 0,
            album: None,
            disc_number: 0,
            track_number: 0,
            audio_metadata: None,
        }
    }

    fn saved(current: Option<usize>) -> SavedQueue {
        SavedQueue {
            items: ["a", "b", "c"]
                .iter()
                .enumerate()
                .map(|(i, uri)| SavedItem {
                    queue_item_id: format!("q{i}"),
                    sort_index: i as u64 + 1,
                    uri: (*uri).to_owned(),
                })
                .collect(),
            current_index: current,
            shuffle_enabled: true,
            repeat_mode: RepeatMode::All,
            position_ms: 42_000,
        }
    }

    fn resolve_all_but<'a>(
        gone: &'a [&'a str],
    ) -> impl FnMut(&str) -> Option<(Track, Playable)> + 'a {
        move |uri| {
            (!gone.contains(&uri))
                .then(|| (track(uri), PathBuf::from(format!("/new/{uri}.flac")).into()))
        }
    }

    #[test]
    fn a_queue_kept_with_its_files_paths_still_reads() {
        let kept = r#"{"items":[{"queue_item_id":"q0","sort_index":1,"uri":"a",
            "path":"/music/a.flac"}],"current_index":0,"shuffle_enabled":false,
            "repeat_mode":"off","position_ms":0}"#;
        let saved: SavedQueue = serde_json::from_str(kept).unwrap();
        assert_eq!(saved.items[0].uri, "a");
    }

    #[test]
    fn a_queue_comes_back_idle_where_it_was() {
        let queue = saved(Some(1)).restore(resolve_all_but(&[]));
        assert_eq!(queue.items.len(), 3);
        assert_eq!(queue.current_index, Some(1));
        assert_eq!(queue.resume_position, Duration::from_secs(42));
        assert_eq!(queue.state, PlaybackState::Idle);
        assert!(queue.shuffle_enabled);
        assert_eq!(queue.repeat_mode, RepeatMode::All);
        assert_eq!(
            queue.items[2].input,
            bopsie_audio::engine::Input::File(PathBuf::from("/new/c.flac")),
            "as the library has it"
        );
        assert_eq!(
            SavedQueue::of(&queue, Duration::from_secs(42)).items[0].uri,
            "a"
        );
    }

    #[test]
    fn tracks_that_are_gone_are_dropped() {
        // Before the current one: it stays current, where it was.
        let queue = saved(Some(1)).restore(resolve_all_but(&["a"]));
        assert_eq!(queue.items.len(), 2);
        assert_eq!(queue.current().unwrap().track.fields.uri, "b");
        assert_eq!(queue.resume_position, Duration::from_secs(42));
        // The current one: the next takes its place, from the start.
        let queue = saved(Some(1)).restore(resolve_all_but(&["b"]));
        assert_eq!(queue.current().unwrap().track.fields.uri, "c");
        assert_eq!(queue.resume_position, Duration::ZERO);
        // The current one and all after it: nothing is current.
        let queue = saved(Some(1)).restore(resolve_all_but(&["b", "c"]));
        assert_eq!(queue.current_index, None);
        assert_eq!(queue.resume_position, Duration::ZERO);
    }

    #[test]
    fn new_items_sort_after_restored_ones() {
        let mut queue = saved(None).restore(resolve_all_but(&[]));
        let mut rng = rand::rng();
        queue.enqueue(
            vec![(track("d"), PathBuf::from("/d.flac").into())],
            crate::QueueOption::Add,
            &mut rng,
        );
        let d = queue
            .items
            .iter()
            .find(|i| i.track.fields.uri == "d")
            .unwrap();
        assert_eq!(d.sort_index, 4);
    }
}
