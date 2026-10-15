//! A catalog for tests: one artist with one album of two tracks, and a playlist, each track
//! playing from a WAV file.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bopsie_audio::engine::Input;
use bopsie_core::media::{
    Album, Artist, ArtistRef, ItemFields, ItemMapping, MediaItem, MediaType, Metadata, Playlist,
    Track, sort_name,
};
use bopsie_library::Library;
use bopsie_library::catalog::{self, Catalog, CatalogError, CatalogResult};
use futures::future::BoxFuture;

pub const FAKE: &str = "fake";

pub struct FakeCatalog {
    files: PathBuf,
}

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

impl FakeCatalog {
    /// Adds the catalog to `library`, its tracks playing from WAV files written to `dir`.
    pub fn add_to(library: &Library, dir: &Path) {
        for track in tracks() {
            let path = dir.join(format!("{}.wav", track.fields.item_id));
            bopsie_fixtures::write_wav(&path, 44_100, 2, &bopsie_fixtures::noise(3, 44_100 * 4))
                .unwrap();
        }
        library.catalogs().add(Arc::new(Self {
            files: dir.to_owned(),
        }));
    }
}

fn all() -> Vec<MediaItem> {
    let mut items = vec![
        MediaItem::Artist(Box::new(artist())),
        MediaItem::Album(Box::new(album())),
        MediaItem::Playlist(Box::new(playlist())),
    ];
    items.extend(tracks().into_iter().map(|t| MediaItem::Track(Box::new(t))));
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
}
