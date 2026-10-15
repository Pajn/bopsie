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
    assert_eq!(
        settings["spotify_connect"],
        json!({"value": true, "layer": "default"})
    );
    client
        .result(
            "admin/player/update",
            json!({"player_id": "kitchen", "spotify_connect": false}),
        )
        .await;
    let devices = client.result("admin/devices", json!({})).await;
    assert_eq!(
        device(&devices, "kitchen")["settings"]["spotify_connect"],
        json!({"value": false, "layer": "saved"})
    );
    assert_eq!(device(&devices, "kitchen")["sources"], json!([]));

    // The Spotify name follows the pattern, unless the player has its own.
    assert_eq!(
        device(&devices, "kitchen")["spotify_name"],
        "Cooking (Bopsie)"
    );
    client
        .result(
            "admin/player/update",
            json!({"player_id": "kitchen", "spotify_name": "Cooking"}),
        )
        .await;
    let devices = client.result("admin/devices", json!({})).await;
    assert_eq!(device(&devices, "kitchen")["spotify_name"], "Cooking");
    assert_eq!(
        device(&devices, "kitchen")["spotify_default_name"],
        "Cooking (Bopsie)"
    );
    assert_eq!(
        device(&devices, "kitchen")["settings"]["spotify_name"],
        json!({"value": "Cooking", "layer": "saved"})
    );
    client
        .result(
            "admin/player/update",
            json!({"player_id": "kitchen", "spotify_name": ""}),
        )
        .await;
    let devices = client.result("admin/devices", json!({})).await;
    assert_eq!(
        device(&devices, "kitchen")["spotify_name"],
        "Cooking (Bopsie)"
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

#[tokio::test]
async fn the_server_holds_a_player_at_its_maximum_volume() {
    let (server, token) = start_set_up().await;
    let kitchen = Speaker::register(&server.players, "kitchen", "airplay", &[CD], None);
    let bedroom = Speaker::register(&server.players, "bedroom", "airplay", &[CD], None);
    let mut client = server.connect().await;
    client.result("auth", json!({"token": token})).await;
    until(&mut client, "players/all", json!({}), |all| {
        all.as_array().unwrap().len() == 2
    })
    .await;
    let volume = |s: &Speaker| *s.volume.lock().unwrap();
    assert_eq!(volume(&kitchen), 40);

    // A maximum below where it is turns it down at once.
    client
        .result(
            "admin/player/update",
            json!({"player_id": "kitchen", "max_volume": 30}),
        )
        .await;
    eventually(|| volume(&kitchen) == 30).await;

    // Setting it louder stops at the maximum.
    client
        .result(
            "players/cmd/volume_set",
            json!({"player_id": "kitchen", "volume_level": 80}),
        )
        .await;
    assert_eq!(volume(&kitchen), 30);

    // Turned up by something else, it is turned back down.
    *kitchen.volume.lock().unwrap() = 90;
    server.players.changed("kitchen");
    eventually(|| volume(&kitchen) == 30).await;

    // In a group, each member stops at its own maximum.
    client
        .result(
            "players/cmd/group",
            json!({"player_id": "bedroom", "target_player": "kitchen"}),
        )
        .await;
    client
        .result(
            "players/cmd/group_volume",
            json!({"player_id": "kitchen", "volume_level": 80}),
        )
        .await;
    assert_eq!((volume(&kitchen), volume(&bedroom)), (30, 80));

    // Without a maximum it goes where it is set.
    client
        .result(
            "admin/player/update",
            json!({"player_id": "kitchen", "max_volume": null}),
        )
        .await;
    client
        .result(
            "players/cmd/volume_set",
            json!({"player_id": "kitchen", "volume_level": 80}),
        )
        .await;
    assert_eq!(volume(&kitchen), 80);
}

async fn eventually(mut holds: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !holds() {
        assert!(tokio::time::Instant::now() < deadline, "never held");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn an_album_edited_by_an_admin_reaches_every_client() {
    let (server, token) = start_set_up().await;
    let mut client = server.connect().await;
    client.result("auth", json!({"token": token})).await;
    let albums = client.result("music/albums/library_items", json!({})).await;
    let first = albums
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == "First Album")
        .unwrap()["item_id"]
        .as_str()
        .unwrap()
        .to_owned();

    client
        .result(
            "admin/library/update",
            json!({"media_type": "album", "item_id": first, "changes": {"name": "Debut", "year": 1999}}),
        )
        .await;
    let album = client
        .result(
            "music/albums/get",
            json!({"item_id": first, "provider_instance_id_or_domain": "library"}),
        )
        .await;
    assert_eq!(
        (album["name"].clone(), album["year"].clone()),
        (json!("Debut"), json!(1999))
    );
    assert_eq!(
        client
            .result(
                "admin/library/edits",
                json!({"media_type": "album", "item_id": first})
            )
            .await,
        json!({"name": "Debut", "year": 1999})
    );
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !client
        .events
        .iter()
        .any(|e| e["event"] == "media_item_updated" && e["data"]["name"] == "Debut")
    {
        assert!(
            tokio::time::Instant::now() < deadline,
            "no media_item_updated"
        );
        client.result("auth/me", json!({})).await;
    }

    let code = client
        .error_code(
            "admin/library/update",
            json!({"media_type": "album", "item_id": first, "changes": {"bpm": 120}}),
        )
        .await;
    assert_eq!(code, 3, "invalid data");
}

#[tokio::test]
async fn suggestions_decisions_and_separating_are_for_admins() {
    let (server, token) = start_set_up().await;
    let mut client = server.connect().await;
    client.result("auth", json!({"token": token})).await;
    assert_eq!(
        client
            .result("admin/library/suggestions", json!({"all": true}))
            .await,
        json!([])
    );
    let code = client
        .error_code(
            "admin/library/decide",
            json!({"id": 1, "decision": "merge"}),
        )
        .await;
    assert_eq!(code, 2, "no such suggestion");
    let code = client
        .error_code(
            "admin/library/decide",
            json!({"id": 1, "decision": "maybe"}),
        )
        .await;
    assert_eq!(code, 3, "invalid data");
    let albums = client.result("music/albums/library_items", json!({})).await;
    let first = albums[0]["item_id"].as_str().unwrap().to_owned();
    assert_eq!(
        client
            .result("admin/library/release_suggestions", json!({}))
            .await,
        json!([])
    );
    let code = client
        .error_code(
            "admin/library/decide_release",
            json!({"item_id": first, "decision": "accept"}),
        )
        .await;
    assert_eq!(code, 2, "nothing suggested");
    // Looked up again at once, as if never; an album that is not there cannot be.
    client
        .result("admin/library/refresh_metadata", json!({"item_id": first}))
        .await;
    let code = client
        .error_code(
            "admin/library/refresh_metadata",
            json!({"item_id": "999999"}),
        )
        .await;
    assert_eq!(code, 2, "no such album");
    // Nothing decided, nothing to reconsider.
    let code = client
        .error_code("admin/library/reconsider", json!({"id": 1}))
        .await;
    assert_eq!(code, 2, "no such decision");
    let code = client
        .error_code(
            "admin/library/reconsider_release",
            json!({"item_id": first}),
        )
        .await;
    assert_eq!(code, 2, "no decision on a release");
    assert_eq!(
        client
            .result("admin/library/duplicate_files", json!({}))
            .await,
        json!([])
    );
    let server = client.result("admin/server", json!({})).await;
    assert_eq!(
        server["library"]["metadata"]["albums"],
        albums.as_array().unwrap().len()
    );
    assert_eq!(server["library"]["metadata"]["checked"], 0);
    // Held by the files alone, there is nothing to separate.
    assert_eq!(
        client
            .result(
                "admin/library/separate",
                json!({"item_id": first, "provider_instance": "streamed"}),
            )
            .await,
        json!({"item_id": null})
    );
}

fn png() -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbImage::from_pixel(4, 4, image::Rgb([200, 30, 30]))
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    bytes
}

async fn upload(
    server: &Server,
    token: Option<&str>,
    body: Vec<u8>,
) -> (axum::http::StatusCode, Value) {
    let mut request = axum::http::Request::post("/artwork");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let (status, body, _) = server
        .request(request.body(axum::body::Body::from(body)).unwrap())
        .await;
    (status, body)
}

#[tokio::test]
async fn uploaded_artwork_becomes_an_albums_image() {
    use axum::http::StatusCode;
    let (server, token) = start_set_up().await;

    assert_eq!(
        upload(&server, None, png()).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        upload(&server, Some(&token), b"not an image".to_vec())
            .await
            .0,
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
    let mut broken = png();
    broken.truncate(30);
    assert_eq!(
        upload(&server, Some(&token), broken).await.0,
        StatusCode::BAD_REQUEST
    );
    let mut huge = png();
    huge.resize(11 * 1024 * 1024, 0);
    assert_eq!(
        upload(&server, Some(&token), huge).await.0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    // Only admins upload.
    let bob = server
        .api
        .auth
        .create_user("bob", "battery staple", "user", None)
        .await
        .unwrap();
    let bob_token = server
        .api
        .auth
        .issue(
            &bob.user_id,
            "test",
            bopsie_api::auth::lifetime::TokenKind::Session,
        )
        .await
        .unwrap();
    assert_eq!(
        upload(&server, Some(&bob_token), png()).await.0,
        StatusCode::FORBIDDEN
    );

    let (status, answer) = upload(&server, Some(&token), png()).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let proxy_id = answer["proxy_id"].as_str().unwrap().to_owned();
    let mut client = server.connect().await;
    client.result("auth", json!({"token": token})).await;
    let first = client
        .result("music/albums/library_items", json!({}))
        .await
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == "First Album")
        .unwrap()["item_id"]
        .clone();
    client
        .result(
            "admin/library/update",
            json!({"media_type": "album", "item_id": first, "changes": {"image": proxy_id}}),
        )
        .await;
    let album = client
        .result(
            "music/albums/get",
            json!({"item_id": first, "provider_instance_id_or_domain": "library"}),
        )
        .await;
    assert_eq!(album["metadata"]["images"][0]["proxy_id"], proxy_id);
    let (status, _) = server.get(&format!("/imageproxy/{proxy_id}?size=80")).await;
    assert_eq!(status, StatusCode::OK);
}
