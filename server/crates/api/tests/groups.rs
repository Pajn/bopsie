//! Groups as the planner allows them, through the API: speakers of one protocol group, one
//! that cannot play over the group's protocol or format is refused with the reason, a group
//! changes how it plays for a member that joins, and a speaker reached over two protocols is
//! one player.

mod common;

use std::sync::atomic::Ordering;
use std::time::Duration;

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

async fn eventually(mut holds: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !holds() {
        assert!(tokio::time::Instant::now() < deadline, "never held");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn signed_in(server: &Server, token: &str) -> Client {
    let mut client = server.connect().await;
    client.result("auth", json!({"token": token})).await;
    client
}

async fn play_album(client: &mut Client, queue_id: &str) {
    let albums = client.result("music/albums/library_items", json!({})).await;
    let album = albums
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == "First Album")
        .unwrap()["uri"]
        .clone();
    client
        .result(
            "player_queues/play_media",
            json!({"queue_id": queue_id, "media": album, "option": "replace"}),
        )
        .await;
}

async fn player(client: &mut Client, id: &str) -> Value {
    client.result("players/get", json!({"player_id": id})).await
}

#[tokio::test]
async fn speakers_group_across_protocols_and_others_are_refused_with_the_reason() {
    let (server, token) = start_set_up_with(10).await;
    let kitchen = Speaker::register(&server.players, "kitchen", "airplay", &[CD], None);
    let bedroom = Speaker::register(&server.players, "bedroom", "airplay", &[CD], None);
    let phone = Speaker::register(&server.players, "phone", "sendspin", &[DAT, CD], None);
    // A speaker the server cannot stream to, nor group by its own multiroom.
    let _radio = Speaker::register(&server.players, "radio", "linkplay", &[CD], None);
    let mut client = signed_in(&server, &token).await;

    let leader = until(
        &mut client,
        "players/get",
        json!({"player_id": "kitchen"}),
        |p| p["can_group_with"] == json!(["bedroom", "phone"]),
    )
    .await;
    assert!(
        leader["supported_features"]
            .as_array()
            .unwrap()
            .contains(&json!("set_members"))
    );
    assert_eq!(
        player(&mut client, "radio").await["can_group_with"],
        json!([]),
        "it is streamed over neither protocol"
    );

    play_album(&mut client, "kitchen").await;
    eventually(|| kitchen.frames() > 0).await;
    client
        .result(
            "players/cmd/group",
            json!({"player_id": "bedroom", "target_player": "kitchen"}),
        )
        .await;
    eventually(|| bedroom.frames() > 0).await;
    assert_eq!(
        player(&mut client, "kitchen").await["group_members"],
        json!(["kitchen", "bedroom"])
    );

    // The phone joins over Sendspin, and the speakers play on undisturbed.
    client
        .result(
            "players/cmd/group",
            json!({"player_id": "phone", "target_player": "kitchen"}),
        )
        .await;
    eventually(|| phone.frames() > 0).await;
    assert_eq!(
        player(&mut client, "kitchen").await["group_members"],
        json!(["kitchen", "bedroom", "phone"])
    );
    let outputs = |s: &Speaker| s.outputs.load(Ordering::SeqCst);
    assert_eq!((outputs(&kitchen), outputs(&bedroom)), (1, 1));

    let refused = client
        .send(
            "players/cmd/group",
            json!({"player_id": "radio", "target_player": "kitchen"}),
        )
        .await;
    assert_eq!(refused["error_code"], 9, "{refused}");
    assert_eq!(
        refused["details"],
        "Radio can't join Kitchen: it can't be streamed to over AirPlay or Sendspin"
    );
    assert_eq!(player(&mut client, "radio").await["synced_to"], Value::Null);

    // Group volume moves them all by the same proportion, without disturbing the stream.
    client
        .result(
            "players/cmd/volume_set",
            json!({"player_id": "bedroom", "volume_level": 20}),
        )
        .await;
    client
        .result(
            "players/cmd/group_volume",
            json!({"player_id": "kitchen", "volume_level": 80}),
        )
        .await;
    assert_eq!(*kitchen.volume.lock().unwrap(), 80);
    assert_eq!(*bedroom.volume.lock().unwrap(), 40);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!((outputs(&kitchen), outputs(&bedroom)), (1, 1));

    // The leader leaves: the other takes the queue and plays on.
    client
        .result("players/cmd/ungroup", json!({"player_id": "kitchen"}))
        .await;
    let queue = until(
        &mut client,
        "player_queues/get",
        json!({"queue_id": "bedroom"}),
        |q| q["state"] == "playing",
    )
    .await;
    assert_eq!(queue["items"], 3);
    let leader = player(&mut client, "kitchen").await;
    assert_eq!(leader["group_members"], json!([]));
    assert_eq!(leader["active_source"], "kitchen");
}

#[tokio::test]
async fn a_member_that_cannot_play_the_groups_format_is_refused() {
    let (server, token) = start_set_up_with(10).await;
    let _phone = Speaker::register(&server.players, "phone", "sendspin", &[DAT], None);
    let _old = Speaker::register(&server.players, "old", "sendspin", &[CD], None);
    let mut client = signed_in(&server, &token).await;
    until(&mut client, "players/all", json!({}), |all| {
        all.as_array().unwrap().len() == 2
    })
    .await;
    let refused = client
        .send(
            "players/cmd/group",
            json!({"player_id": "old", "target_player": "phone"}),
        )
        .await;
    assert_eq!(refused["error_code"], 9, "{refused}");
    assert_eq!(
        refused["details"],
        "Old can't join Phone: it can't play the group's 48000 Hz 2-channel audio"
    );
}

#[tokio::test]
async fn a_speaker_over_two_protocols_is_one_player() {
    let (server, token) = start_set_up_with(10).await;
    let lounge_airplay =
        Speaker::register(&server.players, "aplounge", "airplay", &[CD], Some("aa"));
    let lounge_sendspin = Speaker::register(
        &server.players,
        "lounge",
        "sendspin",
        &[DAT, CD],
        Some("AA"),
    );
    let _phone = Speaker::register(&server.players, "phone", "sendspin", &[DAT], None);
    let mut client = signed_in(&server, &token).await;

    let all = until(&mut client, "players/all", json!({}), |all| {
        all.as_array().unwrap().len() == 2
    })
    .await;
    let ids: Vec<&str> = all
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["player_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["aplounge", "phone"], "the speaker took its first id");
    let lounge = &all[0];
    assert_eq!(
        lounge["provider"], "airplay",
        "shown as its preferred protocol"
    );
    let protocols: Vec<&str> = lounge["output_protocols"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["output_protocol_id"].as_str().unwrap())
        .collect();
    assert_eq!(protocols, ["aplounge", "lounge"]);
    assert_eq!(lounge["can_group_with"], json!(["phone"]), "over Sendspin");

    let with_protocols = client
        .result("players/all", json!({"return_protocol_players": true}))
        .await;
    let endpoint = with_protocols
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["player_id"] == "lounge")
        .expect("its Sendspin endpoint on its own");
    assert_eq!(endpoint["type"], "protocol");

    // Alone it plays over AirPlay; joined by the phone, the group moves to Sendspin and
    // plays on.
    play_album(&mut client, "aplounge").await;
    eventually(|| lounge_airplay.frames_at(44_100) > 0).await;
    assert_eq!(
        player(&mut client, "aplounge").await["active_output_protocol"],
        "aplounge"
    );
    client
        .result(
            "players/cmd/group",
            json!({"player_id": "phone", "target_player": "aplounge"}),
        )
        .await;
    eventually(|| lounge_sendspin.frames_at(48_000) > 0).await;
    let leader = until(
        &mut client,
        "players/get",
        json!({"player_id": "aplounge"}),
        |p| p["active_output_protocol"] == "lounge",
    )
    .await;
    assert_eq!(leader["group_members"], json!(["aplounge", "phone"]));
    assert_eq!(leader["playback_state"], "playing");
}

#[tokio::test]
async fn an_output_that_fails_rejoins_its_group() {
    let (server, token) = start_set_up_with(10).await;
    let kitchen = Speaker::register(&server.players, "kitchen", "airplay", &[CD], None);
    let bedroom = Speaker::register(&server.players, "bedroom", "airplay", &[CD], None);
    let mut client = signed_in(&server, &token).await;
    until(&mut client, "players/all", json!({}), |all| {
        all.as_array().unwrap().len() == 2
    })
    .await;
    play_album(&mut client, "kitchen").await;
    client
        .result(
            "players/cmd/group",
            json!({"player_id": "bedroom", "target_player": "kitchen"}),
        )
        .await;
    eventually(|| bedroom.frames() > 0).await;

    bedroom.trouble.fail.store(true, Ordering::SeqCst);
    eventually(|| bedroom.outputs.load(Ordering::SeqCst) == 2).await;
    let rejoined = bedroom.frames();
    eventually(|| bedroom.frames() > rejoined).await;
    assert_eq!(
        kitchen.outputs.load(Ordering::SeqCst),
        1,
        "the rest play on"
    );
    assert_eq!(player(&mut client, "bedroom").await["synced_to"], "kitchen");
}

#[tokio::test]
async fn a_crashed_engine_plays_on_from_where_it_was() {
    let (server, token) = start_set_up_with(10).await;
    let kitchen = Speaker::register(&server.players, "kitchen", "airplay", &[CD], None);
    let mut client = signed_in(&server, &token).await;
    until(&mut client, "players/all", json!({}), |all| {
        all.as_array().unwrap().len() == 1
    })
    .await;
    play_album(&mut client, "kitchen").await;
    let playing = until(
        &mut client,
        "player_queues/get",
        json!({"queue_id": "kitchen"}),
        |q| q["state"] == "playing" && q["elapsed_time"].as_f64().unwrap() > 1.0,
    )
    .await;
    let before = playing["elapsed_time"].as_f64().unwrap();

    kitchen.trouble.panic.store(true, Ordering::SeqCst);
    eventually(|| kitchen.outputs.load(Ordering::SeqCst) == 2).await;
    let after = until(
        &mut client,
        "player_queues/get",
        json!({"queue_id": "kitchen"}),
        |q| q["state"] == "playing",
    )
    .await;
    assert_eq!(after["current_index"], 0);
    assert!(
        after["elapsed_time"].as_f64().unwrap() >= before,
        "from where it was, not the start"
    );
}

#[tokio::test]
async fn a_group_whose_leader_goes_pauses() {
    let (server, token) = start_set_up_with(10).await;
    let kitchen = Speaker::register(&server.players, "kitchen", "airplay", &[CD], None);
    let mut client = signed_in(&server, &token).await;
    until(&mut client, "players/all", json!({}), |all| {
        all.as_array().unwrap().len() == 1
    })
    .await;
    play_album(&mut client, "kitchen").await;
    until(
        &mut client,
        "player_queues/get",
        json!({"queue_id": "kitchen"}),
        |q| q["state"] == "playing",
    )
    .await;
    kitchen.available.store(false, Ordering::SeqCst);
    server.players.changed("kitchen");
    until(
        &mut client,
        "player_queues/get",
        json!({"queue_id": "kitchen"}),
        |q| q["state"] == "paused",
    )
    .await;
    // Back within a minute, it plays on.
    kitchen.available.store(true, Ordering::SeqCst);
    server.players.changed("kitchen");
    until(
        &mut client,
        "player_queues/get",
        json!({"queue_id": "kitchen"}),
        |q| q["state"] == "playing",
    )
    .await;
}
