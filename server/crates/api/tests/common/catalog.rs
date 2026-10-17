//! A catalog for tests: one artist with one album of two tracks, a playlist, and an audiobook
//! in two chapters by the same artist, each track and the book playing from a WAV file.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use bopsie_audio::engine::Input;
use bopsie_core::media::{
    Album, Artist, ArtistRef, Audiobook, Chapter, ItemFields, ItemMapping, MediaItem, MediaType,
    Metadata, Playlist, Track, sort_name,
};
use bopsie_library::Library;
use bopsie_library::catalog::{self, Catalog, CatalogError, CatalogResult, Progress};
use futures::future::BoxFuture;

pub const FAKE: &str = "fake";

pub struct FakeCatalog {
    files: PathBuf,
    progress: Reports,
}

/// What playback reported of the catalog's items' progress.
pub type Reports = Arc<Mutex<Vec<(String, Progress)>>>;

fn fields(media_type: MediaType, id: &str, name: &str) -> ItemFields {
    ItemFields {
        item_id: id.to_owned(),
        provider: FAKE.to_owned(),
        name: name.to_owned(),
        version: String::new(),
        sort_name: sort_name(name),
        uri: catalog::uri(FAKE, media_type, id),
        external_ids: Vec::new(),
        is_playable: true,
        media_type,
        provider_mappings: Vec::new(),
        metadata: Metadata::default(),
        favorite: false,
        position: None,
        date_added: None,
    }
}

fn mapping(media_type: MediaType, id: &str, name: &str) -> ItemMapping {
    ItemMapping {
        item_id: id.to_owned(),
        provider: FAKE.to_owned(),
        name: name.to_owned(),
        version: String::new(),
        sort_name: sort_name(name),
        uri: catalog::uri(FAKE, media_type, id),
        external_ids: Vec::new(),
        is_playable: true,
        media_type,
        available: true,
        image: None,
        year: None,
    }
}

fn artist() -> Artist {
    Artist {
        fields: fields(MediaType::Artist, "fr1", "Fake Artist"),
        artist_type: "artist",
    }
}

fn album() -> Album {
    Album {
        fields: fields(MediaType::Album, "fa1", "Fake Album"),
        year: Some(2026),
        artists: vec![ArtistRef::Mapping(Box::new(mapping(
            MediaType::Artist,
            "fr1",
            "Fake Artist",
        )))],
        album_type: "album",
    }
}

fn track(id: &str, name: &str, number: u32) -> Track {
    Track {
        fields: fields(MediaType::Track, id, name),
        duration: 2,
        artists: vec![mapping(MediaType::Artist, "fr1", "Fake Artist")],
        last_played: 0,
        album: Some(mapping(MediaType::Album, "fa1", "Fake Album")),
        disc_number: 1,
        track_number: number,
        audio_metadata: None,
    }
}

fn tracks() -> Vec<Track> {
    vec![track("ft1", "Fake One", 1), track("ft2", "Alpha", 2)]
}

fn playlist() -> Playlist {
    Playlist {
        fields: fields(MediaType::Playlist, "fp1", "Fake Mix"),
        owner: "someone".to_owned(),
        is_editable: false,
        is_dynamic: false,
        supported_mediatypes: vec![MediaType::Track],
    }
}

/// The book: 4 s, in two chapters of 2 s.
pub fn audiobook() -> Audiobook {
    let mut fields = fields(MediaType::Audiobook, "fb1", "The Fake Book");
    fields.metadata.chapters = Some(vec![
        Chapter {
            position: 0,
            name: "Beginning".to_owned(),
            start_ms: 0,
            end_ms: Some(2_000),
        },
        Chapter {
            position: 1,
            name: "End".to_owned(),
            start_ms: 2_000,
            end_ms: Some(4_000),
        },
    ]);
    Audiobook {
        fields,
        publisher: None,
        authors: vec![mapping(MediaType::Artist, "fr1", "Fake Artist")],
        narrators: vec!["Fake Narrator".to_owned()],
        duration: 4,
        fully_played: Some(false),
        resume_position_ms: Some(1_000),
    }
}

