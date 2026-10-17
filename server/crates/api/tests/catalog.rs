//! Catalogs, such as Spotify: searched beside the library, browsed and played as they are.

mod common;

use std::time::Duration;

use serde_json::{Value, json};

use bopsie_library::catalog::Progress;
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

#[tokio::test]
async fn a_catalogs_audiobooks_list_search_and_play_as_one_item() {
    let (server, token) = start_set_up_with(10).await;
    let dir = tempfile::tempdir().unwrap();
    FakeCatalog::add_to(&server.api.library, dir.path());
    let kitchen = Speaker::register(&server.players, "kitchen", "airplay", &[CD], None);
    let mut client = server.connect().await;
    client.result("auth", json!({"token": token})).await;

    let books = client
        .result("music/audiobooks/library_items", json!({}))
        .await;
    assert_eq!(books.as_array().unwrap().len(), 1);
    assert_eq!(books[0]["name"], "The Fake Book");
    assert_eq!(books[0]["media_type"], "audiobook");
    assert_eq!(books[0]["narrators"], json!(["Fake Narrator"]));
    assert_eq!(
        client
            .result("music/audiobooks/library_items", json!({"offset": 1}))
            .await,
        json!([])
    );
    assert_eq!(client.result("music/audiobooks/count", json!({})).await, 1);
    let book = client
        .result(
            "music/audiobooks/get",
            json!({"item_id": "fb1", "provider_instance_id_or_domain": FAKE}),
        )
        .await;
    assert_eq!(book["authors"][0]["name"], "Fake Artist");
    assert_eq!(book["metadata"]["chapters"][1]["start"], 2.0);
    let found = client
        .result(
            "music/search",
            json!({"search_query": "fake book", "media_types": ["audiobook"]}),
        )
        .await;
    assert_eq!(names(&found, "audiobooks"), ["fake:The Fake Book"]);

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
            json!({"queue_id": "kitchen", "media": "fake://audiobook/fb1", "option": "replace"}),
        )
        .await;
    let queue = client
        .result("player_queues/get", json!({"queue_id": "kitchen"}))
        .await;
    assert_eq!(queue["items"], 1, "one item, however many chapters");
    let item = &queue["current_item"]["media_item"];
    assert_eq!(item["media_type"], "audiobook");
    assert_eq!(item["metadata"]["chapters"].as_array().unwrap().len(), 2);
    while kitchen.frames() < 44_100 {
        assert!(tokio::time::Instant::now() < deadline, "nothing played");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let player = client
        .result("players/get", json!({"player_id": "kitchen"}))
        .await;
    assert_eq!(player["current_media"]["media_type"], "audiobook");
}

