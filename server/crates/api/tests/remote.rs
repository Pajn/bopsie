//! A speaker's own controls act on the group it plays in.

mod common;

use std::time::Duration;

use bopsie_players::{Remote, Repeat};
use serde_json::{Value, json};

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

/// Waits until `holds` is true.
async fn eventually(what: &str, holds: impl Fn() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !holds() {
        assert!(tokio::time::Instant::now() < deadline, "{what} never held");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn queue_state(client: &mut Client, state: &'static str) {
    until(
        client,
        "player_queues/get",
        json!({"queue_id": "kitchen"}),
        |q| q["state"] == state,
    )
    .await;
}

#[tokio::test]
async fn a_button_on_any_speaker_controls_its_whole_group() {
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
    queue_state(&mut client, "playing").await;

    // Pause pressed on the member pauses the group.
    server.players.remote("bedroom", Remote::Pause);
    queue_state(&mut client, "paused").await;
    // Play pressed on it resumes the group.
    server.players.remote("bedroom", Remote::Play);
    queue_state(&mut client, "playing").await;
    // A play that changes nothing, as a speaker echoing the server's own, is ignored.
    server.players.remote("kitchen", Remote::Play);
    tokio::time::sleep(Duration::from_millis(200)).await;
    queue_state(&mut client, "playing").await;

    // Next skips the group's queue.
    let before = client
        .result("player_queues/get", json!({"queue_id": "kitchen"}))
        .await["current_index"]
        .clone();
    server.players.remote("bedroom", Remote::Next);
    until(
        &mut client,
        "player_queues/get",
        json!({"queue_id": "kitchen"}),
        |q| q["current_index"] != before,
    )
    .await;

    // Volume buttons move the speaker's own volume.
    let volume = *bedroom.volume.lock().unwrap();
    server.players.remote("bedroom", Remote::VolumeUp);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while *bedroom.volume.lock().unwrap() != volume + 5 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the volume never moved"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    // A controller sets the group's volume, mute, repeat and shuffle, and seeks.
    server.players.remote("bedroom", Remote::Volume(60));
    eventually("the group volume", || {
        *kitchen.volume.lock().unwrap() == 60 || *bedroom.volume.lock().unwrap() == 60
    })
    .await;
    server.players.remote("bedroom", Remote::Mute(true));
    eventually("the group muted", || {
        *kitchen.muted.lock().unwrap() && *bedroom.muted.lock().unwrap()
    })
    .await;
    server
        .players
        .remote("bedroom", Remote::Repeat(Repeat::All));
    until(
        &mut client,
        "player_queues/get",
        json!({"queue_id": "kitchen"}),
        |q| q["repeat_mode"] == "all",
    )
    .await;
    server.players.remote("bedroom", Remote::Shuffle(true));
    until(
        &mut client,
        "player_queues/get",
        json!({"queue_id": "kitchen"}),
        |q| q["shuffle_enabled"] == true,
    )
    .await;
    // Every member is told the group's state.
    eventually("the controls", || {
        bedroom.controls.lock().unwrap().is_some_and(|c| {
            c.muted && c.repeat == Repeat::All && c.shuffle && c.seek_max_ms.is_some()
        })
    })
    .await;
    server.players.remote("bedroom", Remote::Seek(2_000));
    until(
        &mut client,
        "player_queues/get",
        json!({"queue_id": "kitchen"}),
        |q| {
            q["elapsed_time"]
                .as_f64()
                .is_some_and(|t| (2.0..4.0).contains(&t))
        },
    )
    .await;

    // A member switched to another source leaves the group, which plays on.
    server.players.remote("bedroom", Remote::Released);
    until(
        &mut client,
        "players/get",
        json!({"player_id": "bedroom"}),
        |p| p["synced_to"].is_null(),
    )
    .await;
    queue_state(&mut client, "playing").await;
}

#[tokio::test]
async fn a_speaker_the_server_moved_to_another_protocol_is_not_taken_for_released() {
    let (server, token) = start_set_up_with(10).await;
    // A speaker reached over AirPlay and Sendspin, as a WiiM is over LinkPlay and Cast.
    let _airplay = Speaker::register(&server.players, "kitchen", "airplay", &[CD], Some("aa"));
    let sendspin = Speaker::register(&server.players, "kitchen-ss", "sendspin", &[CD], Some("aa"));
    let phone = Speaker::register(&server.players, "phone", "sendspin", &[CD], None);
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
    queue_state(&mut client, "playing").await;

    // The phone joins, which the speaker plays with over Sendspin: it leaves AirPlay, and its
    // AirPlay endpoint says so, as LinkPlay does when the WiiM goes to its Cast receiver.
    client
        .result(
            "players/cmd/group",
            json!({"player_id": "phone", "target_player": "kitchen"}),
        )
        .await;
    eventually("the speaker over Sendspin", || sendspin.frames() > 0).await;
    eventually("the phone", || phone.frames() > 0).await;
    server.players.remote("kitchen", Remote::Released);
    tokio::time::sleep(Duration::from_millis(300)).await;
    queue_state(&mut client, "playing").await;

    // Released on the endpoint it plays through still pauses it.
    server.players.remote("kitchen-ss", Remote::Released);
    queue_state(&mut client, "paused").await;
}
