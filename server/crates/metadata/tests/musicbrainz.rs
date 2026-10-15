//! Looking albums up against a stand-in for MusicBrainz's web service.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use bopsie_core::db::Database;
use bopsie_core::events::EventBus;
use bopsie_core::media::{MediaItem, MediaType};
use bopsie_library::{Library, ListQuery};
use bopsie_metadata::job::{Enricher, Summary};
use bopsie_metadata::musicbrainz::MusicBrainz;
use bopsie_providers_filesystem::FilesystemProvider;
use serde_json::{Value, json};

const FIRST: &str = "11111111-1111-1111-1111-111111111111";
const SECOND: &str = "22222222-2222-2222-2222-222222222222";
const GROUP: &str = "33333333-3333-3333-3333-333333333333";
const ARTIST: &str = "44444444-4444-4444-4444-444444444444";

#[derive(Default)]
struct Stand {
    /// Requests by path and query, in order.
    requests: Vec<(String, Instant)>,
    agents: Vec<String>,
    /// Requests left to answer as busy.
    busy: usize,
}

type Shared = Arc<Mutex<Stand>>;

fn track(position: u32, title: &str, recording: &str) -> Value {
    json!({
        "id": format!("{recording}-track"),
        "position": position,
        "title": title,
        "length": 1000,
        "recording": {"id": recording, "isrcs": [format!("SEAAA2100{position:03}")]},
        "artist-credit": [{"name": "Test Artist", "joinphrase": "",
            "artist": {"id": ARTIST, "name": "Test Artist"}}],
    })
}

fn release(id: &str, title: &str, artist: &str, tracks: Vec<Value>) -> Value {
    json!({
        "id": id,
        "title": title,
        "date": "2021-03-01",
        "country": "SE",
        "barcode": "0602557382549",
        "artist-credit": [{"name": artist, "joinphrase": "",
            "artist": {"id": ARTIST, "name": artist}}],
        "release-group": {"id": GROUP, "first-release-date": "2021-03-01",
            "genres": [{"name": "pop", "count": 3}]},
        "relations": [{"type": "free streaming",
            "url": {"resource": "https://open.spotify.com/album/streamedalbum"}}],
        "media": [{"position": 1, "format": "Digital Media", "tracks": tracks}],
    })
}

fn releases() -> HashMap<&'static str, Value> {
    HashMap::from([
        (
            FIRST,
            release(
                FIRST,
                "First Album",
                "Test Artist",
                vec![
                    track(1, "Opening", "55555555-5555-5555-5555-000000000001"),
                    track(2, "Middle", "55555555-5555-5555-5555-000000000002"),
                    track(3, "Closing", "55555555-5555-5555-5555-000000000003"),
                ],
            ),
        ),
        // Close, but with a track the files lack.
        (
            SECOND,
            release(
                SECOND,
                "Second Album",
                "Other Artist",
                vec![
                    track(1, "Alpha", "66666666-6666-6666-6666-000000000001"),
                    track(2, "Beta", "66666666-6666-6666-6666-000000000002"),
                    track(3, "Gamma", "66666666-6666-6666-6666-000000000003"),
                    track(4, "Delta", "66666666-6666-6666-6666-000000000004"),
                ],
            ),
        ),
    ])
}

fn note(stand: &Shared, headers: &HeaderMap, request: String) -> bool {
    let mut stand = stand.lock().unwrap();
    stand.requests.push((request, Instant::now()));
    if let Some(agent) = headers.get("user-agent").and_then(|v| v.to_str().ok()) {
        stand.agents.push(agent.to_owned());
    }
    if stand.busy > 0 {
        stand.busy -= 1;
        return true;
    }
    false
}

