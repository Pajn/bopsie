//! Playing to Sendspin players through the API, as Bopsie's players, queue and Sendspin
//! suites do: a phone that joins over `/sendspin`, two speakers grouped and ungrouped, a queue
//! moved between them, and the queue sheet's edits.

mod common;

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

use common::*;

/// A Sendspin client that keeps everything it is sent.
struct Player {
    messages: Arc<Mutex<Vec<Value>>>,
    /// Audio chunks: timestamp and length.
    chunks: Arc<Mutex<Vec<(i64, usize)>>>,
    outgoing: tokio::sync::mpsc::UnboundedSender<Message>,
}

impl Player {
    /// Joins as Bopsie's phone does: `/sendspin` with the API token, hello, time and state.
    async fn join(addr: SocketAddr, token: &str, id: &str, name: &str) -> Self {
        let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/sendspin"))
            .await
            .unwrap();
        let (mut sink, mut stream) = socket.split();
        let (outgoing, mut outbox) = tokio::sync::mpsc::unbounded_channel::<Message>();
        tokio::spawn(async move {
            while let Some(message) = outbox.recv().await {
                if sink.send(message).await.is_err() {
                    break;
                }
            }
        });
        let messages = Arc::new(Mutex::new(Vec::new()));
        let chunks = Arc::new(Mutex::new(Vec::new()));
        let (m, c) = (messages.clone(), chunks.clone());
        tokio::spawn(async move {
            while let Some(Ok(message)) = stream.next().await {
                match message {
                    Message::Text(text) => {
                        m.lock().unwrap().push(serde_json::from_str(&text).unwrap())
                    }
                    Message::Binary(bytes) => {
                        let timestamp = i64::from_be_bytes(bytes[1..9].try_into().unwrap());
                        c.lock().unwrap().push((timestamp, bytes.len()));
                    }
                    _ => {}
                }
            }
        });
        let player = Self {
            messages,
            chunks,
            outgoing,
        };
        player.send(json!({"type": "auth", "token": token, "client_id": id}));
        player.message("auth_ok", |_| true).await;
        let format = json!({"codec": "pcm", "channels": 2, "sample_rate": 48000, "bit_depth": 16});
        let support = json!({"supported_formats": [format], "buffer_capacity": 1_048_576,
            "supported_commands": ["volume", "mute"]});
        player.send(json!({"type": "client/hello", "payload": {
            "client_id": id, "name": name, "version": 1,
            "supported_roles": ["player@v1", "controller@v1", "metadata@v1"],
            "device_info": {"product_name": "Mobile Application", "manufacturer": "Bopsie", "software_version": "0"},
            "player@v1_support": support, "player_support": support}}));
        player.message("server/hello", |_| true).await;
        player.send(json!({"type": "client/time", "payload": {"client_transmitted": 1}}));
        player.message("server/time", |_| true).await;
        player.send(
            json!({"type": "client/state", "payload": {"available": true, "state": "synchronized",
            "player": {"volume": 50, "muted": false, "static_delay_ms": 0,
                "required_lead_time_ms": 300, "min_buffer_ms": 300}}}),
        );
        player
    }

    fn send(&self, message: Value) {
        let _ = self.outgoing.send(Message::text(message.to_string()));
    }

    /// Waits for a message of `kind` that `matches`.
    async fn message(&self, kind: &str, matches: impl Fn(&Value) -> bool) -> Value {
        eventually(|| {
            self.messages
                .lock()
                .unwrap()
                .iter()
                .find(|m| m["type"] == kind && matches(&m["payload"]))
                .cloned()
        })
        .await
        .unwrap_or_else(|| panic!("no {kind}"))
    }

    fn count(&self, kind: &str) -> usize {
        self.messages
            .lock()
            .unwrap()
            .iter()
            .filter(|m| m["type"] == kind)
            .count()
    }

    fn chunk_count(&self) -> usize {
        self.chunks.lock().unwrap().len()
    }
}

