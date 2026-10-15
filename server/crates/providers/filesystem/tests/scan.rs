//! Scanning the fixture library from disk into the library, and rescanning it as it changes.

use std::fs;
use std::sync::Arc;

use bopsie_core::db::Database;
use bopsie_core::events::{Event, EventBus, EventType};
use bopsie_core::media::{MediaItem, MediaType};
use bopsie_library::{Library, ListQuery};
use bopsie_providers_filesystem::FilesystemProvider;
use lofty::config::WriteOptions;
use lofty::prelude::*;
use lofty::tag::{Tag, TagType};
use tokio::sync::broadcast;

struct Fixture {
    dir: tempfile::TempDir,
    library: Library,
    events: broadcast::Receiver<Event>,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("music");
    bopsie_fixtures::write_library(&music, 1).unwrap();
    let db = Database::open_in_memory(&[bopsie_library::MIGRATIONS]).unwrap();
    let bus = EventBus::default();
    let events = bus.subscribe();
    let provider = Arc::new(FilesystemProvider::new(music));
    let library = Library::new(db, bus, vec![provider], dir.path().join("artwork"));
    Fixture {
        dir,
        library,
        events,
    }
}

/// The library id of the item named `name`.
async fn id(library: &Library, media_type: MediaType, name: &str) -> i64 {
    let items = library
        .list(media_type, ListQuery::default())
        .await
        .unwrap();
    let item = items
        .iter()
        .find(|item| item.fields().name == name)
        .unwrap_or_else(|| panic!("no {media_type:?} named {name}"));
    item.fields().item_id.parse().unwrap()
}

fn names(items: &[MediaItem]) -> Vec<String> {
    items
        .iter()
        .map(|item| item.fields().name.clone())
        .collect()
}

async fn list(library: &Library, media_type: MediaType) -> Vec<String> {
    names(
        &library
            .list(media_type, ListQuery::default())
            .await
            .unwrap(),
    )
}

fn drain(events: &mut broadcast::Receiver<Event>) -> Vec<(EventType, Option<String>)> {
    std::iter::from_fn(|| events.try_recv().ok())
        .map(|event| (event.event, event.object_id))
        .collect()
}

#[tokio::test]
async fn a_first_scan_builds_the_library() {
    let mut f = fixture();
    let changes = f.library.sync().await.unwrap();
    assert_eq!(changes.added.len(), 2 + 2 + 6 + 1);

    assert_eq!(
        list(&f.library, MediaType::Artist).await,
        ["Other Artist", "Test Artist"]
    );
    assert_eq!(
        list(&f.library, MediaType::Album).await,
        ["First Album", "Second Album"]
    );
    assert_eq!(list(&f.library, MediaType::Playlist).await, ["Mix"]);
    assert_eq!(
        f.library
            .count(MediaType::Track, ListQuery::default())
            .await
            .unwrap(),
        6
    );

    let first = id(&f.library, MediaType::Album, "First Album").await;
    let Some(MediaItem::Album(album)) = f.library.item(MediaType::Album, first).await.unwrap()
    else {
        panic!("First Album");
    };
    assert_eq!(album.fields.name, "First Album");
    assert_eq!(album.year, Some(2021));
    assert_eq!(album.artists[0].name(), "Test Artist");
    let image = &album.fields.metadata.images.as_ref().unwrap()[0];
    assert_eq!(image.path, "Test Artist/First Album/cover.jpg");
    assert_eq!(
        album.fields.provider_mappings[0].item_id,
        "Test Artist/First Album"
    );
    let cover = f
        .library
        .image(image.proxy_id.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&cover[..2], [0xff, 0xd8], "a JPEG");

    let tracks = f.library.album_tracks(first).await.unwrap();
    let titles: Vec<&str> = tracks.iter().map(|t| t.fields.name.as_str()).collect();
    assert_eq!(titles, ["Opening", "Middle", "Closing"]);
    assert_eq!(tracks[0].duration, 1);
    assert_eq!(tracks[0].album.as_ref().unwrap().name, "First Album");
    assert_eq!(
        tracks[0].fields.provider_mappings[0]
            .audio_format
            .content_type,
        "wav"
    );

    let mix = f.library.playlist_tracks(1).await.unwrap();
    let entries: Vec<(&str, Option<u32>)> = mix
        .iter()
        .map(|t| (t.fields.name.as_str(), t.fields.position))
        .collect();
    assert_eq!(entries, [("Opening", Some(0)), ("Gamma", Some(1))]);

    let events = drain(&mut f.events);
    assert_eq!(events.len(), 11 + 1);
    assert!(events.contains(&(
        EventType::MediaItemAdded,
        Some(format!("library://album/{first}"))
    )));
    assert_eq!(
        events.last().unwrap(),
        &(EventType::MusicSyncCompleted, None)
    );
}

#[tokio::test]
async fn search_finds_substrings_and_short_prefixes() {
    let f = fixture();
    f.library.sync().await.unwrap();
    let search = |text: &str, media_type| {
        let library = f.library.clone();
        let text = text.to_owned();
        async move { names(&library.search(media_type, text, 25).await.unwrap()) }
    };
    assert_eq!(search("Gamma", MediaType::Track).await, ["Gamma"]);
    assert_eq!(search("amm", MediaType::Track).await, ["Gamma"]);
    assert_eq!(search("ga", MediaType::Track).await, ["Gamma"]);
    assert_eq!(search("album", MediaType::Album).await.len(), 2);
    assert_eq!(search("other", MediaType::Artist).await, ["Other Artist"]);
    assert!(search("nothing like it", MediaType::Track).await.is_empty());
}

