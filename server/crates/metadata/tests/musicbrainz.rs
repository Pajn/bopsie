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
const THIRD: &str = "88888888-8888-8888-8888-888888888888";

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
        // What the second album's files sound like.
        (
            THIRD,
            release(
                THIRD,
                "Second Album",
                "Other Artist",
                vec![
                    track(1, "Alpha", "99999999-9999-9999-9999-000000000001"),
                    track(2, "Beta", "99999999-9999-9999-9999-000000000002"),
                    track(3, "Gamma", "99999999-9999-9999-9999-000000000003"),
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

/// A one-pixel PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0xf0,
    0x1f, 0x00, 0x05, 0x00, 0x01, 0xff, 0x89, 0x99, 0x3d, 0x1d, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

async fn cover(
    State(stand): State<Shared>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    note(&stand, &headers, format!("cover {id}"));
    ([("content-type", "image/png")], PNG).into_response()
}

/// AcoustID: each fixture file's stand-in fingerprint is the recording of its title on THIRD.
async fn acoustid(State(stand): State<Shared>, headers: HeaderMap, body: String) -> Response {
    let form: HashMap<String, String> = body
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
    note(
        &stand,
        &headers,
        format!("acoustid {}", form["fingerprint"]),
    );
    assert_eq!(form["client"], "the-key");
    let recording = match form["fingerprint"].as_str() {
        "FP-01+-+Alpha.wav" => "99999999-9999-9999-9999-000000000001",
        "FP-02+-+Beta.wav" => "99999999-9999-9999-9999-000000000002",
        "FP-03+-+Gamma.wav" => "99999999-9999-9999-9999-000000000003",
        _ => return axum::Json(json!({"status": "ok", "results": []})).into_response(),
    };
    axum::Json(
        json!({"status": "ok", "results": [{"id": "a", "score": 0.93, "recordings": [
            {"id": recording, "releases": [{"id": THIRD}]}
        ]}]}),
    )
    .into_response()
}

async fn stand_in() -> (String, Shared) {
    let stand = Shared::default();
    let app = Router::new()
        .route("/v2/lookup", axum::routing::post(acoustid))
        .route("/ws/2/release", get(search))
        .route("/ws/2/release/{id}", get(lookup))
        .route("/release-group/{id}/front-500", get(cover))
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
    // The second album's files bring no art.
    std::fs::remove_file(music.join("Other Artist/Second Album/cover.jpg")).unwrap();
    let db = database();
    let library = Library::new(
        db.clone(),
        EventBus::default(),
        vec![Arc::new(FilesystemProvider::new(music))],
        dir.path().join("artwork"),
    );
    library.sync().await.unwrap();
    let first_art = album(&library, "First Album")
        .await
        .fields()
        .metadata
        .images
        .clone();
    assert!(first_art.is_some());
    let enricher = Enricher::new(
        library.clone(),
        MusicBrainz::new(db, base.clone(), None).paced(Duration::from_millis(10)),
    )
    .cover_art(Some(base.trim_end_matches("/ws/2").to_owned()));

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
    // Its own art stays; it gains the release group's genres, as do its artist and tracks.
    assert_eq!(first.fields().metadata.images, first_art);
    assert_eq!(first.fields().metadata.genres, Some(vec!["pop".to_owned()]));
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
    assert_eq!(
        artist.fields().metadata.genres,
        Some(vec!["pop".to_owned()])
    );
    assert_eq!(
        opening.fields().metadata.genres,
        Some(vec!["pop".to_owned()])
    );

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

    // Refreshed by hand, it is looked up again at once.
    library.refresh_metadata(first_id).await.unwrap();
    tokio::time::timeout(Duration::from_secs(1), library.metadata_wanted().notified())
        .await
        .expect("the job is woken");
    assert_eq!(enricher.run().await.matched, 1);
    let (run, status) = library.metadata_status().await.unwrap();
    assert!(!run.running);
    assert_eq!(run.last.unwrap().matched, 1);
    assert_eq!(
        (
            status.albums,
            status.checked,
            status.identified,
            status.suggested
        ),
        (2, 2, 1, 1)
    );

    // Accepted, it is taken.
    library
        .decide_release(suggestions[0].album_id, "accept".to_owned())
        .await
        .unwrap();
    assert_eq!(enricher.run().await.matched, 1);
    let second = album(&library, "Second Album").await;
    assert!(served_ids(&second).contains(&("musicbrainz_albumid".to_owned(), SECOND.to_owned())));
    // Without art of its own, it has the Cover Art Archive's.
    let images = second.fields().metadata.images.clone().unwrap();
    assert_eq!(images[0].provider, "coverartarchive");
    let proxy_id = images[0].proxy_id.clone();
    assert_eq!(library.image(proxy_id).await.unwrap().unwrap(), PNG);
    let covers = stand
        .lock()
        .unwrap()
        .requests
        .iter()
        .filter(|(request, _)| request.starts_with("cover "))
        .count();
    assert_eq!(covers, 1, "only for the album without art");
    assert!(library.release_suggestions(false).await.unwrap().is_empty());
}

/// A streaming service holding the fixture's First Album under another name, as its own id,
/// mastered a few seconds longer: nothing but a link says it is the same album.
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
            duration_secs: 5,
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

#[cfg(unix)]
#[tokio::test]
async fn a_badly_tagged_album_is_found_by_its_sound() {
    use bopsie_metadata::fingerprint::AcoustId;
    use std::os::unix::fs::PermissionsExt;

    let (base, stand) = stand_in().await;
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("music");
    bopsie_fixtures::write_library(&music, 1).unwrap();
    // The second album's files, tagged with nothing to find them by.
    for (number, file) in ["01 - Alpha.wav", "02 - Beta.wav", "03 - Gamma.wav"]
        .iter()
        .enumerate()
    {
        use lofty::config::WriteOptions;
        use lofty::prelude::*;
        use lofty::tag::{Tag, TagType};
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_title(format!("Track {}", number + 1));
        tag.set_artist("Unknown".to_owned());
        tag.set_album("Untitled".to_owned());
        tag.set_track(number as u32 + 1);
        tag.save_to_path(
            music.join("Other Artist/Second Album").join(file),
            WriteOptions::default(),
        )
        .unwrap();
    }
    // fpcalc, as a script that names each file's fingerprint after it.
    let fpcalc = dir.path().join("fpcalc");
    std::fs::write(
        &fpcalc,
        "#!/bin/sh\nname=$(basename \"$4\")\nprintf '{\"duration\": 1.0, \"fingerprint\": \"FP-%s\"}' \"$name\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&fpcalc, std::fs::Permissions::from_mode(0o755)).unwrap();

    let db = database();
    let library = Library::new(
        db.clone(),
        EventBus::default(),
        vec![Arc::new(FilesystemProvider::new(music))],
        dir.path().join("artwork"),
    );
    library.sync().await.unwrap();
    let root = base.trim_end_matches("/ws/2").to_owned();
    let enricher = Enricher::new(
        library.clone(),
        MusicBrainz::new(db.clone(), base, None).paced(Duration::from_millis(10)),
    )
    .cover_art(None)
    .by_sound_with(Some(AcoustId::new(
        db.clone(),
        format!("{root}/v2"),
        "the-key".to_owned(),
        fpcalc,
    )));
    assert_eq!(enricher.run().await.matched, 2);
    let untitled = album(&library, "Untitled").await;
    assert!(served_ids(&untitled).contains(&("musicbrainz_albumid".to_owned(), THIRD.to_owned())));
    let id: i64 = untitled.fields().item_id.parse().unwrap();
    let first = &library.album_tracks(id).await.unwrap()[0];
    let first = library
        .item(MediaType::Track, first.fields.item_id.parse().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert!(served_ids(&first).contains(&(
        "musicbrainz_recordingid".to_owned(),
        "99999999-9999-9999-9999-000000000001".to_owned()
    )));
    // Each file was fingerprinted once, and is kept.
    let kept: i64 = db
        .call(|conn| Ok(conn.query_row("SELECT count(*) FROM fingerprints", [], |row| row.get(0))?))
        .await
        .unwrap();
    assert_eq!(kept, 3);
    let lookups = stand
        .lock()
        .unwrap()
        .requests
        .iter()
        .filter(|(request, _)| request.starts_with("acoustid "))
        .count();
    assert_eq!(lookups, 3, "only the album a search could not find");
}
