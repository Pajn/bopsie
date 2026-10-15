//! Spotify as a catalog: searched and browsed as it is, beside the library.
//!
//! Search, items and albums come from Spotify's Web API through the listener's developer app.
//! What that app may not read in development mode, an artist's top tracks and a followed
//! playlist's tracks, comes from librespot's own metadata, which Spotify's apps use.

use bopsie_audio::engine::Input;
use bopsie_core::media::{
    Album, Artist, ArtistRef, Image, ItemFields, ItemMapping, MediaItem, MediaType, Metadata,
    Playlist, Track, sort_name,
};
use bopsie_library::catalog::{self, Catalog, CatalogError, CatalogResult};
use futures::StreamExt;
use futures::future::BoxFuture;
use librespot_core::{Session, SpotifyId, SpotifyUri};
use librespot_metadata::Metadata as _;
use serde_json::Value;
use tokio::runtime::Handle;

use crate::account::{AccountError, SpotifyAccount};
use crate::media::SpotifyMedia;

/// The catalog's provider id and its URIs' scheme.
pub const SPOTIFY: &str = "spotify";
/// The most a developer app's search returns per type.
const SEARCH_LIMIT: usize = 10;
/// How many tracks' metadata librespot fetches at once.
const AT_ONCE: usize = 8;

pub struct SpotifyCatalog {
    account: SpotifyAccount,
    runtime: Handle,
    /// Each playlist's tracks as of its revision, so that opening it again costs one call.
    playlists: std::sync::Mutex<std::collections::HashMap<String, Revised>>,
}

/// A playlist's tracks, and the revision they are of.
type Revised = (Vec<u8>, Vec<Track>);

impl SpotifyCatalog {
    /// Needs a Tokio runtime, which tracks are fetched on.
    pub fn new(account: SpotifyAccount) -> Self {
        Self {
            account,
            runtime: Handle::current(),
            playlists: std::sync::Mutex::default(),
        }
    }
}

fn unavailable(error: AccountError) -> CatalogError {
    match error {
        AccountError::Refused(why) if why.starts_with("404") => CatalogError::NotFound(why),
        other => CatalogError::Unavailable(other.to_string()),
    }
}

fn librespot(error: librespot_core::Error) -> CatalogError {
    CatalogError::Unavailable(error.to_string())
}

fn image(url: &str) -> Image {
    Image {
        kind: "thumb",
        path: url.to_owned(),
        provider: SPOTIFY.to_owned(),
        remotely_accessible: true,
        // Spotify's own URL is used as it is.
        proxy_id: String::new(),
    }
}

/// The largest of a Web API object's images.
fn best_image(object: &Value) -> Option<Image> {
    object["images"]
        .as_array()?
        .iter()
        .max_by_key(|i| i["width"].as_u64().unwrap_or(0))
        .and_then(|i| i["url"].as_str())
        .map(image)
}

fn fields(media_type: MediaType, id: &str, name: &str, images: Option<Image>) -> ItemFields {
    ItemFields {
        item_id: id.to_owned(),
        provider: SPOTIFY.to_owned(),
        name: name.to_owned(),
        version: String::new(),
        sort_name: sort_name(name),
        uri: catalog::uri(SPOTIFY, media_type, id),
        external_ids: Vec::new(),
        is_playable: true,
        media_type,
        provider_mappings: Vec::new(),
        metadata: Metadata {
            images: images.map(|image| vec![image]),
            ..Metadata::default()
        },
        favorite: false,
        position: None,
        date_added: None,
    }
}

fn mapping(media_type: MediaType, id: &str, name: &str, image: Option<Image>) -> ItemMapping {
    ItemMapping {
        item_id: id.to_owned(),
        provider: SPOTIFY.to_owned(),
        name: name.to_owned(),
        version: String::new(),
        sort_name: sort_name(name),
        uri: catalog::uri(SPOTIFY, media_type, id),
        external_ids: Vec::new(),
        is_playable: true,
        media_type,
        available: true,
        image,
        year: None,
    }
}

fn text<'a>(object: &'a Value, key: &str) -> &'a str {
    object[key].as_str().unwrap_or_default()
}

fn year(release_date: &str) -> Option<i32> {
    release_date.get(..4)?.parse().ok()
}

/// A Web API artist, full or simplified.
pub fn web_artist(object: &Value) -> Artist {
    Artist {
        fields: fields(
            MediaType::Artist,
            text(object, "id"),
            text(object, "name"),
            best_image(object),
        ),
        artist_type: "artist",
    }
}

fn web_artist_mappings(object: &Value) -> Vec<ItemMapping> {
    object["artists"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|a| mapping(MediaType::Artist, text(a, "id"), text(a, "name"), None))
        .collect()
}