#[tokio::test]
async fn a_book_starts_where_listening_left_off_and_its_progress_is_reported() {
    let (server, token) = start_set_up_with(10).await;
    let dir = tempfile::tempdir().unwrap();
    let reports = FakeCatalog::add_to(&server.api.library, dir.path());
    let _kitchen = Speaker::register(&server.players, "kitchen", "airplay", &[CD], None);
    let mut client = server.connect().await;
    client.result("auth", json!({"token": token})).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
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
            json!({"queue_id": "kitchen", "media": "fake://audiobook/fb1", "option": "replace"}),
        )
        .await;
    let reported = |what: fn(&Progress) -> bool| {
        reports
            .lock()
            .unwrap()
            .iter()
            .any(|(id, progress)| id == "fb1" && what(progress))
    };
    // It was left 1 s in.
    while !reported(|p| matches!(p, Progress::Playing(_))) {
        assert!(
            tokio::time::Instant::now() < deadline,
            "no progress reported"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let first = reports.lock().unwrap()[0].1;
    let Progress::Playing(at) = first else {
        panic!("{first:?}");
    };
    assert!(at >= Duration::from_millis(1_000), "started at {at:?}");
    client
        .result("player_queues/pause", json!({"queue_id": "kitchen"}))
        .await;
    while !reported(|p| matches!(p, Progress::Paused(_))) {
        assert!(tokio::time::Instant::now() < deadline, "no pause reported");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    client
        .result("player_queues/clear", json!({"queue_id": "kitchen"}))
        .await;
    while !reported(|p| matches!(p, Progress::Stopped(_) | Progress::Ended)) {
        assert!(
            tokio::time::Instant::now() < deadline,
            "leaving it not reported"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn next_and_previous_move_by_chapter_within_a_book() {
    let (server, token) = start_set_up_with(10).await;
    let dir = tempfile::tempdir().unwrap();
    FakeCatalog::add_to(&server.api.library, dir.path());
    let _kitchen = Speaker::register(&server.players, "kitchen", "airplay", &[CD], None);
    let mut client = server.connect().await;
    client.result("auth", json!({"token": token})).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
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
            json!({"queue_id": "kitchen", "media": "fake://audiobook/fb1", "option": "replace"}),
        )
        .await;
    let queue = async |client: &mut common::Client| {
        client
            .result("player_queues/get", json!({"queue_id": "kitchen"}))
            .await
    };
    // Heard from where it was left, 1 s into the first chapter.
    loop {
        let q = queue(&mut client).await;
        if q["state"] == "playing" && q["buffering"] == false {
            assert_eq!(q["current_chapter"], 0);
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "never played: {q}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    client
        .result("player_queues/pause", json!({"queue_id": "kitchen"}))
        .await;
    client
        .result("player_queues/next", json!({"queue_id": "kitchen"}))
        .await;
    let q = queue(&mut client).await;
    assert_eq!(q["current_chapter"], 1, "{q}");
    assert_eq!(q["elapsed_time"], 2.0);
    assert_eq!(q["items"], 1, "the same book");
    // Within 3 s of the chapter's start, previous goes to the chapter before.
    client
        .result("player_queues/previous", json!({"queue_id": "kitchen"}))
        .await;
    let q = queue(&mut client).await;
    assert_eq!(q["current_chapter"], 0, "{q}");
    assert_eq!(q["elapsed_time"], 0.0);
}

#[tokio::test]
async fn a_book_plays_faster_and_as_fast_again_the_next_time() {
    let (server, token) = start_set_up_with(10).await;
    let dir = tempfile::tempdir().unwrap();
    FakeCatalog::add_to(&server.api.library, dir.path());
    let _kitchen = Speaker::register(&server.players, "kitchen", "airplay", &[CD], None);
    let mut client = server.connect().await;
    client.result("auth", json!({"token": token})).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
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
    let play_book =
        json!({"queue_id": "kitchen", "media": "fake://audiobook/fb1", "option": "replace"});
    client
        .result("player_queues/play_media", play_book.clone())
        .await;
    let queue = async |client: &mut common::Client| {
        client
            .result("player_queues/get", json!({"queue_id": "kitchen"}))
            .await
    };
    loop {
        let q = queue(&mut client).await;
        if q["state"] == "playing" && q["buffering"] == false {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "never played: {q}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    client
        .result(
            "player_queues/set_playback_speed",
            json!({"queue_id": "kitchen", "speed": 1.5}),
        )
        .await;
    let q = queue(&mut client).await;
    assert_eq!(q["playback_speed"], 1.5);
    // Its time runs at 1.5×, once heard again.
    let heard = loop {
        let q = queue(&mut client).await;
        if q["state"] == "playing" && q["buffering"] == false {
            break q["elapsed_time"].as_f64().unwrap();
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "never played again: {q}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    tokio::time::sleep(Duration::from_millis(600)).await;
    let later = queue(&mut client).await["elapsed_time"].as_f64().unwrap();
    let moved = later - heard;
    assert!((0.75..1.2).contains(&moved), "moved {moved} s in 0.6 s");

    let refused = client
        .send(
            "player_queues/set_playback_speed",
            json!({"queue_id": "kitchen", "speed": 4.0}),
        )
        .await;
    assert!(refused["error_code"].is_number(), "{refused}");
    // Queued again, it plays as fast as it last did.
    client.result("player_queues/play_media", play_book).await;
    assert_eq!(queue(&mut client).await["playback_speed"], 1.5);
    // A track has no speed of its own.
    client
        .result(
            "player_queues/play_media",
            json!({"queue_id": "kitchen", "media": "fake://album/fa1", "option": "replace"}),
        )
        .await;
    let refused = client
        .send(
            "player_queues/set_playback_speed",
            json!({"queue_id": "kitchen", "speed": 1.5}),
        )
        .await;
    assert!(refused["error_code"].is_number(), "{refused}");
}
