//! What is played is counted, and the library lists the most played first.

mod common;

use std::time::Duration;

use serde_json::{Value, json};

use common::speaker::*;
use common::*;

fn names(items: &Value) -> Vec<String> {
    items
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["name"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn what_is_played_most_comes_first() {
    let (server, token) = start_set_up_with(4).await;
    let _kitchen = Speaker::register(&server.players, "kitchen", "airplay", &[CD], None);
    let mut client = server.connect().await;
    client.result("auth", json!({"token": token})).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while client
        .result("players/all", json!({}))
        .await
        .as_array()
        .unwrap()
        .is_empty()
    {
        assert!(tokio::time::Instant::now() < deadline, "no player");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let most_played = async |client: &mut Client, kind: &str| {
        names(
            &client
                .result(
                    &format!("music/{kind}/library_items"),
                    json!({"order_by": "play_count_desc"}),
                )
                .await,
        )
    };
    // Nothing played yet: the newest first.
    let albums = client
        .result(
            "music/albums/library_items",
            json!({"order_by": "play_count_desc"}),
        )
        .await;
    let last = albums.as_array().unwrap().last().unwrap().clone();
    let artist = last["artists"][0]["name"].as_str().unwrap().to_owned();
    assert_ne!(most_played(&mut client, "artists").await[0], artist);

    // Its first track, half of it heard, counts for it, its album and its artist.
    client
        .result(
            "player_queues/play_media",
            json!({"queue_id": "kitchen", "media": last["uri"], "option": "replace"}),
        )
        .await;
    while most_played(&mut client, "albums").await[0] != last["name"] {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the album never counted"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(most_played(&mut client, "artists").await[0], artist);
    let track = client
        .result(
            "music/albums/album_tracks",
            json!({
                "item_id": last["item_id"], "provider_instance_id_or_domain": "library",
            }),
        )
        .await[0]["name"]
        .clone();
    assert_eq!(most_played(&mut client, "tracks").await[0], track);
}