impl FakeCatalog {
    /// Adds the catalog to `library`, its tracks and its book playing from WAV files written to
    /// `dir`.
    pub fn add_to(library: &Library, dir: &Path) -> Reports {
        let ids = tracks()
            .into_iter()
            .map(|track| track.fields.item_id)
            .chain(["fb1".to_owned()]);
        for id in ids {
            let path = dir.join(format!("{id}.wav"));
            bopsie_fixtures::write_wav(&path, 44_100, 2, &bopsie_fixtures::noise(3, 44_100 * 4))
                .unwrap();
        }
        let progress = Reports::default();
        library.catalogs().add(Arc::new(Self {
            files: dir.to_owned(),
            progress: progress.clone(),
        }));
        progress
    }
}

fn all() -> Vec<MediaItem> {
    let mut items = vec![
        MediaItem::Artist(Box::new(artist())),
        MediaItem::Album(Box::new(album())),
        MediaItem::Playlist(Box::new(playlist())),
    ];
    items.extend(tracks().into_iter().map(|t| MediaItem::Track(Box::new(t))));
    items.push(MediaItem::Audiobook(Box::new(audiobook())));
    items
}

impl Catalog for FakeCatalog {
    fn instance_id(&self) -> &str {
        FAKE
    }
    fn domain(&self) -> &str {
        FAKE
    }
    fn name(&self) -> &str {
        "Fake"
    }
    fn search<'a>(
        &'a self,
        query: &'a str,
        media_type: MediaType,
        limit: usize,
    ) -> BoxFuture<'a, CatalogResult<Vec<MediaItem>>> {
        let query = query.to_lowercase();
        Box::pin(async move {
            Ok(all()
                .into_iter()
                .filter(|item| item.fields().media_type == media_type)
                .filter(|item| item.fields().name.to_lowercase().contains(&query))
                .take(limit)
                .collect())
        })
    }
    fn item<'a>(
        &'a self,
        media_type: MediaType,
        id: &'a str,
    ) -> BoxFuture<'a, CatalogResult<MediaItem>> {
        Box::pin(async move {
            all()
                .into_iter()
                .find(|item| item.fields().media_type == media_type && item.fields().item_id == id)
                .ok_or_else(|| CatalogError::NotFound(format!("{id} not found")))
        })
    }
    fn album_tracks<'a>(&'a self, id: &'a str) -> BoxFuture<'a, CatalogResult<Vec<Track>>> {
        Box::pin(async move { Ok(if id == "fa1" { tracks() } else { Vec::new() }) })
    }
    fn artist_albums<'a>(&'a self, _id: &'a str) -> BoxFuture<'a, CatalogResult<Vec<Album>>> {
        Box::pin(async move { Ok(vec![album()]) })
    }
    fn artist_tracks<'a>(&'a self, _id: &'a str) -> BoxFuture<'a, CatalogResult<Vec<Track>>> {
        Box::pin(async move { Ok(tracks()) })
    }
    fn playlist_tracks<'a>(&'a self, _id: &'a str) -> BoxFuture<'a, CatalogResult<Vec<Track>>> {
        Box::pin(async move { Ok(tracks().into_iter().rev().collect()) })
    }
    fn media(&self, track_id: &str) -> Option<Input> {
        let path = self.files.join(format!("{track_id}.wav"));
        path.exists().then_some(Input::File(path))
    }
    fn audiobooks<'a>(
        &'a self,
        offset: usize,
        limit: usize,
    ) -> BoxFuture<'a, CatalogResult<Vec<Audiobook>>> {
        Box::pin(async move {
            Ok(vec![audiobook()]
                .into_iter()
                .skip(offset)
                .take(limit)
                .collect())
        })
    }
    fn audiobook_count<'a>(&'a self) -> BoxFuture<'a, CatalogResult<usize>> {
        Box::pin(async { Ok(1) })
    }
    fn progress(&self, id: &str, progress: Progress) {
        self.progress
            .lock()
            .unwrap()
            .push((id.to_owned(), progress));
    }
}
