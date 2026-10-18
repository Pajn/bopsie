//! Tests against a running Bopsie server, the throwaway one the mobile integration tests use:
//!
//! ```sh
//! mobile/scripts/test-server.sh start
//! eval "$(mobile/scripts/test-server.sh env)"
//! cargo test
//! ```
//!
//! Each test returns early, passing, when `TEST_SERVER_URL` is not set. The player starts muted.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use tokio::sync::watch;

use crate::api::CommandSocket;
use crate::{
    ConnectionState, Error, PRODUCT_NAME, Player, PlayerConfig, Remote, SignIn, Snapshot, sign_in,
};

struct Server {
    url: String,
    username: String,
    password: String,
}

fn server() -> Option<Server> {
    let Ok(url) = std::env::var("TEST_SERVER_URL") else {
        eprintln!("TEST_SERVER_URL is not set; skipping a test against the server");
        return None;
    };
    Some(Server {
        url,
        username: std::env::var("TEST_SERVER_USERNAME").unwrap_or_else(|_| "bopsie".to_owned()),
        password: std::env::var("TEST_SERVER_PASSWORD")
            .unwrap_or_else(|_| "bopsie-test".to_owned()),
    })
}

fn unique(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{prefix}-{nanos}")
}

fn start(signed_in: &SignIn, token: &str, client_id: &str) -> Player {
    Player::start(PlayerConfig {
        server_url: signed_in.server_url.clone(),
        token: token.to_owned(),
        client_id: client_id.to_owned(),
        name: "Integration test player".to_owned(),
        product_name: PRODUCT_NAME.to_owned(),
        software_version: None,
        volume: 100,
        muted: true,
    })
    .unwrap()
}

async fn wait_for(
    changes: &mut watch::Receiver<Snapshot>,
    what: &str,
    done: impl Fn(&Snapshot) -> bool,
) -> Snapshot {
    let waited = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let snapshot = changes.borrow_and_update().clone();
            if done(&snapshot) {
                return snapshot;
            }
            changes.changed().await.expect("the player stopped");
        }
    })
    .await;
    waited.unwrap_or_else(|_| panic!("timed out waiting for {what}: {:?}", changes.borrow()))
}

async fn stop(player: Player) {
    tokio::task::spawn_blocking(move || player.stop())
        .await
        .unwrap();
}

#[tokio::test]
async fn a_wrong_password_is_reported_as_such() {
    let Some(server) = server() else { return };
    let error = sign_in(&server.url, &server.username, "not the password", "Test")
        .await
        .unwrap_err();
    assert!(matches!(error, Error::WrongCredentials(_)), "{error:?}");
}

#[tokio::test]
async fn a_rejected_token_stops_reconnecting() {
    let Some(server) = server() else { return };
    let signed_in = sign_in(&server.url, &server.username, &server.password, "Test")
        .await
        .unwrap();
    let player = start(&signed_in, "not a token", &unique("bopsie-player-test"));
    wait_for(&mut player.watch(), "unauthorized", |s| {
        s.connection == ConnectionState::Unauthorized
    })
    .await;
    stop(player).await;
}

#[tokio::test]
async fn plays_what_the_api_plays_to_it_and_pauses_from_the_controller() {
    let Some(server) = server() else { return };
    let signed_in = sign_in(
        &server.url,
        &server.username,
        &server.password,
        "Integration test",
    )
    .await
    .unwrap();
    let client_id = unique("bopsie-player-test");
    let player = start(&signed_in, &signed_in.token, &client_id);
    let mut changes = player.watch();
    wait_for(&mut changes, "the connection", |s| {
        s.connection == ConnectionState::Connected
    })
    .await;

    let mut api = CommandSocket::open(&signed_in.server_url).await.unwrap();
    api.authenticate(&signed_in.token).await.unwrap();
    let albums = api
        .command(
            "music/albums/library_items",
            json!({ "limit": 500, "offset": 0, "order_by": "sort_name" }),
        )
        .await
        .unwrap();
    let uri = albums
        .as_array()
        .and_then(|albums| {
            albums
                .iter()
                .find(|album| album["name"] == "First Album")
                .and_then(|album| album["uri"].as_str())
        })
        .map(str::to_owned)
        .expect("the test library has First Album");
    api.command(
        "player_queues/play_media",
        json!({ "queue_id": client_id, "media": uri, "option": "replace" }),
    )
    .await
    .unwrap();

    let playing = wait_for(&mut changes, "the first track", |s| {
        s.playing
            && s.now_playing
                .as_ref()
                .is_some_and(|n| n.title.as_deref() == Some("Opening"))
    })
    .await;
    let now_playing = playing.now_playing.unwrap();
    assert_eq!(now_playing.album.as_deref(), Some("First Album"));
    assert!(now_playing.progress.is_some_and(|p| p.duration_ms > 0));
    wait_for(&mut changes, "the controls", |s| s.controls.is_some()).await;

    player.remote(Remote::Pause);
    wait_for(&mut changes, "the pause", |s| !s.playing).await;
    let queue = api
        .command("player_queues/get", json!({ "queue_id": client_id }))
        .await
        .unwrap();
    assert_eq!(queue["state"], Value::from("paused"), "{queue}");

    api.close().await;
    stop(player).await;
}
