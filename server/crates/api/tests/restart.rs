//! What outlasts a restart: queues, where they were, groups and volumes, over one database.

mod common;

use std::sync::Arc;
use std::time::Duration;

use bopsie_core::db::Database;
use bopsie_core::events::EventBus;
use bopsie_core::media::MediaType;
use bopsie_library::{Library, ListQuery};
use bopsie_players::{Devices, Players};
use bopsie_providers_filesystem::FilesystemProvider;
use bopsie_queues::{Config, Playback, QueueOption};

use common::speaker::*;

struct Run {
    playback: Playback,
    players: Players,
    library: Library,
}

async fn run(dir: &std::path::Path) -> Run {
    let db = Database::open(
        &dir.join("bopsie.db"),
        &[
            bopsie_library::MIGRATIONS,
            bopsie_players::devices::MIGRATIONS,
            bopsie_queues::store::MIGRATIONS,
        ],
    )
    .unwrap();
    let events = EventBus::default();
    let library = Library::new(
        db.clone(),
        events.clone(),
        vec![Arc::new(FilesystemProvider::new(dir.join("music")))],
        dir.join("artwork"),
    );
    library.sync().await.unwrap();
    let players = Players::default();
    let devices = Devices::new(players.clone(), db.clone()).await.unwrap();
    let playback = Playback::start(devices, events, Config::default());
    playback.persist(db, library.clone()).await.unwrap();
    Run {
        playback,
        players,
        library,
    }
}

async fn eventually(what: &str, mut holds: impl FnMut() -> bool) {
    for _ in 0..250 {
        if holds() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("never: {what}");
}

#[tokio::test(flavor = "multi_thread")]
async fn queues_groups_and_volumes_outlast_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    bopsie_fixtures::write_library(&dir.path().join("music"), 10).unwrap();

    let first = run(dir.path()).await;
    let kitchen = Speaker::register(&first.players, "kitchen", "airplay", &[CD], None);
    let _bedroom = Speaker::register(&first.players, "bedroom", "airplay", &[CD], None);
    eventually("the first bedroom", || {
        first.playback.player("bedroom", None).is_some()
    })
    .await;
    let albums = first
        .library
        .list(
            MediaType::Album,
            ListQuery {
                search: Some("First Album".into()),
                ..ListQuery::default()
            },
        )
        .await
        .unwrap();
    let tracks = first
        .library
        .playable(&albums[0].fields().uri)
        .await
        .unwrap();
    first
        .playback
        .play_media("kitchen", tracks, QueueOption::Replace, None)
        .unwrap();
    first.playback.group("bedroom", "kitchen").unwrap();
    first.playback.volume_set("kitchen", 55).unwrap();
    eventually("playing a second", || {
        first.playback.queue("kitchen").unwrap()["elapsed_time"]
            .as_f64()
            .unwrap()
            > 1.0
    })
    .await;
    first.playback.save(true).await;
    let elapsed = first.playback.queue("kitchen").unwrap()["elapsed_time"]
        .as_f64()
        .unwrap();
    drop(kitchen);

    let second = run(dir.path()).await;
    let kitchen = Speaker::register(&second.players, "kitchen", "airplay", &[CD], None);
    let _bedroom = Speaker::register(&second.players, "bedroom", "airplay", &[CD], None);
    eventually("the bedroom grouped again", || {
        second
            .playback
            .player("bedroom", None)
            .is_some_and(|p| p["synced_to"] == "kitchen")
    })
    .await;
    let queue = second.playback.queue("kitchen").unwrap();
    assert_eq!(queue["items"], 3);
    assert_eq!(queue["state"], "idle", "nothing plays on its own");
    assert_eq!(queue["current_index"], 0);
    let resumed = queue["elapsed_time"].as_f64().unwrap();
    assert!((resumed - elapsed).abs() < 0.5, "{resumed} vs {elapsed}");
    assert_eq!(*kitchen.volume.lock().unwrap(), 55);
    drop(first);
}