#[tokio::test]
async fn rescanning_reads_only_what_changed() {
    let mut f = fixture();
    f.library.sync().await.unwrap();
    drain(&mut f.events);

    let unchanged = f.library.sync().await.unwrap();
    assert!(unchanged.is_empty(), "{unchanged:?}");
    assert_eq!(
        drain(&mut f.events),
        [(EventType::MusicSyncCompleted, None)]
    );

    // Retitle one track.
    let path = f
        .dir
        .path()
        .join("music/Test Artist/First Album/02 - Middle.wav");
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title("Centre".to_owned());
    tag.set_artist("Test Artist".to_owned());
    tag.set_album("First Album".to_owned());
    tag.save_to_path(&path, WriteOptions::default()).unwrap();
    let changes = f.library.sync().await.unwrap();
    assert_eq!(changes.updated.len(), 1, "{changes:?}");
    assert!(changes.added.is_empty() && changes.deleted.is_empty());
    let titles: Vec<String> = f
        .library
        .album_tracks(id(&f.library, MediaType::Album, "First Album").await)
        .await
        .unwrap()
        .into_iter()
        .map(|t| t.fields.name)
        .collect();
    assert!(titles.contains(&"Centre".to_owned()), "{titles:?}");
}

#[tokio::test]
async fn removing_files_removes_what_only_they_held() {
    let mut f = fixture();
    f.library.sync().await.unwrap();
    drain(&mut f.events);

    let other = id(&f.library, MediaType::Artist, "Other Artist").await;
    fs::remove_dir_all(f.dir.path().join("music/Other Artist")).unwrap();
    let changes = f.library.sync().await.unwrap();
    // Three tracks, their album and its artist.
    assert_eq!(changes.deleted.len(), 5, "{changes:?}");
    assert_eq!(list(&f.library, MediaType::Artist).await, ["Test Artist"]);
    assert_eq!(list(&f.library, MediaType::Album).await, ["First Album"]);

    // The playlist keeps the track that is left.
    let mix = f.library.playlist_tracks(1).await.unwrap();
    assert_eq!(mix.len(), 1);
    assert!(
        f.library
            .search(MediaType::Track, "Gamma".to_owned(), 25)
            .await
            .unwrap()
            .is_empty()
    );

    let events = drain(&mut f.events);
    assert!(events.contains(&(
        EventType::MediaItemDeleted,
        Some(format!("library://artist/{other}"))
    )));
}

#[tokio::test]
async fn listings_page_filter_and_order() {
    let f = fixture();
    f.library.sync().await.unwrap();
    let page = |limit, offset, order_by: Option<&str>| ListQuery {
        limit: Some(limit),
        offset: Some(offset),
        order_by: order_by.map(str::to_owned),
        ..ListQuery::default()
    };
    let albums = f
        .library
        .list(MediaType::Album, page(1, 1, None))
        .await
        .unwrap();
    assert_eq!(names(&albums), ["Second Album"]);
    let newest_first = f
        .library
        .list(MediaType::Album, page(10, 0, Some("year_desc")))
        .await
        .unwrap();
    assert_eq!(names(&newest_first), ["Second Album", "First Album"]);
    let matching = ListQuery {
        search: Some("pha".to_owned()),
        ..ListQuery::default()
    };
    assert_eq!(
        names(&f.library.list(MediaType::Track, matching).await.unwrap()),
        ["Alpha"]
    );
    let album_artists = ListQuery {
        album_artists_only: true,
        ..ListQuery::default()
    };
    assert_eq!(
        f.library
            .count(MediaType::Artist, album_artists)
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        f.library
            .artist_albums(id(&f.library, MediaType::Artist, "Test Artist").await)
            .await
            .unwrap()
            .into_iter()
            .map(|album| album.fields.name)
            .collect::<Vec<_>>(),
        ["First Album"]
    );
    assert_eq!(
        f.library
            .artist_tracks(id(&f.library, MediaType::Artist, "Other Artist").await)
            .await
            .unwrap()
            .len(),
        3
    );
}

async fn track_count(library: &Library) -> u32 {
    library
        .count(MediaType::Track, ListQuery::default())
        .await
        .unwrap()
}

#[tokio::test]
async fn a_folder_that_lists_nothing_keeps_its_tracks_until_a_forced_sync() {
    let f = fixture();
    f.library.sync().await.unwrap();
    let before = track_count(&f.library).await;
    assert!(before > 0);

    // The disk is not mounted: the folder is there, and empty.
    let music = f.dir.path().join("music");
    fs::remove_dir_all(&music).unwrap();
    fs::create_dir(&music).unwrap();
    f.library.sync().await.unwrap();
    assert_eq!(track_count(&f.library).await, before, "nothing deleted");

    f.library.sync_with(true).await.unwrap();
    assert_eq!(track_count(&f.library).await, 0, "a forced sync empties it");
}

#[tokio::test]
async fn tracks_under_an_unreadable_folder_are_kept() {
    use std::os::unix::fs::PermissionsExt;
    let f = fixture();
    f.library.sync().await.unwrap();
    let before = track_count(&f.library).await;

    let other = f.dir.path().join("music/Other Artist");
    fs::set_permissions(&other, fs::Permissions::from_mode(0o000)).unwrap();
    let readable = fs::read_dir(&other).is_ok();
    let changes = f.library.sync().await;
    fs::set_permissions(&other, fs::Permissions::from_mode(0o755)).unwrap();
    if readable {
        // Running as a user no permission stops, such as root: nothing to show.
        return;
    }
    let changes = changes.unwrap();
    assert!(changes.deleted.is_empty(), "{changes:?}");
    assert_eq!(track_count(&f.library).await, before);
}

fn edits(pairs: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
    pairs.as_object().unwrap().clone()
}

