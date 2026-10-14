//! Sources, such as Spotify Connect, playing to a player's group in place of its queue.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use bopsie_players::Remote;
use bopsie_sources::{Control, Happened};
use serde_json::{Value, json};

use common::source::FakeSource;
use common::speaker::*;
use common::*;

async fn until(
    client: &mut Client,
    command: &str,
    args: Value,
    holds: impl Fn(&Value) -> bool,
) -> Value {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let result = client.result(command, args.clone()).await;
        if holds(&result) {
            return result;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "{command} never held: {result}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn eventually(what: &str, holds: impl Fn() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !holds() {
        assert!(tokio::time::Instant::now() < deadline, "{what} never held");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Pushes audio into `source` as it would arrive, until the returned flag is cleared.
fn produce(source: &Arc<FakeSource>) -> Arc<AtomicBool> {
    let running = Arc::new(AtomicBool::new(true));
    let (source, flag) = (source.clone(), running.clone());
    std::thread::spawn(move || {
        while flag.load(Ordering::SeqCst) {
            source.push(50);
            std::thread::sleep(Duration::from_millis(50));
        }
    });
    running
}

#[tokio::test(flavor = "multi_thread")]
async fn a_source_plays_to_its_players_group_in_place_of_the_queue() {
    let (server, token) = start_set_up_with(10).await;
    let kitchen = Speaker::register(&server.players, "kitchen", "airplay", &[CD], None);
    let bedroom = Speaker::register(&server.players, "bedroom", "airplay", &[CD], None);
    let mut client = server.connect().await;
    client.result("auth", json!({"token": token})).await;
    until(&mut client, "players/all", json!({}), |all| {
        all.as_array().unwrap().len() == 2
    })
    .await;
    let album = client
        .result("music/albums/library_items", json!({}))
        .await
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == "First Album")
        .unwrap()["uri"]
        .clone();
    client
        .result(
            "player_queues/play_media",
            json!({"queue_id": "kitchen", "media": album, "option": "replace"}),
        )
        .await;
    client
        .result(
            "players/cmd/group",
            json!({"player_id": "bedroom", "target_player": "kitchen"}),
        )
        .await;
    until(
        &mut client,
        "player_queues/get",
        json!({"queue_id": "kitchen"}),
        |q| q["state"] == "playing",
    )
    .await;

    // Picked for the bedroom, the source plays to the bedroom's whole group.
    let source = FakeSource::register(&server.sources, "bedroom");
    let bedroom_player = until(
        &mut client,
        "players/get",
        json!({"player_id": "bedroom"}),
        |p| p["source_list"].as_array().is_some_and(|l| !l.is_empty()),
    )
    .await;
    assert_eq!(bedroom_player["source_list"][0]["id"], "fake--bedroom");
    source.start("A Spotify Song", 200_000);
    let producing = produce(&source);
    let queue = until(
        &mut client,
        "player_queues/get",
        json!({"queue_id": "kitchen"}),
        |q| q["active"] == false,
    )
    .await;
    assert_eq!(queue["state"], "paused", "the queue rests");
    let player = until(
        &mut client,
        "players/get",
        json!({"player_id": "kitchen"}),
        |p| p["active_source"] == "fake--bedroom",
    )
    .await;
    assert_eq!(player["current_media"]["title"], "A Spotify Song");
    assert_eq!(player["playback_state"], "playing");
    let (k, b) = (kitchen.frames(), bedroom.frames());
    eventually("both speakers play the source", || {
        kitchen.frames() > k + 4_410 && bedroom.frames() > b + 4_410
    })
    .await;

    // The group's controls act on the source.
    server.players.remote("kitchen", Remote::Pause);
    client
        .result("player_queues/next", json!({"queue_id": "kitchen"}))
        .await;
    client
        .result(
            "player_queues/seek",
            json!({"queue_id": "kitchen", "position": 30}),
        )
        .await;
    eventually("the controls reach the source", || {
        let controls = source.controls();
        controls.contains(&Control::Pause)
            && controls.contains(&Control::Next)
            && controls.contains(&Control::Seek(30_000))
    })
    .await;

    // The source's own volume moves the group's, and the group's is told to the source.
    source.happen(Happened::Volume(30));
    eventually("the group follows the source's volume", || {
        *kitchen.volume.lock().unwrap() == 30 && *bedroom.volume.lock().unwrap() == 30
    })
    .await;
    client
        .result(
            "players/cmd/group_volume",
            json!({"player_id": "kitchen", "volume_level": 20}),
        )
        .await;
    eventually("the source hears of the group's volume", || {
        source.controls().contains(&Control::Volume(20))
    })
    .await;

    // Paused at the source, the speakers stop at once, and the source goes back by what they
    // had not played.
    source.happen(Happened::Paused);
    until(
        &mut client,
        "players/get",
        json!({"player_id": "kitchen"}),
        |p| p["playback_state"] == "paused",
    )
    .await;
    eventually("the source is told what went unheard", || {
        source
            .unheard
            .lock()
            .unwrap()
            .first()
            .is_some_and(|d| *d > Duration::from_millis(100))
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let (k, b) = (kitchen.frames(), bedroom.frames());
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!((kitchen.frames(), bedroom.frames()), (k, b), "silent");

    // Playing on, it plays to the group again.
    source.happen(Happened::Playing);
    eventually("the group plays the source again", || {
        kitchen.frames() > k + 4_410
    })
    .await;

    // Playing from the queue takes the group back.
    client
        .result(
            "player_queues/play_media",
            json!({"queue_id": "kitchen", "media": album, "option": "replace"}),
        )
        .await;
    eventually("the source is let go", || {
        source.controls().contains(&Control::Release)
    })
    .await;
    let player = until(
        &mut client,
        "players/get",
        json!({"player_id": "kitchen"}),
        |p| p["active_source"] == "kitchen",
    )
    .await;
    assert_eq!(player["current_media"]["title"], "Opening");
    until(
        &mut client,
        "player_queues/get",
        json!({"queue_id": "kitchen"}),
        |q| q["active"] == true && q["state"] == "playing",
    )
    .await;

    // A source that ends leaves the group with its queue, paused.
    source.start("Another Song", 100_000);
    until(
        &mut client,
        "players/get",
        json!({"player_id": "kitchen"}),
        |p| p["active_source"] == "fake--bedroom",
    )
    .await;
    source.happen(Happened::Ended);
    let player = until(
        &mut client,
        "players/get",
        json!({"player_id": "kitchen"}),
        |p| p["active_source"] == "kitchen",
    )
    .await;
    assert_eq!(player["playback_state"], "paused");
    producing.store(false, Ordering::SeqCst);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_speaker_that_joins_a_group_playing_a_source_plays_it_too() {
    let (server, token) = start_set_up_with(10).await;
    let _kitchen = Speaker::register(&server.players, "kitchen", "airplay", &[CD], None);
    let bedroom = Speaker::register(&server.players, "bedroom", "airplay", &[CD], None);
    let mut client = server.connect().await;
    client.result("auth", json!({"token": token})).await;
    until(&mut client, "players/all", json!({}), |all| {
        all.as_array().unwrap().len() == 2
    })
    .await;
    let source = FakeSource::register(&server.sources, "kitchen");
    source.start("A Spotify Song", 200_000);
    let producing = produce(&source);
    until(
        &mut client,
        "players/get",
        json!({"player_id": "kitchen"}),
        |p| p["active_source"] == "fake--kitchen" && p["playback_state"] == "playing",
    )
    .await;
    assert_eq!(bedroom.frames(), 0);
    client
        .result(
            "players/cmd/group",
            json!({"player_id": "bedroom", "target_player": "kitchen"}),
        )
        .await;
    eventually("the bedroom plays the source", || bedroom.frames() > 4_410).await;
    let player = until(
        &mut client,
        "players/get",
        json!({"player_id": "bedroom"}),
        |p| p["active_source"] == "fake--kitchen",
    )
    .await;
    assert_eq!(player["current_media"]["title"], "A Spotify Song");
    producing.store(false, Ordering::SeqCst);
}