/// A Web API album, full or simplified.
pub fn web_album(object: &Value) -> Album {
    let mut album = Album {
        fields: fields(
            MediaType::Album,
            text(object, "id"),
            text(object, "name"),
            best_image(object),
        ),
        year: year(text(object, "release_date")),
        artists: web_artist_mappings(object)
            .into_iter()
            .map(|m| ArtistRef::Mapping(Box::new(m)))
            .collect(),
        album_type: match text(object, "album_type") {
            "single" => "single",
            "compilation" => "compilation",
            _ => "album",
        },
    };
    if let Some(upc) = object["external_ids"]["upc"].as_str() {
        album.fields.external_ids = vec![serde_json::json!(["barcode", upc])];
    }
    album
}

/// A Web API track; `album` stands in for the album of a simplified track, which has none.
pub fn web_track(object: &Value, album: Option<&Value>) -> Track {
    let album_object = album.unwrap_or(&object["album"]);
    let album = (!text(album_object, "id").is_empty()).then(|| {
        let mut mapping = mapping(
            MediaType::Album,
            text(album_object, "id"),
            text(album_object, "name"),
            best_image(album_object),
        );
        mapping.year = year(text(album_object, "release_date"));
        mapping
    });
    let mut track = Track {
        fields: fields(
            MediaType::Track,
            text(object, "id"),
            text(object, "name"),
            best_image(album_object),
        ),
        duration: (object["duration_ms"].as_u64().unwrap_or(0) / 1_000) as u32,
        artists: web_artist_mappings(object),
        last_played: 0,
        album,
        disc_number: object["disc_number"].as_u64().unwrap_or(1) as u32,
        track_number: object["track_number"].as_u64().unwrap_or(0) as u32,
        audio_metadata: None,
    };
    if let Some(isrc) = object["external_ids"]["isrc"].as_str() {
        track.fields.external_ids = vec![serde_json::json!(["isrc", isrc])];
    }
    track
}

/// A Web API playlist, full or simplified.
pub fn web_playlist(object: &Value) -> Playlist {
    Playlist {
        fields: fields(
            MediaType::Playlist,
            text(object, "id"),
            text(object, "name"),
            best_image(object),
        ),
        owner: object["owner"]["display_name"]
            .as_str()
            .or(object["owner"]["id"].as_str())
            .unwrap_or_default()
            .to_owned(),
        is_editable: false,
        is_dynamic: false,
        supported_mediatypes: vec![MediaType::Track],
    }
}

fn cover_url(id: &librespot_core::FileId) -> Option<String> {
    Some(format!("https://i.scdn.co/image/{}", id.to_base16().ok()?))
}

/// A track as librespot's metadata has it.
pub fn librespot_track(track: &librespot_metadata::Track) -> Track {
    let id = track.id.to_id().unwrap_or_default();
    let cover = track
        .album
        .covers
        .iter()
        .max_by_key(|cover| cover.width)
        .and_then(|cover| cover_url(&cover.id))
        .map(|url| image(&url));
    let album_id = track.album.id.to_id().unwrap_or_default();
    let mut album = mapping(
        MediaType::Album,
        &album_id,
        &track.album.name,
        cover.clone(),
    );
    album.year = Some(track.album.date.as_utc().year());
    Track {
        fields: fields(MediaType::Track, &id, &track.name, cover),
        duration: (track.duration.max(0) / 1_000) as u32,
        artists: track
            .artists
            .iter()
            .map(|artist| {
                mapping(
                    MediaType::Artist,
                    &artist.id.to_id().unwrap_or_default(),
                    &artist.name,
                    None,
                )
            })
            .collect(),
        last_played: 0,
        album: (!album_id.is_empty()).then_some(album),
        disc_number: track.disc_number.max(1) as u32,
        track_number: track.number.max(0) as u32,
        audio_metadata: None,
    }
}

/// The tracks `uris` name, fetched a few at a time over librespot, in order, leaving out any
/// that cannot be.
async fn librespot_tracks(session: &Session, uris: Vec<SpotifyUri>) -> Vec<Track> {
    let fetched: Vec<Option<Track>> = futures::stream::iter(uris)
        .map(|uri| async move {
            if !matches!(uri, SpotifyUri::Track { .. }) {
                return None;
            }
            librespot_metadata::Track::get(session, &uri)
                .await
                .ok()
                .map(|track| librespot_track(&track))
        })
        .buffered(AT_ONCE)
        .collect()
        .await;
    fetched.into_iter().flatten().collect()
}