async fn until(mut holds: impl AsyncFnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    while !holds().await {
        assert!(tokio::time::Instant::now() < deadline, "never held");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn an_edited_name_is_listed_searched_and_sorted_by_and_resets() {
    let mut f = fixture();
    f.library.sync().await.unwrap();
    drain(&mut f.events);
    let second = id(&f.library, MediaType::Album, "Second Album").await;

    f.library
        .edit(
            MediaType::Album,
            second,
            edits(serde_json::json!({"name": "Alpha"})),
        )
        .await
        .unwrap();
    assert_eq!(
        list(&f.library, MediaType::Album).await,
        ["Alpha", "First Album"]
    );
    let found = f
        .library
        .search(MediaType::Album, "alpha".to_owned(), 10)
        .await
        .unwrap();
    assert_eq!(names(&found), ["Alpha"]);
    assert_eq!(
        drain(&mut f.events),
        [(
            EventType::MediaItemUpdated,
            Some(format!("library://album/{second}"))
        )],
        "no sync is announced"
    );
    assert_eq!(
        f.library.edits(MediaType::Album, second).await.unwrap(),
        edits(serde_json::json!({"name": "Alpha"}))
    );

    // A sync keeps it.
    f.library.sync().await.unwrap();
    assert_eq!(
        list(&f.library, MediaType::Album).await,
        ["Alpha", "First Album"]
    );

    f.library
        .edit(
            MediaType::Album,
            second,
            edits(serde_json::json!({"name": null})),
        )
        .await
        .unwrap();
    assert_eq!(
        list(&f.library, MediaType::Album).await,
        ["First Album", "Second Album"]
    );
    assert!(
        f.library
            .edits(MediaType::Album, second)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn an_edited_track_keeps_its_edits_when_its_file_changes() {
    let f = fixture();
    f.library.sync().await.unwrap();
    let middle = id(&f.library, MediaType::Track, "Middle").await;
    let first = id(&f.library, MediaType::Album, "First Album").await;

    f.library
        .edit(
            MediaType::Track,
            middle,
            edits(serde_json::json!({
                "name": "Centre",
                "track_number": 9,
                "disc_number": 2,
                "artists": [{"name": "Guest"}],
                "album": {"name": "Singles"},
            })),
        )
        .await
        .unwrap();
    let singles = id(&f.library, MediaType::Album, "Singles").await;
    let tracks = f.library.album_tracks(singles).await.unwrap();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].fields.name, "Centre");
    assert_eq!((tracks[0].disc_number, tracks[0].track_number), (2, 9));
    assert_eq!(tracks[0].artists[0].name, "Guest");
    assert_eq!(f.library.album_tracks(first).await.unwrap().len(), 2);
    assert!(
        list(&f.library, MediaType::Artist)
            .await
            .contains(&"Guest".to_owned())
    );

    // Its file is retagged: what was edited stays, the rest follows the file.
    let path = f
        .dir
        .path()
        .join("music/Test Artist/First Album/02 - Middle.wav");
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title("Middle Mix".to_owned());
    tag.set_artist("Test Artist".to_owned());
    tag.set_album("First Album".to_owned());
    tag.save_to_path(&path, WriteOptions::default()).unwrap();
    f.library.sync().await.unwrap();
    let tracks = f.library.album_tracks(singles).await.unwrap();
    assert_eq!(tracks.len(), 1, "the edited album outlives the sync");
    assert_eq!(tracks[0].fields.name, "Centre");
    assert_eq!(tracks[0].artists[0].name, "Guest");

    // Reset, the name and album are read from the file again.
    f.library
        .edit(
            MediaType::Track,
            middle,
            edits(serde_json::json!({"name": null, "album": null})),
        )
        .await
        .unwrap();
    until(async || {
        f.library
            .album_tracks(first)
            .await
            .unwrap()
            .iter()
            .any(|t| t.fields.name == "Middle Mix")
    })
    .await;
    assert!(
        !list(&f.library, MediaType::Album)
            .await
            .contains(&"Singles".to_owned()),
        "an album left without tracks goes"
    );
    // What is still edited is still applied.
    let track = f
        .library
        .album_tracks(first)
        .await
        .unwrap()
        .into_iter()
        .find(|t| t.fields.name == "Middle Mix")
        .unwrap();
    assert_eq!(track.track_number, 9);
    assert_eq!(track.artists[0].name, "Guest");
}

#[tokio::test]
async fn album_art_and_artists_edit_and_reset() {
    let f = fixture();
    f.library.sync().await.unwrap();
    let first = id(&f.library, MediaType::Album, "First Album").await;
    let second = id(&f.library, MediaType::Album, "Second Album").await;
    let image = |item: Option<MediaItem>| {
        let Some(MediaItem::Album(album)) = item else {
            panic!("an album")
        };
        (
            album.fields.metadata.images.unwrap()[0].proxy_id.clone(),
            album.artists[0].name().to_owned(),
        )
    };
    let (first_art, _) = image(f.library.item(MediaType::Album, first).await.unwrap());
    let (second_art, _) = image(f.library.item(MediaType::Album, second).await.unwrap());

    f.library
        .edit(
            MediaType::Album,
            first,
            edits(serde_json::json!({"image": second_art, "artists": [{"name": "Various"}]})),
        )
        .await
        .unwrap();
    assert_eq!(
        image(f.library.item(MediaType::Album, first).await.unwrap()),
        (second_art.clone(), "Various".to_owned())
    );
    f.library
        .edit(
            MediaType::Album,
            first,
            edits(serde_json::json!({"image": null, "artists": null})),
        )
        .await
        .unwrap();
    assert_eq!(
        image(f.library.item(MediaType::Album, first).await.unwrap()),
        (first_art, "Test Artist".to_owned())
    );
}

#[tokio::test]
async fn edits_that_make_no_sense_are_refused() {
    let f = fixture();
    f.library.sync().await.unwrap();
    let first = id(&f.library, MediaType::Album, "First Album").await;
    for (media_type, id, change) in [
        (
            MediaType::Album,
            first,
            serde_json::json!({"track_number": 1}),
        ),
        (MediaType::Album, first, serde_json::json!({"name": "  "})),
        (MediaType::Album, first, serde_json::json!({"year": -1})),
        (
            MediaType::Album,
            first,
            serde_json::json!({"image": "nothing"}),
        ),
        (MediaType::Album, first, serde_json::json!({"artists": []})),
        (MediaType::Album, 999, serde_json::json!({"name": "x"})),
    ] {
        let refused = f.library.edit(media_type, id, edits(change.clone())).await;
        assert!(refused.is_err(), "{change}");
    }
    assert_eq!(
        list(&f.library, MediaType::Album).await,
        ["First Album", "Second Album"]
    );
}

/// A streaming service's saved music: one album named as a local one, by the same artist, of
/// another recording.
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
            entries: vec![
                Entry {
                    id: "s1".to_owned(),
                    kind: EntryKind::Track,
                    stamp: "1".to_owned(),
                },
                Entry {
                    id: "p1".to_owned(),
                    kind: EntryKind::Playlist,
                    stamp: "1".to_owned(),
                },
            ],
            unreadable: Vec::new(),
        })
    }
    fn read_track(
        &self,
        _id: &str,
    ) -> Result<bopsie_library::provider::ScannedTrack, bopsie_library::provider::ProviderError>
    {
        Ok(bopsie_library::provider::ScannedTrack {
            title: "Opening (Remastered)".to_owned(),
            artists: vec!["Test Artist".to_owned()],
            album: Some("First Album".to_owned()),
            album_artists: vec!["Test Artist".to_owned()],
            year: Some(2020),
            track_number: Some(1),
            disc_number: Some(1),
            duration_secs: 60,
            audio_format: bopsie_core::media::AudioFormat::unknown(),
            image: None,
            album_id: Some("streamed-album".to_owned()),
            ..Default::default()
        })
    }
    fn read_playlist(
        &self,
        _id: &str,
    ) -> Result<bopsie_library::provider::ScannedPlaylist, bopsie_library::provider::ProviderError>
    {
        Ok(bopsie_library::provider::ScannedPlaylist {
            name: "Streamed Mix".to_owned(),
            track_ids: Vec::new(),
            image: Some(bopsie_library::provider::ScannedImage::Path(
                "https://images.example/mix.jpg".to_owned(),
            )),
        })
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
async fn a_streaming_services_album_of_other_recordings_stays_apart_from_a_local_one() {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("music");
    bopsie_fixtures::write_library(&music, 1).unwrap();
    let db = Database::open_in_memory(&[bopsie_library::MIGRATIONS]).unwrap();
    let library = Library::new(
        db,
        EventBus::default(),
        vec![Arc::new(FilesystemProvider::new(music)), Arc::new(Streamed)],
        dir.path().join("artwork"),
    );
    library.sync().await.unwrap();
    let albums = list(&library, MediaType::Album).await;
    assert_eq!(
        albums.iter().filter(|name| *name == "First Album").count(),
        2,
        "{albums:?}"
    );
    // The artist is one, with both albums.
    let artists = list(&library, MediaType::Artist).await;
    assert_eq!(
        artists.iter().filter(|name| *name == "Test Artist").count(),
        1
    );
    let artist = id(&library, MediaType::Artist, "Test Artist").await;
    let first_albums: Vec<_> = library
        .artist_albums(artist)
        .await
        .unwrap()
        .into_iter()
        .filter(|album| album.fields.name == "First Album")
        .collect();
    assert_eq!(first_albums.len(), 2);
    // The streamed one is found by the service's own id for it, as search finds it.
    let held = library
        .by_mapping("streamed", MediaType::Album, "streamed-album")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(held.fields().name, "First Album");
}

/// The streaming service's catalog: its playlist holds a track the library does not.
struct StreamedCatalog(std::path::PathBuf);

impl bopsie_library::catalog::Catalog for StreamedCatalog {
    fn instance_id(&self) -> &str {
        "streamed"
    }
    fn domain(&self) -> &str {
        "streamed"
    }
    fn name(&self) -> &str {
        "Streamed"
    }
    fn search<'a>(
        &'a self,
        _query: &'a str,
        _media_type: MediaType,
        _limit: usize,
    ) -> futures::future::BoxFuture<'a, bopsie_library::catalog::CatalogResult<Vec<MediaItem>>>
    {
        Box::pin(async { Ok(Vec::new()) })
    }
    fn item<'a>(
        &'a self,
        _media_type: MediaType,
        id: &'a str,
    ) -> futures::future::BoxFuture<'a, bopsie_library::catalog::CatalogResult<MediaItem>> {
        Box::pin(async move {
            Err(bopsie_library::catalog::CatalogError::NotFound(
                id.to_owned(),
            ))
        })
    }
    fn album_tracks<'a>(
        &'a self,
        _id: &'a str,
    ) -> futures::future::BoxFuture<
        'a,
        bopsie_library::catalog::CatalogResult<Vec<bopsie_core::media::Track>>,
    > {
        Box::pin(async { Ok(Vec::new()) })
    }
    fn artist_albums<'a>(
        &'a self,
        _id: &'a str,
    ) -> futures::future::BoxFuture<
        'a,
        bopsie_library::catalog::CatalogResult<Vec<bopsie_core::media::Album>>,
    > {
        Box::pin(async { Ok(Vec::new()) })
    }
    fn artist_tracks<'a>(
        &'a self,
        _id: &'a str,
    ) -> futures::future::BoxFuture<
        'a,
        bopsie_library::catalog::CatalogResult<Vec<bopsie_core::media::Track>>,
    > {
        Box::pin(async { Ok(Vec::new()) })
    }
    fn playlist_tracks<'a>(
        &'a self,
        id: &'a str,
    ) -> futures::future::BoxFuture<
        'a,
        bopsie_library::catalog::CatalogResult<Vec<bopsie_core::media::Track>>,
    > {
        Box::pin(async move {
            assert_eq!(id, "p1");
            let mut track = match library_track() {
                MediaItem::Track(track) => *track,
                _ => unreachable!(),
            };
            track.fields.provider = "streamed".to_owned();
            track.fields.item_id = "unsaved".to_owned();
            Ok(vec![track])
        })
    }
    fn media(&self, track_id: &str) -> Option<bopsie_audio::engine::Input> {
        (track_id == "unsaved").then(|| bopsie_audio::engine::Input::File(self.0.clone()))
    }
}