/// Polls `check` until it gives something, for up to ten seconds.
async fn eventually<T>(mut check: impl FnMut() -> Option<T>) -> Option<T> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(found) = check() {
            return Some(found);
        }
        if tokio::time::Instant::now() > deadline {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Polls a command until its result satisfies `holds`.
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
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn album_uri(client: &mut Client, name: &str) -> String {
    let albums = client.result("music/albums/library_items", json!({})).await;
    albums
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == name)
        .unwrap()["uri"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn signed_in(server: &Server, token: &str) -> Client {
    let mut client = server.connect().await;
    client.result("auth", json!({"token": token})).await;
    client
}

#[tokio::test]
async fn a_phone_joins_plays_pauses_and_resumes() {
    let (server, token) = start_set_up_with(10).await;
    let phone = Player::join(server.addr, &token, "phone", "Test phone").await;
    let mut client = signed_in(&server, &token).await;

    let player = until(
        &mut client,
        "players/get",
        json!({"player_id": "phone"}),
        |p| p["available"] == true,
    )
    .await;
    assert_eq!(player["type"], "player");
    assert_eq!(player["private"], false, "the phone's own user sees it");
    assert_eq!(
        client
            .result("player_queues/get", json!({"queue_id": "phone"}))
            .await["queue_id"],
        "phone"
    );

    let album = album_uri(&mut client, "First Album").await;
    client
        .result(
            "player_queues/play_media",
            json!({"queue_id": "phone", "media": album, "option": "replace"}),
        )
        .await;
    let start = phone.message("stream/start", |_| true).await;
    assert_eq!(
        start["payload"]["player"],
        json!({"codec": "pcm", "sample_rate": 48000, "channels": 2, "bit_depth": 16})
    );
    let metadata = phone
        .message("server/state", |p| p["metadata"]["title"].is_string())
        .await;
    assert_eq!(metadata["payload"]["metadata"]["title"], "Opening");
    assert_eq!(metadata["payload"]["metadata"]["album"], "First Album");
    phone
        .message("group/update", |p| p["playback_state"] == "playing")
        .await;
    assert!(phone.chunk_count() > 0);

    let queue = until(
        &mut client,
        "player_queues/get",
        json!({"queue_id": "phone"}),
        |q| q["state"] == "playing" && q["elapsed_time"].as_f64().unwrap() > 0.5,
    )
    .await;
    assert_eq!(queue["current_item"]["name"], "Test Artist - Opening");
    assert_eq!(queue["items"], 3);

    client
        .result("player_queues/pause", json!({"queue_id": "phone"}))
        .await;
    phone.message("stream/end", |_| true).await;
    let paused = client
        .result("player_queues/get", json!({"queue_id": "phone"}))
        .await;
    assert_eq!(paused["state"], "paused");
    let at = paused["elapsed_time"].as_f64().unwrap();
    assert!(at > 0.5, "{at}");

    client
        .result("player_queues/play", json!({"queue_id": "phone"}))
        .await;
    eventually(|| (phone.count("stream/start") == 2).then_some(()))
        .await
        .unwrap();
    let resumed = until(
        &mut client,
        "player_queues/get",
        json!({"queue_id": "phone"}),
        |q| q["state"] == "playing",
    )
    .await;
    assert_eq!(resumed["current_index"], 0);
    assert!(
        resumed["elapsed_time"].as_f64().unwrap() >= at - 0.1,
        "resumes where it paused"
    );
}

#[tokio::test]
async fn speakers_group_ungroup_and_take_over_a_queue() {
    let (server, token) = start_set_up_with(10).await;
    let kitchen = Player::join(server.addr, &token, "kitchen", "Kitchen").await;
    let bedroom = Player::join(server.addr, &token, "bedroom", "Bedroom").await;
    let mut client = signed_in(&server, &token).await;
    let both = until(&mut client, "players/all", json!({}), |all| {
        all.as_array()
            .unwrap()
            .iter()
            .filter(|p| p["available"] == true)
            .count()
            == 2
    })
    .await;
    let kitchen_player = both
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["player_id"] == "kitchen")
        .unwrap();
    assert!(
        kitchen_player["supported_features"]
            .as_array()
            .unwrap()
            .contains(&json!("set_members"))
    );
    assert_eq!(kitchen_player["can_group_with"], json!(["bedroom"]));

    let album = album_uri(&mut client, "First Album").await;
    client
        .result(
            "player_queues/play_media",
            json!({"queue_id": "kitchen", "media": album}),
        )
        .await;
    kitchen.message("stream/start", |_| true).await;

    client
        .result(
            "players/cmd/group",
            json!({"player_id": "bedroom", "target_player": "kitchen"}),
        )
        .await;
    let member = client
        .result("players/get", json!({"player_id": "bedroom"}))
        .await;
    assert_eq!(member["synced_to"], "kitchen");
    assert_eq!(
        member["playback_state"], "playing",
        "a member shows its group's playback"
    );
    let leader = client
        .result("players/get", json!({"player_id": "kitchen"}))
        .await;
    assert_eq!(leader["group_members"], json!(["kitchen", "bedroom"]));
    let active = client
        .result(
            "player_queues/get_active_queue",
            json!({"player_id": "bedroom"}),
        )
        .await;
    assert_eq!(active["queue_id"], "kitchen");

    // The member joins the stream the leader plays.
    bedroom.message("stream/start", |_| true).await;
    eventually(|| (bedroom.chunk_count() > 10).then_some(()))
        .await
        .unwrap();
    let kitchen_chunks = kitchen.chunks.lock().unwrap().clone();
    let bedroom_chunks = bedroom.chunks.lock().unwrap().clone();
    let shared = bedroom_chunks[5];
    assert!(
        kitchen_chunks.contains(&shared),
        "both play the same chunk at the same time"
    );

    client
        .result("players/cmd/ungroup", json!({"player_id": "bedroom"}))
        .await;
    bedroom.message("stream/end", |_| true).await;
    let member = client
        .result("players/get", json!({"player_id": "bedroom"}))
        .await;
    assert_eq!(member["synced_to"], Value::Null);
    let leader = client
        .result("players/get", json!({"player_id": "kitchen"}))
        .await;
    assert_eq!(leader["group_members"], json!([]));
    assert_eq!(leader["playback_state"], "playing", "the rest play on");

    client
        .result(
            "player_queues/transfer",
            json!({"source_queue_id": "kitchen", "target_queue_id": "bedroom"}),
        )
        .await;
    let moved = until(
        &mut client,
        "player_queues/get",
        json!({"queue_id": "bedroom"}),
        |q| q["items"] == 3 && q["state"] == "playing",
    )
    .await;
    assert_eq!(moved["current_item"]["name"], "Test Artist - Opening");
    assert_eq!(
        client
            .result("player_queues/get", json!({"queue_id": "kitchen"}))
            .await["items"],
        0
    );
    kitchen.message("stream/end", |_| true).await;
    eventually(|| (bedroom.count("stream/start") >= 2).then_some(()))
        .await
        .unwrap();

    client
        .result(
            "players/cmd/volume_set",
            json!({"player_id": "bedroom", "volume_level": 70}),
        )
        .await;
    let command = bedroom.message("server/command", |_| true).await;
    assert_eq!(
        command["payload"]["player"],
        json!({"command": "volume", "volume": 70})
    );
}

#[tokio::test]
async fn the_queue_sheet_edits_around_what_the_player_holds() {
    let (server, token) = start_set_up_with(10).await;
    let _phone = Player::join(server.addr, &token, "phone", "Test phone").await;
    let mut client = signed_in(&server, &token).await;
    until(
        &mut client,
        "players/get",
        json!({"player_id": "phone"}),
        |p| p["available"] == true,
    )
    .await;
    let first = album_uri(&mut client, "First Album").await;
    let second = album_uri(&mut client, "Second Album").await;
    client
        .result(
            "player_queues/play_media",
            json!({"queue_id": "phone", "media": [first, second], "option": "replace"}),
        )
        .await;
    until(
        &mut client,
        "player_queues/get",
        json!({"queue_id": "phone"}),
        |q| q["state"] == "playing",
    )
    .await;
    client
        .result("player_queues/stop", json!({"queue_id": "phone"}))
        .await;
    let queue = until(
        &mut client,
        "player_queues/get",
        json!({"queue_id": "phone"}),
        |q| q["state"] == "idle",
    )
    .await;

    let items = client
        .result(
            "player_queues/items",
            json!({"queue_id": "phone", "limit": 500, "offset": 0}),
        )
        .await;
    let ids: Vec<String> = items
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["queue_item_id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(ids.len(), 6);
    // What the player holds, as Bopsie's queue sheet works it out.
    let first_free = queue["current_index"]
        .as_i64()
        .unwrap()
        .max(queue["index_in_buffer"].as_i64().unwrap_or(-1))
        + 1;
    assert_eq!(first_free, 2, "the current item and the one handed over");

    let error = client
        .send(
            "player_queues/move_item",
            json!({"queue_id": "phone", "queue_item_id": ids[0], "pos_shift": 1}),
        )
        .await;
    assert!(error.get("error_code").is_some(), "{error}");

    let last = ids[5].clone();
    client
        .result(
            "player_queues/move_item",
            json!({"queue_id": "phone", "queue_item_id": last, "pos_shift": -3}),
        )
        .await;
    let items = client
        .result("player_queues/items", json!({"queue_id": "phone"}))
        .await;
    assert_eq!(items[2]["queue_item_id"], last.as_str());

    let gone = items[3]["queue_item_id"].as_str().unwrap().to_owned();
    client
        .result(
            "player_queues/delete_item",
            json!({"queue_id": "phone", "item_id_or_index": gone}),
        )
        .await;
    let items = client
        .result("player_queues/items", json!({"queue_id": "phone"}))
        .await;
    assert_eq!(items.as_array().unwrap().len(), 5);

    let target = items[4]["queue_item_id"].as_str().unwrap().to_owned();
    client
        .result(
            "player_queues/play_index",
            json!({"queue_id": "phone", "index": target}),
        )
        .await;
    until(
        &mut client,
        "player_queues/get",
        json!({"queue_id": "phone"}),
        |q| q["current_item"]["queue_item_id"] == target.as_str(),
    )
    .await;
}