impl SpotifyCatalog {
    async fn album_tracks_of(&self, id: &str) -> CatalogResult<Vec<Track>> {
        let album = self
            .account
            .get(&format!("albums/{id}"))
            .await
            .map_err(unavailable)?;
        let mut items = album["tracks"]["items"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if let Some(next) = album["tracks"]["next"].as_str() {
            let rest = self
                .account
                .get_all(next, |page| page)
                .await
                .map_err(unavailable)?;
            items.extend(rest);
        }
        Ok(items.iter().map(|t| web_track(t, Some(&album))).collect())
    }
}

impl Catalog for SpotifyCatalog {
    fn instance_id(&self) -> &str {
        SPOTIFY
    }

    fn domain(&self) -> &str {
        SPOTIFY
    }

    fn name(&self) -> &str {
        "Spotify"
    }

    fn available(&self) -> bool {
        self.account.web_logged_in()
    }

    fn search<'a>(
        &'a self,
        query: &'a str,
        media_type: MediaType,
        limit: usize,
    ) -> BoxFuture<'a, CatalogResult<Vec<MediaItem>>> {
        Box::pin(async move {
            let path = format!(
                "search?q={}&type={}&limit={}",
                crate::account::urlencode(query),
                media_type.as_str(),
                limit.clamp(1, SEARCH_LIMIT)
            );
            let found = self.account.get(&path).await.map_err(unavailable)?;
            let items = found[format!("{}s", media_type.as_str())]["items"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            Ok(items
                .iter()
                // Spotify's search pads its answer with nulls for items it withholds.
                .filter(|item| item.is_object())
                .map(|item| match media_type {
                    MediaType::Artist => MediaItem::Artist(Box::new(web_artist(item))),
                    MediaType::Album => MediaItem::Album(Box::new(web_album(item))),
                    MediaType::Track => MediaItem::Track(Box::new(web_track(item, None))),
                    MediaType::Playlist => MediaItem::Playlist(Box::new(web_playlist(item))),
                })
                .collect())
        })
    }

