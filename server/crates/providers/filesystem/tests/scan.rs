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

/// A streaming service's saved music: one album named as a local one, by the same artist.
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
async fn a_streaming_services_album_stays_apart_from_a_local_one_of_the_same_name() {
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

/// A streaming service holding the fixture's First Album too, tagged a little differently, each
/// track playing from a file of its own.
struct Holding {
    streams: std::path::PathBuf,
    stamp: std::sync::atomic::AtomicU32,
}

const HELD: [&str; 3] = ["Opening", "Middle", "Closing"];

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
            entries: HELD
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
        Ok(bopsie_library::provider::ScannedTrack {
            title: title.to_uppercase(),
            artists: vec!["Test Artist".to_owned()],
            album: Some("First Album".to_owned()),
            album_artists: vec!["Test Artist".to_owned()],
            track_number: Some(HELD.iter().position(|t| *t == title).unwrap() as u32 + 1),
            disc_number: Some(1),
            duration_secs: 7,
            audio_format: bopsie_core::media::AudioFormat::unknown(),
            album_id: Some("held-album".to_owned()),
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

/// Merges the streaming service's First Album into the local one, as reconciling would.
async fn merge_held(db: &Database) {
    db.call(|conn| {
        let tx = conn.transaction()?;
        let local_album: i64 = tx.query_row(
            "SELECT albums.id FROM albums JOIN provider_mappings
                 ON media_type = 'album' AND item_id = albums.id
             WHERE name = 'First Album' AND provider_instance != 'streamed'",
            [],
            |row| row.get(0),
        )?;
        for title in HELD {
            let local: i64 =
                tx.query_row("SELECT id FROM tracks WHERE name = ?1", [title], |row| {
                    row.get(0)
                })?;
            let held: i64 = tx.query_row(
                "SELECT item_id FROM provider_mappings
                 WHERE media_type = 'track' AND provider_item_id = ?1",
                [format!("s-{title}")],
                |row| row.get(0),
            )?;
            tx.execute(
                "UPDATE provider_mappings SET item_id = ?1
                 WHERE media_type = 'track' AND item_id = ?2",
                [local, held],
            )?;
            tx.execute("DELETE FROM track_artists WHERE track_id = ?1", [held])?;
            tx.execute("DELETE FROM tracks WHERE id = ?1", [held])?;
        }
        tx.execute(
            "UPDATE provider_mappings SET item_id = ?1
             WHERE media_type = 'album' AND provider_instance = 'streamed'",
            [local_album],
        )?;
        tx.commit()?;
        Ok(())
    })
    .await
    .unwrap();
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

#[tokio::test]
async fn a_merged_album_keeps_the_files_tags_and_plays_from_one_source() {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("music");
    let streams = dir.path().join("streams");
    bopsie_fixtures::write_library(&music, 1).unwrap();
    let db = Database::open_in_memory(&[bopsie_library::MIGRATIONS]).unwrap();
    let holding = Arc::new(Holding {
        streams: streams.clone(),
        stamp: 1.into(),
    });
    let library = Library::new(
        db.clone(),
        EventBus::default(),
        vec![
            Arc::new(FilesystemProvider::new(music.clone())),
            holding.clone(),
        ],
        dir.path().join("artwork"),
    );
    library.sync().await.unwrap();
    merge_held(&db).await;

    // The service syncing again changes neither the tracks' tags nor the albums.
    holding.stamp.store(2, std::sync::atomic::Ordering::SeqCst);
    library.sync().await.unwrap();
    let albums = list(&library, MediaType::Album).await;
    assert_eq!(albums, ["First Album", "Second Album"]);
    let album = id(&library, MediaType::Album, "First Album").await;
    let tracks = library.album_tracks(album).await.unwrap();
    assert_eq!(names_of(&tracks), HELD);
    assert!(
        tracks.iter().all(|track| track.duration == 1),
        "the files' durations"
    );

    // It plays from the files while they hold every track.
    let played = played_from(&library, album).await;
    assert!(
        played.iter().all(|path| path.starts_with(&music)),
        "{played:?}"
    );

    // Once they lack one, the whole album plays from the service.
    fs::remove_file(music.join("Test Artist/First Album/02 - Middle.wav")).unwrap();
    library.sync().await.unwrap();
    let played = played_from(&library, album).await;
    assert_eq!(played.len(), 3);
    assert!(
        played.iter().all(|path| path.starts_with(&streams)),
        "{played:?}"
    );
}

fn names_of(tracks: &[bopsie_core::media::Track]) -> Vec<String> {
    tracks
        .iter()
        .map(|track| track.fields.name.clone())
        .collect()
}