/// Any track, to dress up as the catalog's.
fn library_track() -> MediaItem {
    MediaItem::Track(Box::new(bopsie_core::media::Track {
        fields: bopsie_core::media::ItemFields {
            item_id: "0".to_owned(),
            provider: "library".to_owned(),
            name: "Not Saved".to_owned(),
            version: String::new(),
            sort_name: "not saved".to_owned(),
            uri: "streamed://track/unsaved".to_owned(),
            external_ids: Vec::new(),
            is_playable: true,
            media_type: MediaType::Track,
            provider_mappings: Vec::new(),
            metadata: bopsie_core::media::Metadata::default(),
            favorite: false,
            position: None,
            date_added: None,
        },
        duration: 60,
        artists: Vec::new(),
        last_played: 0,
        album: None,
        disc_number: 1,
        track_number: 1,
        audio_metadata: None,
    }))
}

#[tokio::test]
async fn a_streaming_services_playlist_has_its_artwork_and_its_catalogs_tracks() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open_in_memory(&[bopsie_library::MIGRATIONS]).unwrap();
    let library = Library::new(
        db,
        EventBus::default(),
        vec![Arc::new(Streamed)],
        dir.path().join("artwork"),
    );
    let file = dir.path().join("unsaved.wav");
    library
        .catalogs()
        .add(Arc::new(StreamedCatalog(file.clone())));
    library.sync().await.unwrap();
    let playlist = id(&library, MediaType::Playlist, "Streamed Mix").await;
    let Some(MediaItem::Playlist(listed)) =
        library.item(MediaType::Playlist, playlist).await.unwrap()
    else {
        panic!("no playlist");
    };
    let image = &listed.fields.metadata.images.as_ref().unwrap()[0];
    assert_eq!(image.path, "https://images.example/mix.jpg");

    // Its tracks are the catalog's, not the library's, which does not hold them.
    let tracks = library.playlist_tracks(playlist).await.unwrap();
    assert_eq!(tracks[0].fields.item_id, "unsaved");
    assert_eq!(
        list(&library, MediaType::Track).await,
        ["Opening (Remastered)"]
    );
    let playable = library
        .playable(&format!("library://playlist/{playlist}"))
        .await
        .unwrap();
    assert_eq!(playable.len(), 1);
    assert_eq!(playable[0].1, bopsie_audio::engine::Input::File(file));
}