async fn lookup(
    State(stand): State<Shared>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if note(&stand, &headers, format!("release/{id}")) {
        return (StatusCode::SERVICE_UNAVAILABLE, [("retry-after", "1")]).into_response();
    }
    match releases().remove(id.as_str()) {
        Some(release) => axum::Json(release).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn search(
    State(stand): State<Shared>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let query = query["query"].clone();
    if note(&stand, &headers, format!("search {query}")) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let found = if query.contains("\"First Album\"") {
        vec![json!({"id": FIRST, "score": 100, "title": "First Album", "track-count": 3})]
    } else if query.contains("\"Second Album\"") {
        vec![json!({"id": SECOND, "score": 100, "title": "Second Album", "track-count": 4})]
    } else {
        Vec::new()
    };
    axum::Json(json!({"releases": found})).into_response()
}

async fn stand_in() -> (String, Shared) {
    let stand = Shared::default();
    let app = Router::new()
        .route("/ws/2/release", get(search))
        .route("/ws/2/release/{id}", get(lookup))
        .with_state(stand.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/ws/2", listener.local_addr().unwrap());
    tokio::spawn(axum::serve(listener, app).into_future());
    (base, stand)
}

fn database() -> Database {
    Database::open_in_memory(&[bopsie_library::MIGRATIONS, bopsie_metadata::MIGRATIONS]).unwrap()
}

#[tokio::test]
async fn requests_are_paced_named_retried_when_busy_and_kept() {
    let (base, stand) = stand_in().await;
    let musicbrainz = MusicBrainz::new(database(), base, Some("someone@example.org"));
    stand.lock().unwrap().busy = 1;
    let started = Instant::now();
    let first = musicbrainz.release(FIRST).await.unwrap().unwrap();
    assert_eq!(first.title, "First Album");
    assert!(
        started.elapsed() >= Duration::from_secs(1),
        "waited as MusicBrainz asked"
    );
    assert_eq!(first.spotify_albums(), ["streamedalbum"]);
    assert_eq!(
        first.describe(),
        "Test Artist – First Album (2021, SE, Digital Media)"
    );
    assert!(musicbrainz.release(SECOND).await.unwrap().is_some());
    assert!(
        musicbrainz
            .release("77777777-7777-7777-7777-777777777777")
            .await
            .unwrap()
            .is_none()
    );

    // Asked again, it is kept, as is what was not found.
    musicbrainz.release(FIRST).await.unwrap();
    musicbrainz
        .release("77777777-7777-7777-7777-777777777777")
        .await
        .unwrap();
    let stand = stand.lock().unwrap();
    assert_eq!(stand.requests.len(), 4);
    for pair in stand.requests.windows(2) {
        assert!(
            pair[1].1 - pair[0].1 >= Duration::from_millis(990),
            "a request a second"
        );
    }
    assert!(
        stand
            .agents
            .iter()
            .all(|agent| agent.starts_with("Bopsie/") && agent.ends_with("( someone@example.org )")),
        "{:?}",
        stand.agents
    );
}

fn served_ids(item: &MediaItem) -> Vec<(String, String)> {
    let value = serde_json::to_value(item).unwrap();
    value["external_ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|pair| {
            (
                pair[0].as_str().unwrap().to_owned(),
                pair[1].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

async fn album(library: &Library, name: &str) -> MediaItem {
    let albums = library
        .list(MediaType::Album, ListQuery::default())
        .await
        .unwrap();
    albums
        .into_iter()
        .find(|album| album.fields().name == name)
        .unwrap()
}

#[tokio::test]
async fn albums_are_identified_suggested_and_accepted() {
    let (base, stand) = stand_in().await;
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("music");
    bopsie_fixtures::write_library(&music, 1).unwrap();
    let db = database();
    let library = Library::new(
        db.clone(),
        EventBus::default(),
        vec![Arc::new(FilesystemProvider::new(music))],
        dir.path().join("artwork"),
    );
    library.sync().await.unwrap();
    let enricher = Enricher::new(
        library.clone(),
        MusicBrainz::new(db, base, None).paced(Duration::from_millis(10)),
    );

    assert_eq!(
        enricher.run().await,
        Summary {
            matched: 1,
            suggested: 1,
            ..Summary::default()
        }
    );
    let first = album(&library, "First Album").await;
    let ids = served_ids(&first);
    for expected in [
        ("musicbrainz_albumid", FIRST),
        ("musicbrainz_releasegroupid", GROUP),
        ("barcode", "00602557382549"),
    ] {
        assert!(
            ids.contains(&(expected.0.to_owned(), expected.1.to_owned())),
            "{ids:?}"
        );
    }
    let first_id: i64 = first.fields().item_id.parse().unwrap();
    let tracks = library.album_tracks(first_id).await.unwrap();
    let opening = library
        .item(MediaType::Track, tracks[0].fields.item_id.parse().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert!(served_ids(&opening).contains(&(
        "musicbrainz_recordingid".to_owned(),
        "55555555-5555-5555-5555-000000000001".to_owned()
    )));
    let artist = library
        .item(
            MediaType::Artist,
            match &first {
                MediaItem::Album(album) => match &album.artists[0] {
                    bopsie_core::media::ArtistRef::Full(artist) => {
                        artist.fields.item_id.parse().unwrap()
                    }
                    bopsie_core::media::ArtistRef::Mapping(mapping) => {
                        mapping.item_id.parse().unwrap()
                    }
                },
                _ => unreachable!(),
            },
        )
        .await
        .unwrap()
        .unwrap();
    assert!(served_ids(&artist).contains(&("musicbrainz_artistid".to_owned(), ARTIST.to_owned())));

    // The second is close but lacks a track: suggested, not taken.
    let suggestions = library.release_suggestions(false).await.unwrap();
    assert_eq!(suggestions.len(), 1);
    assert_eq!(suggestions[0].release_id, SECOND);
    assert_eq!(
        suggestions[0].title,
        "Other Artist – Second Album (2021, SE, Digital Media)"
    );
    let second = album(&library, "Second Album").await;
    assert!(served_ids(&second).is_empty());

    // Nothing is due again until it is refreshed.
    let asked = stand.lock().unwrap().requests.len();
    assert_eq!(enricher.run().await, Summary::default());
    assert_eq!(stand.lock().unwrap().requests.len(), asked);

    // Accepted, it is taken.
    library
        .decide_release(suggestions[0].album_id, "accept".to_owned())
        .await
        .unwrap();
    assert_eq!(enricher.run().await.matched, 1);
    let second = album(&library, "Second Album").await;
    assert!(served_ids(&second).contains(&("musicbrainz_albumid".to_owned(), SECOND.to_owned())));
    assert!(library.release_suggestions(false).await.unwrap().is_empty());
}

/// A streaming service holding the fixture's First Album under another name, as its own id.
struct Streamed;

impl bopsie_library::provider::MusicProvider for Streamed {
    fn domain(&self) -> &'static str {
        "streamed"
    }
    fn instance_id(&self) -> &str {
        "streamed"
    }
    fn list(
        &self,
    ) -> Result<bopsie_library::provider::Listing, bopsie_library::provider::ProviderError> {
        use bopsie_library::provider::{Entry, EntryKind, Listing};
        Ok(Listing {
            entries: ["Opening", "Middle", "Closing"]
                .iter()
                .map(|title| Entry {
                    id: (*title).to_owned(),
                    kind: EntryKind::Track,
                    stamp: "1".to_owned(),
                })
                .collect(),
            unreadable: Vec::new(),
        })
    }
    fn read_track(
        &self,
        id: &str,
    ) -> Result<bopsie_library::provider::ScannedTrack, bopsie_library::provider::ProviderError>
    {
        use bopsie_library::ids::IdKind;
        let number = ["Opening", "Middle", "Closing"]
            .iter()
            .position(|title| *title == id)
            .unwrap() as u32
            + 1;
        Ok(bopsie_library::provider::ScannedTrack {
            title: id.to_owned(),
            artists: vec!["Test Artist".to_owned()],
            album: Some("Debut".to_owned()),
            album_artists: vec!["Test Artist".to_owned()],
            track_number: Some(number),
            disc_number: Some(1),
            duration_secs: 1,
            audio_format: bopsie_core::media::AudioFormat::unknown(),
            album_id: Some("streamedalbum".to_owned()),
            album_ids: vec![(IdKind::Spotify, "streamedalbum".to_owned())],
            ..Default::default()
        })
    }
    fn read_playlist(
        &self,
        id: &str,
    ) -> Result<bopsie_library::provider::ScannedPlaylist, bopsie_library::provider::ProviderError>
    {
        Err(bopsie_library::provider::ProviderError::Unreadable(
            id.to_owned(),
        ))
    }
    fn image(&self, path: &str) -> Result<Vec<u8>, bopsie_library::provider::ProviderError> {
        Err(bopsie_library::provider::ProviderError::Unreadable(
            path.to_owned(),
        ))
    }
    fn file(&self, _id: &str) -> Option<std::path::PathBuf> {
        None
    }
    fn separate_albums(&self) -> bool {
        true
    }
}

#[tokio::test]
async fn a_release_linked_to_a_streamed_album_merges_them_whatever_their_names() {
    let (base, _stand) = stand_in().await;
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("music");
    bopsie_fixtures::write_library(&music, 1).unwrap();
    let db = database();
    let library = Library::new(
        db.clone(),
        EventBus::default(),
        vec![Arc::new(FilesystemProvider::new(music)), Arc::new(Streamed)],
        dir.path().join("artwork"),
    );
    library.sync().await.unwrap();
    let names = |albums: Vec<MediaItem>| -> Vec<String> {
        albums.iter().map(|a| a.fields().name.clone()).collect()
    };
    assert_eq!(
        names(
            library
                .list(MediaType::Album, ListQuery::default())
                .await
                .unwrap()
        ),
        ["Debut", "First Album", "Second Album"]
    );
    Enricher::new(
        library.clone(),
        MusicBrainz::new(db, base, None).paced(Duration::from_millis(10)),
    )
    .run()
    .await;
    assert_eq!(
        names(
            library
                .list(MediaType::Album, ListQuery::default())
                .await
                .unwrap()
        ),
        ["First Album", "Second Album"]
    );
    let first = album(&library, "First Album").await;
    assert_eq!(first.fields().provider_mappings.len(), 2);
}
