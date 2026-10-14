//! The admin views: devices with everything known of their endpoints, how groups play, and the
//! server itself.

mod common;

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

fn device<'a>(devices: &'a Value, id: &str) -> &'a Value {
    devices
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["id"] == id)
        .unwrap_or_else(|| panic!("no {id} in {devices}"))
}

#[tokio::test]
async fn devices_and_groups_show_what_the_server_knows() {
    let (server, token) = start_set_up_with(10).await;
    let _kitchen = Speaker::register(
        &server.players,
        "kitchen",
        "airplay",
        &[CD],
        Some("aabbcc001122"),
    );
    let _bedroom = Speaker::register(&server.players, "bedroom", "airplay", &[CD], None);
    let mut client = server.connect().await;
    client.result("auth", json!({"token": token})).await;

    let devices = until(&mut client, "admin/devices", json!({}), |all| {
        all.as_array().unwrap().len() == 2
    })
    .await;
    let kitchen = device(&devices, "kitchen");
    assert_eq!(kitchen["name"], "Kitchen");
    assert_eq!(kitchen["available"], true);
    let endpoint = &kitchen["members"][0];
    assert_eq!(endpoint["id"], "kitchen");
    assert_eq!(endpoint["provider"], "airplay");
    assert_eq!(endpoint["facts"]["mac"], "aabbcc001122");
    assert_eq!(
        endpoint["capabilities"]["formats"],
        json!([{"sample_rate": 44100, "channels": 2, "bits": 16}])
    );
    assert_eq!(endpoint["health"]["level"], "good");
    assert!(endpoint["quirk_sources"].is_object(), "{endpoint}");
    assert_eq!(endpoint["diagnostics"], Value::Null, "a stand-in has none");

    // Nothing plays yet.
    assert_eq!(client.result("admin/groups", json!({})).await, json!([]));

    // Play to the kitchen with the bedroom following.
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
    let groups = until(&mut client, "admin/groups", json!({}), |groups| {
        groups[0]["plan"]["carriers"]
            .as_array()
            .is_some_and(|c| c.len() == 2)
    })
    .await;
    let group = &groups[0];
    assert_eq!(group["leader"], "kitchen");
    assert_eq!(group["members"], json!(["bedroom"]));
    let plan = &group["plan"];
    assert_eq!(plan["strategy"], "airplay_direct");
    assert_eq!(plan["sample_rate"], 44100);
    assert_eq!(plan["carriers"][0]["endpoint"], "kitchen");
    assert_eq!(plan["carriers"][1]["endpoint"], "bedroom");
    assert_eq!(plan["carriers"][1]["native"], false);
    assert_eq!(plan["dropped"], json!([]));
}

#[tokio::test]
async fn the_server_view_counts_the_library_and_its_sync() {
    let (server, token) = start_set_up().await;
    let mut client = server.connect().await;
    client.result("auth", json!({"token": token})).await;

    let view = client.result("admin/server", json!({})).await;
    assert_eq!(view["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(view["providers"]["sendspin"], true);
    assert_eq!(view["providers"]["airplay"], false);
    assert_eq!(view["config"]["server"]["name"], "Bopsie");
    let library = &view["library"];
    assert_eq!(library["providers"].as_array().unwrap().len(), 1);
    assert!(library["tracks"].as_u64().unwrap() > 0, "{library}");
    // The scan before the server was set up was a sync.
    let sync = &library["last_sync"];
    assert!(sync["added"].as_u64().unwrap() > 0, "{sync}");
    assert_eq!(sync["failed"], json!([]));
}