#[tokio::test]
async fn a_tracks_ids_are_read_from_its_tags_and_served() {
    let f = fixture();
    let path = f
        .dir
        .path()
        .join("music/Test Artist/First Album/01 - Opening.wav");
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title("Opening".to_owned());
    tag.set_artist("Test Artist".to_owned());
    tag.set_album("First Album".to_owned());
    tag.insert_text(ItemKey::Isrc, "us-um7-17-03861".to_owned());
    tag.insert_text(ItemKey::Barcode, "602557382549".to_owned());
    tag.insert_text(
        ItemKey::MusicBrainzRecordingId,
        "8f1a3c35-3b33-4f8b-9a4e-111122223333".to_owned(),
    );
    tag.insert_text(
        ItemKey::MusicBrainzReleaseGroupId,
        "0E5D6A6C-1C1D-4E0B-9A2E-444455556666".to_owned(),
    );
    tag.insert_text(
        ItemKey::MusicBrainzArtistId,
        "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".to_owned(),
    );
    tag.save_to_path(&path, WriteOptions::default()).unwrap();
    f.library.sync().await.unwrap();

    let track = f
        .library
        .search(MediaType::Track, "Opening".to_owned(), 5)
        .await
        .unwrap()
        .remove(0);
    let ids = &track.fields().external_ids;
    assert!(
        ids.contains(&serde_json::json!(["isrc", "USUM71703861"])),
        "{ids:?}"
    );
    assert!(
        ids.contains(&serde_json::json!([
            "musicbrainz_recordingid",
            "8f1a3c35-3b33-4f8b-9a4e-111122223333"
        ])),
        "{ids:?}"
    );
    let album = id(&f.library, MediaType::Album, "First Album").await;
    let Some(MediaItem::Album(album)) = f.library.item(MediaType::Album, album).await.unwrap()
    else {
        panic!("no album");
    };
    assert_eq!(
        album.fields.external_ids,
        [
            serde_json::json!(["barcode", "00602557382549"]),
            serde_json::json!([
                "musicbrainz_releasegroupid",
                "0e5d6a6c-1c1d-4e0b-9a2e-444455556666"
            ]),
        ]
    );
    let artist = id(&f.library, MediaType::Artist, "Test Artist").await;
    let Some(MediaItem::Artist(artist)) = f.library.item(MediaType::Artist, artist).await.unwrap()
    else {
        panic!("no artist");
    };
    assert_eq!(
        artist.fields.external_ids,
        [serde_json::json!([
            "musicbrainz_artistid",
            "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"
        ])]
    );

    // Gone from the tags, gone from the library.
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title("Opening".to_owned());
    tag.set_artist("Test Artist".to_owned());
    tag.set_album("First Album".to_owned());
    tag.save_to_path(&path, WriteOptions::default()).unwrap();
    f.library.sync().await.unwrap();
    let track = f
        .library
        .search(MediaType::Track, "Opening".to_owned(), 5)
        .await
        .unwrap()
        .remove(0);
    assert!(track.fields().external_ids.is_empty());
}

