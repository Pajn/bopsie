//! A speaker's own controls act on the group it plays in.

mod common;

use std::time::Duration;

use bopsie_players::Remote;
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
    let _kitchen = Speaker::register(&server.players, "kitchen", "airplay", &[CD], None);
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
