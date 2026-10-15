//! Catalogs, such as Spotify: searched beside the library, browsed and played as they are.

mod common;

use std::time::Duration;

use serde_json::{Value, json};

use common::catalog::{FAKE, FakeCatalog};
use common::speaker::*;
use common::*;

fn names(results: &Value, kind: &str) -> Vec<String> {
    results[kind]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            format!(
                "{}:{}",
                item["provider"].as_str().unwrap(),
                item["name"].as_str().unwrap()
            )
        })
        .collect()
}

#[tokio::test]
async fn search_covers_the_library_a_catalog_or_both() {
    let (server, token) = start_set_up().await;
    let dir = tempfile::tempdir().unwrap();
    FakeCatalog::add_to(&server.api.library, dir.path());
    let mut client = server.connect().await;
    client.result("auth", json!({"token": token})).await;

    let search = |extra: Value| {
        let mut args = json!({"search_query": "alpha", "media_types": ["track"]});
        args.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        args
    };
    // Both, the library's first.
    let both = client.result("music/search", search(json!({}))).await;
    assert_eq!(names(&both, "tracks"), ["library:Alpha", "fake:Alpha"]);
    let library = client
        .result("music/search", search(json!({"library_only": true})))
        .await;
    assert_eq!(names(&library, "tracks"), ["library:Alpha"]);
    let library = client
        .result("music/search", search(json!({"providers": ["library"]})))
        .await;
    assert_eq!(names(&library, "tracks"), ["library:Alpha"]);
    let fake = client
        .result("music/search", search(json!({"providers": [FAKE]})))
        .await;
    assert_eq!(names(&fake, "tracks"), ["fake:Alpha"]);

    let providers = client.result("providers", json!({})).await;
    let streaming: Vec<_> = providers
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["is_streaming_provider"] == true)
        .map(|p| p["instance_id"].clone())
        .collect();
    assert_eq!(streaming, [json!(FAKE)]);
    assert_eq!(providers.as_array().unwrap().len(), 2, "{providers}");
}

#[tokio::test]
async fn a_catalogs_items_browse_and_play() {
    let (server, token) = start_set_up_with(10).await;
    let dir = tempfile::tempdir().unwrap();
    FakeCatalog::add_to(&server.api.library, dir.path());
    let kitchen = Speaker::register(&server.players, "kitchen", "airplay", &[CD], None);
    let mut client = server.connect().await;
    client.result("auth", json!({"token": token})).await;

    let item = |id: &str| json!({"item_id": id, "provider_instance_id_or_domain": FAKE});
    let album = client.result("music/albums/get", item("fa1")).await;
    assert_eq!(album["name"], "Fake Album");
    assert_eq!(album["uri"], "fake://album/fa1");
    let tracks = client
        .result("music/albums/album_tracks", item("fa1"))
        .await;
    assert_eq!(tracks.as_array().unwrap().len(), 2);
    let albums = client
        .result("music/artists/artist_albums", item("fr1"))
        .await;
    assert_eq!(albums[0]["name"], "Fake Album");
    let tracks = client
        .result("music/playlists/playlist_tracks", item("fp1"))
        .await;
    assert_eq!(tracks[0]["name"], "Alpha");
    let by_uri = client
        .result("music/item_by_uri", json!({"uri": "fake://artist/fr1"}))
        .await;
    assert_eq!(by_uri["name"], "Fake Artist");
    assert_eq!(
        client.error_code("music/albums/get", item("nothing")).await,
        2
    );
    assert_eq!(
        client
            .error_code(
                "music/albums/get",
                json!({"item_id": "1", "provider_instance_id_or_domain": "elsewhere"})
            )
            .await,
        2
    );

    // Played from the catalog, as a queue plays the library.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
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
    client
        .result(
            "player_queues/play_media",
            json!({"queue_id": "kitchen", "media": "fake://album/fa1", "option": "replace"}),
        )
        .await;
    let queue = client
        .result("player_queues/get", json!({"queue_id": "kitchen"}))
        .await;
    assert_eq!(queue["items"], 2);
    assert_eq!(queue["current_item"]["name"], "Fake Artist - Fake One");
    while kitchen.frames() < 44_100 {
        assert!(tokio::time::Instant::now() < deadline, "nothing played");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
