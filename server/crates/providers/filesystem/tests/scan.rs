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