    fn item<'a>(
        &'a self,
        media_type: MediaType,
        id: &'a str,
    ) -> BoxFuture<'a, CatalogResult<MediaItem>> {
        Box::pin(async move {
            let path = match media_type {
                MediaType::Artist => format!("artists/{id}"),
                MediaType::Album => format!("albums/{id}"),
                MediaType::Track => format!("tracks/{id}"),
                MediaType::Playlist => {
                    format!("playlists/{id}?fields=id,name,owner,images")
                }
            };
            let object = self.account.get(&path).await.map_err(unavailable)?;
            Ok(match media_type {
                MediaType::Artist => MediaItem::Artist(Box::new(web_artist(&object))),
                MediaType::Album => MediaItem::Album(Box::new(web_album(&object))),
                MediaType::Track => MediaItem::Track(Box::new(web_track(&object, None))),
                MediaType::Playlist => MediaItem::Playlist(Box::new(web_playlist(&object))),
            })
        })
    }

    fn album_tracks<'a>(&'a self, id: &'a str) -> BoxFuture<'a, CatalogResult<Vec<Track>>> {
        Box::pin(self.album_tracks_of(id))
    }

    fn artist_albums<'a>(&'a self, id: &'a str) -> BoxFuture<'a, CatalogResult<Vec<Album>>> {
        Box::pin(async move {
            let albums = self
                .account
                .get_all(
                    &format!(
                        "artists/{id}/albums?include_groups=album,single,compilation&limit=50"
                    ),
                    |page| page,
                )
                .await
                .map_err(unavailable)?;
            Ok(albums.iter().map(web_album).collect())
        })
    }

    fn artist_tracks<'a>(&'a self, id: &'a str) -> BoxFuture<'a, CatalogResult<Vec<Track>>> {
        Box::pin(async move {
            let session = self.account.session().await.map_err(unavailable)?;
            let artist_id = SpotifyId::from_base62(id)
                .map_err(|_| CatalogError::NotFound(format!("artist {id} not found")))?;
            let artist =
                librespot_metadata::Artist::get(&session, &SpotifyUri::Artist { id: artist_id })
                    .await
                    .map_err(librespot)?;
            let top = artist.top_tracks.for_country(&session.country());
            Ok(librespot_tracks(&session, top.0).await)
        })
    }

    fn playlist_tracks<'a>(&'a self, id: &'a str) -> BoxFuture<'a, CatalogResult<Vec<Track>>> {
        Box::pin(async move {
            let session = self.account.session().await.map_err(unavailable)?;
            let uri = SpotifyUri::from_uri(&format!("spotify:playlist:{id}"))
                .map_err(|_| CatalogError::NotFound(format!("playlist {id} not found")))?;
            let playlist = librespot_metadata::Playlist::get(&session, &uri)
                .await
                .map_err(librespot)?;
            if let Some((revision, tracks)) = self.playlists.lock().unwrap().get(id)
                && *revision == playlist.revision
            {
                return Ok(tracks.clone());
            }
            let uris = playlist.tracks().cloned().collect();
            let tracks = librespot_tracks(&session, uris).await;
            self.playlists
                .lock()
                .unwrap()
                .insert(id.to_owned(), (playlist.revision.clone(), tracks.clone()));
            Ok(tracks)
        })
    }

    fn artist_images<'a>(
        &'a self,
        ids: &'a [String],
    ) -> BoxFuture<'a, CatalogResult<Vec<(String, String)>>> {
        Box::pin(async move {
            let mut out = Vec::new();
            // The Web API takes 50 artists a request.
            for chunk in ids.chunks(50) {
                let answer = self
                    .account
                    .get(&format!("artists?ids={}", chunk.join(",")))
                    .await
                    .map_err(unavailable)?;
                for artist in answer["artists"].as_array().into_iter().flatten() {
                    let url = artist["images"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .max_by_key(|i| i["width"].as_u64().unwrap_or(0))
                        .and_then(|i| i["url"].as_str());
                    if let (Some(id), Some(url)) = (artist["id"].as_str(), url) {
                        out.push((id.to_owned(), url.to_owned()));
                    }
                }
            }
            Ok(out)
        })
    }

    fn media(&self, track_id: &str) -> Option<Input> {
        let id = SpotifyId::from_base62(track_id).ok()?;
        Some(Input::Open(std::sync::Arc::new(SpotifyMedia::new(
            self.account.clone(),
            id,
            self.runtime.clone(),
        ))))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// As the Web API's reference shows a track, shortened.
    fn track_json() -> Value {
        json!({
            "id": "11dFghVXANMlKmJXsNCbNl",
            "name": "Cut To The Feeling",
            "duration_ms": 207959,
            "disc_number": 1,
            "track_number": 1,
            "external_ids": {"isrc": "USUM71703861"},
            "artists": [{"id": "6sFIWsNpZYqfjUpaCgueju", "name": "Carly Rae Jepsen"}],
            "album": {
                "id": "0tGPJ0bkWOUmH7MEOR77qc",
                "name": "Cut To The Feeling",
                "release_date": "2017-05-26",
                "album_type": "single",
                "images": [
                    {"url": "https://i.scdn.co/image/small", "width": 64, "height": 64},
                    {"url": "https://i.scdn.co/image/large", "width": 640, "height": 640}
                ],
                "artists": [{"id": "6sFIWsNpZYqfjUpaCgueju", "name": "Carly Rae Jepsen"}]
            }
        })
    }

    #[test]
    fn a_web_api_track() {
        let track = web_track(&track_json(), None);
        assert_eq!(track.fields.uri, "spotify://track/11dFghVXANMlKmJXsNCbNl");
        assert_eq!(track.fields.provider, "spotify");
        assert_eq!(track.duration, 207);
        assert_eq!(track.artists[0].name, "Carly Rae Jepsen");
        let album = track.album.as_ref().unwrap();
        assert_eq!(album.uri, "spotify://album/0tGPJ0bkWOUmH7MEOR77qc");
        assert_eq!(album.year, Some(2017));
        let image = &track.fields.metadata.images.as_ref().unwrap()[0];
        assert_eq!(image.path, "https://i.scdn.co/image/large");
        assert!(image.remotely_accessible);
        assert_eq!(track.fields.external_ids, [json!(["isrc", "USUM71703861"])]);
    }

    #[test]
    fn an_albums_simplified_tracks_take_the_album() {
        let album = track_json()["album"].clone();
        let mut simplified = track_json();
        simplified.as_object_mut().unwrap().remove("album");
        let track = web_track(&simplified, Some(&album));
        assert_eq!(track.album.unwrap().name, "Cut To The Feeling");
        let album = web_album(&album);
        assert_eq!(album.album_type, "single");
        assert_eq!(album.year, Some(2017));
    }

    #[test]
    fn a_web_api_playlist() {
        let playlist = web_playlist(&json!({
            "id": "37i9dQZF1DXcBWIGoYBM5M",
            "name": "Today's Top Hits",
            "owner": {"id": "spotify", "display_name": "Spotify"},
            "images": []
        }));
        assert_eq!(playlist.owner, "Spotify");
        assert_eq!(
            playlist.fields.uri,
            "spotify://playlist/37i9dQZF1DXcBWIGoYBM5M"
        );
        assert_eq!(playlist.fields.metadata.images, None);
    }
}