/// A streaming service holding an album like the fixture's First Album, its tracks named and
/// timed a little differently, each playing from a file of its own.
struct Holding {
    streams: std::path::PathBuf,
    album: std::sync::Mutex<(String, Vec<&'static str>)>,
    stamp: std::sync::atomic::AtomicU32,
}

const HELD: [&str; 3] = ["Opening", "Middle", "Closing"];

impl Holding {
    fn new(streams: &std::path::Path, album: &str, titles: &[&'static str]) -> Arc<Self> {
        Arc::new(Self {
            streams: streams.to_owned(),
            album: std::sync::Mutex::new((album.to_owned(), titles.to_vec())),
            stamp: 1.into(),
        })
    }

    /// Now holds `titles` of `album`, read again at the next sync.
    fn hold(&self, album: &str, titles: &[&'static str]) {
        *self.album.lock().unwrap() = (album.to_owned(), titles.to_vec());
        self.stamp.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

impl bopsie_library::provider::MusicProvider for Holding {
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
        let stamp = self.stamp.load(std::sync::atomic::Ordering::SeqCst);
        Ok(Listing {
            entries: self
                .album
                .lock()
                .unwrap()
                .1
                .iter()
                .map(|title| Entry {
                    id: format!("s-{title}"),
                    kind: EntryKind::Track,
                    stamp: stamp.to_string(),
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
        let title = id.strip_prefix("s-").unwrap();
        let (album, titles) = self.album.lock().unwrap().clone();
        // Numbered as on the album, wherever it is among what is held.
        let number = HELD
            .iter()
            .position(|t| *t == title)
            .unwrap_or_else(|| titles.iter().position(|t| *t == title).unwrap())
            as u32
            + 1;
        Ok(bopsie_library::provider::ScannedTrack {
            title: title.to_uppercase(),
            artists: vec!["Test Artist".to_owned()],
            album: Some(album.clone()),
            album_artists: vec!["Test Artist".to_owned()],
            track_number: Some(number),
            disc_number: Some(1),
            duration_secs: 7,
            audio_format: bopsie_core::media::AudioFormat::unknown(),
            album_id: Some(format!("held-{album}")),
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
    fn file(&self, id: &str) -> Option<std::path::PathBuf> {
        Some(self.streams.join(id))
    }
    fn separate_albums(&self) -> bool {
        true
    }
}

struct Merging {
    dir: tempfile::TempDir,
    music: std::path::PathBuf,
    streams: std::path::PathBuf,
    db: Database,
}

impl Merging {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let music = dir.path().join("music");
        bopsie_fixtures::write_library(&music, 1).unwrap();
        Self {
            music,
            streams: dir.path().join("streams"),
            db: Database::open_in_memory(&[bopsie_library::MIGRATIONS]).unwrap(),
            dir,
        }
    }

    /// The library over the files and `holding`, or `holding` alone.
    fn library(&self, files: bool, holding: &Arc<Holding>) -> Library {
        let mut providers: Vec<Arc<dyn bopsie_library::provider::MusicProvider>> =
            vec![holding.clone()];
        if files {
            providers.insert(0, Arc::new(FilesystemProvider::new(self.music.clone())));
        }
        Library::new(
            self.db.clone(),
            EventBus::default(),
            providers,
            self.dir.path().join("artwork"),
        )
    }
}

async fn played_from(library: &Library, album: i64) -> Vec<std::path::PathBuf> {
    library
        .playable(&format!("library://album/{album}"))
        .await
        .unwrap()
        .into_iter()
        .map(|(_, input)| match input {
            bopsie_audio::engine::Input::File(path) => path,
            _ => panic!("not a file"),
        })
        .collect()
}

fn names_of(tracks: &[bopsie_core::media::Track]) -> Vec<String> {
    tracks
        .iter()
        .map(|track| track.fields.name.clone())
        .collect()
}

async fn sources(library: &Library, media_type: MediaType, id: i64) -> Vec<String> {
    let item = library.item(media_type, id).await.unwrap().unwrap();
    let mut sources: Vec<String> = item
        .fields()
        .provider_mappings
        .iter()
        .map(|mapping| mapping.provider_instance.clone())
        .collect();
    sources.sort();
    sources
}

#[tokio::test]
async fn the_same_album_as_files_and_streamed_is_one_album_with_both_sources() {
    let m = Merging::new();
    let holding = Holding::new(&m.streams, "First Album", &HELD);
    // Edited while only streamed: the edit outlasts the merge.
    let streamed = m.library(false, &holding);
    streamed.sync().await.unwrap();
    let album = id(&streamed, MediaType::Album, "First Album").await;
    streamed
        .edit(
            MediaType::Album,
            album,
            edits(serde_json::json!({"year": 1999})),
        )
        .await
        .unwrap();

    let library = m.library(true, &holding);
    library.sync().await.unwrap();
    assert_eq!(
        list(&library, MediaType::Album).await,
        ["First Album", "Second Album"]
    );
    let album = id(&library, MediaType::Album, "First Album").await;
    assert_eq!(sources(&library, MediaType::Album, album).await.len(), 2);
    let Some(MediaItem::Album(merged)) = library.item(MediaType::Album, album).await.unwrap()
    else {
        panic!("no album");
    };
    assert_eq!(merged.year, Some(1999));
    let tracks = library.album_tracks(album).await.unwrap();
    assert_eq!(names_of(&tracks), HELD, "the files' tags");
    let track: i64 = tracks[0].fields.item_id.parse().unwrap();
    assert_eq!(sources(&library, MediaType::Track, track).await.len(), 2);

    // Syncing again changes nothing, even when the service is read again.
    assert!(library.sync().await.unwrap().is_empty());
    holding.hold("First Album", &HELD);
    library.sync().await.unwrap();
    assert_eq!(names_of(&library.album_tracks(album).await.unwrap()), HELD);
    assert!(
        library
            .album_tracks(album)
            .await
            .unwrap()
            .iter()
            .all(|track| track.duration == 1),
        "the files' durations"
    );

    // It plays from the files while they hold every track.
    let played = played_from(&library, album).await;
    assert!(
        played.iter().all(|path| path.starts_with(&m.music)),
        "{played:?}"
    );

    // Once they lack one, the whole album plays from the service.
    fs::remove_file(m.music.join("Test Artist/First Album/02 - Middle.wav")).unwrap();
    library.sync().await.unwrap();
    let played = played_from(&library, album).await;
    assert_eq!(played.len(), 3);
    assert!(
        played.iter().all(|path| path.starts_with(&m.streams)),
        "{played:?}"
    );

    // No longer saved on the service, the files' album is left. (A service that lists nothing
    // is only believed when asked to be.)
    holding.hold("First Album", &[]);
    library.sync_with(true).await.unwrap();
    assert_eq!(
        list(&library, MediaType::Album).await,
        ["First Album", "Second Album"]
    );
    assert_eq!(
        names_of(&library.album_tracks(album).await.unwrap()),
        ["Opening", "Closing"]
    );
    assert_eq!(sources(&library, MediaType::Album, album).await.len(), 1);
}

#[tokio::test]
async fn some_of_an_albums_songs_saved_on_the_service_are_that_album() {
    let m = Merging::new();
    let holding = Holding::new(&m.streams, "First Album", &["Middle", "Closing"]);
    let library = m.library(true, &holding);
    library.sync().await.unwrap();
    assert!(library.suggestions(true).await.unwrap().is_empty());
    assert_eq!(
        list(&library, MediaType::Album).await,
        ["First Album", "Second Album"]
    );
    let album = id(&library, MediaType::Album, "First Album").await;
    assert_eq!(names_of(&library.album_tracks(album).await.unwrap()), HELD);
}

#[tokio::test]
async fn a_remaster_stays_apart() {
    let m = Merging::new();
    let holding = Holding::new(&m.streams, "First Album (2020 Remaster)", &HELD);
    let library = m.library(true, &holding);
    library.sync().await.unwrap();
    assert_eq!(
        list(&library, MediaType::Album).await,
        ["First Album", "First Album (2020 Remaster)", "Second Album"]
    );
    assert!(library.suggestions(true).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_deluxe_edition_is_suggested_and_a_decision_holds() {
    let m = Merging::new();
    let holding = Holding::new(
        &m.streams,
        "First Album (Deluxe)",
        &["Opening", "Middle", "Closing", "Bonus"],
    );
    let library = m.library(true, &holding);
    library.sync().await.unwrap();
    let suggestions = library.suggestions(false).await.unwrap();
    assert_eq!(suggestions.len(), 1, "{suggestions:?}");
    assert_eq!(
        suggestions[0].reason,
        "3 of 4 tracks are the same, in another edition"
    );
    assert_eq!(list(&library, MediaType::Album).await.len(), 3);

    // Kept apart, it is not suggested again.
    library
        .decide(suggestions[0].id, "apart".to_owned())
        .await
        .unwrap();
    holding.hold(
        "First Album (Deluxe)",
        &["Opening", "Middle", "Closing", "Bonus"],
    );
    library.sync().await.unwrap();
    assert!(library.suggestions(false).await.unwrap().is_empty());
    assert_eq!(list(&library, MediaType::Album).await.len(), 3);

    // Said to be the same, they merge, the bonus track joining the files' album.
    let decided = library.suggestions(true).await.unwrap();
    library
        .decide(decided[0].id, "merge".to_owned())
        .await
        .unwrap();
    assert_eq!(
        list(&library, MediaType::Album).await,
        ["First Album", "Second Album"]
    );
    let album = id(&library, MediaType::Album, "First Album").await;
    assert_eq!(
        names_of(&library.album_tracks(album).await.unwrap()),
        ["Opening", "Middle", "Closing", "BONUS"]
    );
    library.sync().await.unwrap();
    assert_eq!(list(&library, MediaType::Album).await.len(), 2);
}

#[tokio::test]
async fn a_merged_album_separates_and_stays_apart() {
    let m = Merging::new();
    let holding = Holding::new(&m.streams, "First Album", &HELD);
    let library = m.library(true, &holding);
    library.sync().await.unwrap();
    let album = id(&library, MediaType::Album, "First Album").await;
    let apart = library
        .separate(album, "streamed".to_owned())
        .await
        .unwrap()
        .expect("held by both");
    assert_eq!(sources(&library, MediaType::Album, album).await.len(), 1);
    assert_eq!(
        sources(&library, MediaType::Album, apart).await,
        ["streamed"]
    );

    library.sync().await.unwrap();
    assert_eq!(
        list(&library, MediaType::Album).await,
        ["First Album", "First Album", "Second Album"]
    );
    assert_eq!(names_of(&library.album_tracks(album).await.unwrap()), HELD);
    assert_eq!(
        names_of(&library.album_tracks(apart).await.unwrap()),
        ["OPENING", "MIDDLE", "CLOSING"],
        "read again from the service"
    );
    let track: i64 = library.album_tracks(album).await.unwrap()[0]
        .fields
        .item_id
        .parse()
        .unwrap();
    assert_eq!(sources(&library, MediaType::Track, track).await.len(), 1);
    assert!(library.suggestions(false).await.unwrap().is_empty());
}

#[tokio::test]
async fn art_the_files_bring_replaces_the_cover_art_archives() {
    let f = fixture();
    let album_dir = f.dir.path().join("music/Other Artist/Second Album");
    let cover = fs::read(album_dir.join("cover.jpg")).unwrap();
    fs::remove_file(album_dir.join("cover.jpg")).unwrap();
    f.library.sync().await.unwrap();
    let album = id(&f.library, MediaType::Album, "Second Album").await;
    let provider = async || match f.library.item(MediaType::Album, album).await.unwrap() {
        Some(MediaItem::Album(album)) => album
            .fields
            .metadata
            .images
            .and_then(|images| images.first().map(|image| image.provider.clone())),
        _ => None,
    };
    assert_eq!(provider().await, None);
    f.library
        .set_cover_art(album, b"not really a png".to_vec(), "png")
        .await
        .unwrap();
    assert_eq!(provider().await.as_deref(), Some("coverartarchive"));

    // The folder gains a cover, and a track is read again.
    fs::write(album_dir.join("cover.jpg"), cover).unwrap();
    let path = album_dir.join("01 - Alpha.wav");
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title("Alpha".to_owned());
    tag.set_artist("Other Artist".to_owned());
    tag.set_album("Second Album".to_owned());
    tag.insert_text(ItemKey::AlbumArtist, "Other Artist".to_owned());
    tag.save_to_path(&path, WriteOptions::default()).unwrap();
    f.library.sync().await.unwrap();
    let provider = provider().await.unwrap();
    assert!(provider.starts_with("filesystem_local"), "{provider}");
}

async fn album_artists(library: &Library, album: i64) -> Vec<String> {
    match library.item(MediaType::Album, album).await.unwrap() {
        Some(MediaItem::Album(album)) => album
            .artists
            .iter()
            .map(|artist| artist.name().to_owned())
            .collect(),
        _ => panic!("no album"),
    }
}

/// A FLAC file of one second of silence's header and no frames, which is enough to be scanned.
fn write_flac(path: &std::path::Path) {
    let mut bytes = b"fLaC".to_vec();
    // The last metadata block, STREAMINFO, 34 bytes.
    bytes.extend([0x80, 0x00, 0x00, 0x22]);
    bytes.extend([0x10, 0x00, 0x10, 0x00]); // block sizes 4096
    bytes.extend([0x00; 6]); // frame sizes unknown
    // 44100 Hz, 2 channels, 16 bits, 44100 samples.
    let packed: u64 = (44_100 << 44) | (1 << 41) | (15 << 36) | 44_100;
    bytes.extend(packed.to_be_bytes());
    bytes.extend([0x00; 16]); // MD5 unknown
    fs::write(path, bytes).unwrap();
}

#[tokio::test]
async fn an_album_artist_tagged_twice_is_credited_once() {
    use lofty::ogg::tag::VorbisComments;
    let f = fixture();
    // FLAC files carrying their album artist under both keys taggers write it under.
    let dir = f.dir.path().join("music/Doubled");
    fs::create_dir_all(&dir).unwrap();
    for (number, title) in ["One", "Two"].iter().enumerate() {
        let path = dir.join(format!("{title}.flac"));
        write_flac(&path);
        let mut comments = VorbisComments::default();
        comments.push("TITLE".to_owned(), (*title).to_owned());
        comments.push("ARTIST".to_owned(), "Doubled Artist".to_owned());
        comments.push("ALBUM".to_owned(), "Doubled Album".to_owned());
        comments.push("ALBUMARTIST".to_owned(), "Doubled Artist".to_owned());
        comments.push("ALBUM ARTIST".to_owned(), "Doubled Artist".to_owned());
        comments.push("TRACKNUMBER".to_owned(), (number + 1).to_string());
        comments
            .save_to_path(&path, WriteOptions::default())
            .unwrap();
    }
    f.library.sync().await.unwrap();
    let album = id(&f.library, MediaType::Album, "Doubled Album").await;
    assert_eq!(album_artists(&f.library, album).await, ["Doubled Artist"]);
    assert_eq!(f.library.album_tracks(album).await.unwrap().len(), 2);
}

#[tokio::test]
async fn an_album_credited_twice_before_is_repaired_and_keeps_its_id() {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("music");
    bopsie_fixtures::write_library(&music, 1).unwrap();
    let db = Database::open_in_memory(&[bopsie_library::MIGRATIONS]).unwrap();
    let library = Library::new(
        db.clone(),
        EventBus::default(),
        vec![Arc::new(FilesystemProvider::new(music.clone()))],
        dir.path().join("artwork"),
    );
    library.sync().await.unwrap();
    let album = id(&library, MediaType::Album, "Second Album").await;
    // As an earlier scan left it: the artist twice, in its credits and its key.
    db.call(move |conn| {
        conn.execute(
            "INSERT INTO album_artists (album_id, position, artist_id)
             SELECT album_id, 1, artist_id FROM album_artists WHERE album_id = ?1",
            [album],
        )?;
        conn.execute(
            "UPDATE albums SET match_key = 'other artist' || char(31) || match_key WHERE id = ?1",
            [album],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    assert_eq!(
        album_artists(&library, album).await,
        ["Other Artist", "Other Artist"]
    );

    library.sync().await.unwrap();
    assert_eq!(album_artists(&library, album).await, ["Other Artist"]);
    // Its files, read again, find it.
    let path = music.join("Other Artist/Second Album/01 - Alpha.wav");
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title("Alpha".to_owned());
    tag.set_artist("Other Artist".to_owned());
    tag.set_album("Second Album".to_owned());
    tag.insert_text(ItemKey::AlbumArtist, "Other Artist".to_owned());
    tag.set_track(1);
    tag.save_to_path(&path, WriteOptions::default()).unwrap();
    library.sync().await.unwrap();
    assert_eq!(id(&library, MediaType::Album, "Second Album").await, album);
    assert_eq!(
        list(&library, MediaType::Album).await,
        ["First Album", "Second Album"]
    );
}

#[tokio::test]
async fn an_id_merged_away_finds_the_item_it_became() {
    let m = Merging::new();
    let holding = Holding::new(&m.streams, "First Album", &HELD);
    // Streamed first: a queue, or a client, holds these ids.
    let streamed = m.library(false, &holding);
    streamed.sync().await.unwrap();
    let old_album = id(&streamed, MediaType::Album, "First Album").await;
    let old_track: i64 = streamed.album_tracks(old_album).await.unwrap()[0]
        .fields
        .item_id
        .parse()
        .unwrap();

    let library = m.library(true, &holding);
    library.sync().await.unwrap();
    let album = id(&library, MediaType::Album, "First Album").await;
    assert_ne!(album, old_album, "the files' album survives");
    let Some(MediaItem::Album(found)) = library.item(MediaType::Album, old_album).await.unwrap()
    else {
        panic!("the old id finds nothing");
    };
    assert_eq!(found.fields.item_id, album.to_string());
    assert_eq!(library.album_tracks(old_album).await.unwrap().len(), 3);
    let Some(MediaItem::Track(track)) = library.item(MediaType::Track, old_track).await.unwrap()
    else {
        panic!("the old track id finds nothing");
    };
    assert_eq!(track.fields.name, "Opening");
    let played = library
        .playable(&format!("library://track/{old_track}"))
        .await
        .unwrap();
    assert_eq!(played.len(), 1);
    assert!(
        matches!(&played[0].1, bopsie_audio::engine::Input::File(path) if path.starts_with(&m.music))
    );
}
