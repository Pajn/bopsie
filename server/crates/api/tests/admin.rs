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

async fn players_all(client: &mut Client) -> Vec<Value> {
    client
        .result("players/all", json!({}))
        .await
        .as_array()
        .unwrap()
        .clone()
}

#[tokio::test]
async fn a_rename_reaches_every_client_and_clears_back() {
    let (server, token) = start_set_up().await;
    let _kitchen = Speaker::register(&server.players, "kitchen", "airplay", &[CD], None);
    let mut client = server.connect().await;
    client.result("auth", json!({"token": token})).await;
    until(&mut client, "players/all", json!({}), |all| {
        all.as_array().unwrap().len() == 1
    })
    .await;

    client
        .result(
            "admin/player/update",
            json!({"player_id": "kitchen", "name": "Cooking", "max_volume": 40}),
        )
        .await;
    assert_eq!(players_all(&mut client).await[0]["name"], "Cooking");
    // Others hear of it.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !client
        .events
        .iter()
        .any(|e| e["event"] == "player_updated" && e["data"]["name"] == "Cooking")
    {
        assert!(tokio::time::Instant::now() < deadline, "no player_updated");
        client.result("players/all", json!({})).await;
    }
    let devices = client.result("admin/devices", json!({})).await;
    let settings = &device(&devices, "kitchen")["settings"];
    assert_eq!(
        settings["name"],
        json!({"value": "Cooking", "layer": "saved"})
    );
    assert_eq!(
        settings["max_volume"],
        json!({"value": 40, "layer": "saved"})
    );

    // Cleared, the name is the speaker's own again; what was not mentioned stays.
    client
        .result(
            "admin/player/update",
            json!({"player_id": "kitchen", "name": null}),
        )
        .await;
    assert_eq!(players_all(&mut client).await[0]["name"], "Kitchen");
    let devices = client.result("admin/devices", json!({})).await;
    assert_eq!(device(&devices, "kitchen")["max_volume"], 40);

    // Nonsense is refused.
    let code = client
        .error_code(
            "admin/player/update",
            json!({"player_id": "kitchen", "max_volume": 101}),
        )
        .await;
    assert_ne!(code, 0);
    let code = client
        .error_code(
            "admin/player/update",
            json!({"player_id": "nobody", "name": "x"}),
        )
        .await;
    assert_ne!(code, 0);
}

#[tokio::test]
async fn endpoint_settings_turn_a_protocol_off_and_quirks_say_they_were_saved() {
    let (server, token) = start_set_up_with(10).await;
    // One speaker reached over AirPlay and over Sendspin.
    let _airplay = Speaker::register(&server.players, "ap1", "airplay", &[CD], Some("aa"));
    let _sendspin = Speaker::register(&server.players, "ss1", "sendspin", &[CD], Some("aa"));
    let mut client = server.connect().await;
    client.result("auth", json!({"token": token})).await;
    until(&mut client, "admin/devices", json!({}), |all| {
        all.as_array().unwrap().len() == 1 && all[0]["members"].as_array().unwrap().len() == 2
    })
    .await;

    client
        .result(
            "admin/endpoint/update",
            json!({"endpoint_id": "ap1", "enabled": false, "quirks": {"sync_adjust_ms": 20}}),
        )
        .await;
    let devices = client.result("admin/devices", json!({})).await;
    let airplay = &device(&devices, "ap1")["members"][0];
    assert_eq!(airplay["id"], "ap1");
    assert_eq!(airplay["disabled"], true);
    assert_eq!(airplay["available"], false);
    assert_eq!(
        airplay["settings"]["enabled"],
        json!({"value": false, "layer": "saved"})
    );
    assert_eq!(airplay["quirks"]["sync_adjust_ms"], 20);
    assert_eq!(airplay["quirk_sources"]["sync_adjust_ms"]["layer"], "saved");

    // With its AirPlay off, the speaker plays over Sendspin.
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
            json!({"queue_id": "ap1", "media": album, "option": "replace"}),
        )
        .await;
    let groups = until(&mut client, "admin/groups", json!({}), |groups| {
        groups[0]["plan"].is_object()
    })
    .await;
    assert_eq!(groups[0]["plan"]["strategy"], "sendspin_direct");
    assert_eq!(groups[0]["plan"]["carriers"][0]["endpoint"], "ss1");

    // Cleared, the quirk and the switch are gone again.
    client
        .result(
            "admin/endpoint/update",
            json!({"endpoint_id": "ap1", "enabled": null, "quirks": {"sync_adjust_ms": null}}),
        )
        .await;
    let devices = client.result("admin/devices", json!({})).await;
    let airplay = &device(&devices, "ap1")["members"][0];
    assert_eq!(airplay["disabled"], false);
    assert_eq!(airplay["quirk_sources"].get("sync_adjust_ms"), None);

    // A quirk that does not exist is refused.
    let code = client
        .error_code(
            "admin/endpoint/update",
            json!({"endpoint_id": "ap1", "quirks": {"louder": 11}}),
        )
        .await;
    assert_ne!(code, 0);
}

#[tokio::test]
async fn turning_off_a_playing_members_endpoint_drops_it_from_the_plan() {
    let (server, token) = start_set_up_with(10).await;
    let _kitchen = Speaker::register(&server.players, "kitchen", "airplay", &[CD], None);
    let _bedroom = Speaker::register(&server.players, "bedroom", "airplay", &[CD], None);
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
    until(&mut client, "admin/groups", json!({}), |groups| {
        groups[0]["plan"]["carriers"]
            .as_array()
            .is_some_and(|c| c.len() == 2)
    })
    .await;

    client
        .result(
            "admin/endpoint/update",
            json!({"endpoint_id": "bedroom", "enabled": false}),
        )
        .await;
    let groups = until(&mut client, "admin/groups", json!({}), |groups| {
        groups[0]["plan"]["carriers"]
            .as_array()
            .is_some_and(|c| c.len() == 1)
    })
    .await;
    assert_eq!(groups[0]["plan"]["carriers"][0]["endpoint"], "kitchen");
}

#[tokio::test]
async fn a_leader_whose_protocol_is_turned_off_plays_on_over_another() {
    let (server, token) = start_set_up_with(10).await;
    let _airplay = Speaker::register(&server.players, "ap1", "airplay", &[CD], Some("aa"));
    let _sendspin = Speaker::register(&server.players, "ss1", "sendspin", &[CD], Some("aa"));
    let mut client = server.connect().await;
    client.result("auth", json!({"token": token})).await;
    until(&mut client, "admin/devices", json!({}), |all| {
        all.as_array().unwrap().len() == 1 && all[0]["members"].as_array().unwrap().len() == 2
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
            json!({"queue_id": "ap1", "media": album, "option": "replace"}),
        )
        .await;
    until(&mut client, "admin/groups", json!({}), |groups| {
        groups[0]["plan"]["strategy"] == "airplay_direct"
    })
    .await;

    client
        .result(
            "admin/endpoint/update",
            json!({"endpoint_id": "ap1", "enabled": false}),
        )
        .await;
    until(&mut client, "admin/groups", json!({}), |groups| {
        groups[0]["plan"]["strategy"] == "sendspin_direct"
    })
    .await;
    let queue = client
        .result("player_queues/get", json!({"queue_id": "ap1"}))
        .await;
    assert_eq!(queue["state"], "playing");
}
