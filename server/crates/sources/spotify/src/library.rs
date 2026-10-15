//! The listener's saved Spotify music, scanned into the library like a folder of files: liked
//! songs and saved albums from the Web API through their developer app, and their playlists
//! from librespot, which Spotify's own apps use.
//!
//! A playlist's tracks are not brought into the library: the library asks Spotify's catalog
//! for them when the playlist is opened or played, so that a followed playlist does not fill
//! the library with albums nobody saved.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use bopsie_audio::engine::Input;
use bopsie_core::media::AudioFormat;
use bopsie_library::ids::IdKind;
use bopsie_library::provider::{
    Entry, EntryKind, Listing, MusicProvider, ProviderError, ScannedImage, ScannedPlaylist,
    ScannedTrack,
};
use librespot_core::{SpotifyId, SpotifyUri};
use librespot_metadata::Metadata as _;
use protobuf::Message;
use serde_json::Value;
use tokio::runtime::Handle;

use crate::account::SpotifyAccount;
use crate::catalog::SPOTIFY;
use crate::media::SpotifyMedia;

/// What a track's stamp is when its metadata is not expected to change.
const SETTLED: &str = "1";

/// What Spotify's audio is: Ogg Vorbis at 320 kbps, or less on a slower account.
fn audio_format() -> AudioFormat {
    AudioFormat {
        content_type: "ogg".to_owned(),
        codec_type: "vorbis".to_owned(),
        sample_rate: 44_100,
        bit_depth: 16,
        channels: 2,
        output_format_str: "ogg".to_owned(),
        bit_rate: 320,
    }
}

fn names(object: &Value, key: &str) -> Vec<String> {
    object[key]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| a["name"].as_str())
        .map(str::to_owned)
        .collect()
}

fn artist_ids(object: &Value) -> Vec<String> {
    let artists = object["artists"].as_array().cloned().unwrap_or_default();
    let ids: Vec<String> = artists
        .iter()
        .filter_map(|a| a["id"].as_str())
        .map(str::to_owned)
        .collect();
    // Only when each artist has one, so that they line up with the names.
    if ids.len() == artists.len() {
        ids
    } else {
        Vec::new()
    }
}

fn best_image(object: &Value) -> Option<ScannedImage> {
    object["images"]
        .as_array()?
        .iter()
        .max_by_key(|i| i["width"].as_u64().unwrap_or(0))
        .and_then(|i| i["url"].as_str())
        .map(|url| ScannedImage::Path(url.to_owned()))
}

/// A Web API track as the scanner takes it; `album` stands in for a simplified track's.
pub fn scanned_web_track(track: &Value, album: Option<&Value>) -> ScannedTrack {
    let album = album.unwrap_or(&track["album"]);
    ScannedTrack {
        title: track["name"].as_str().unwrap_or_default().to_owned(),
        artists: names(track, "artists"),
        album: album["name"].as_str().map(str::to_owned),
        album_artists: names(album, "artists"),
        year: album["release_date"]
            .as_str()
            .and_then(|date| date.get(..4)?.parse().ok()),
        track_number: track["track_number"].as_u64().map(|n| n as u32),
        disc_number: track["disc_number"].as_u64().map(|n| n as u32),
        duration_secs: (track["duration_ms"].as_u64().unwrap_or(0) / 1_000) as u32,
        audio_format: audio_format(),
        image: best_image(album),
        album_id: album["id"].as_str().map(str::to_owned),
        ids: web_ids(track, "isrc", IdKind::Isrc),
        album_ids: web_ids(album, "upc", IdKind::Barcode),
        artist_spotify_ids: artist_ids(track),
        album_artist_spotify_ids: artist_ids(album),
        ..ScannedTrack::default()
    }
}

/// An object's Spotify id, and its code of `code` from its `external_ids` as `kind`.
fn web_ids(object: &Value, code: &str, kind: IdKind) -> Vec<(IdKind, String)> {
    let spotify = object["id"]
        .as_str()
        .map(|id| (IdKind::Spotify, id.to_owned()));
    let code = object["external_ids"][code]
        .as_str()
        .map(|value| (kind, value.to_owned()));
    spotify.into_iter().chain(code).collect()
}

/// librespot's external ids of `code`, such as `isrc` or `upc`, as `kind`.
fn librespot_ids(
    ids: &librespot_metadata::external_id::ExternalIds,
    code: &str,
    kind: IdKind,
) -> impl Iterator<Item = (IdKind, String)> {
    ids.0
        .iter()
        .filter(|id| id.external_type.eq_ignore_ascii_case(code))
        .map(move |id| (kind, id.id.clone()))
        .collect::<Vec<_>>()
        .into_iter()
}

/// librespot's artists' Spotify ids, when each has one.
fn librespot_artist_ids(artists: &librespot_metadata::artist::Artists) -> Vec<String> {
    let ids: Vec<String> = artists.iter().filter_map(|a| a.id.to_id().ok()).collect();
    if ids.len() == artists.len() {
        ids
    } else {
        Vec::new()
    }
}

/// A track as librespot's metadata has it, as the scanner takes it.
pub fn scanned_librespot_track(track: &librespot_metadata::Track) -> ScannedTrack {
    let album = &track.album;
    ScannedTrack {
        title: track.name.clone(),
        artists: track.artists.iter().map(|a| a.name.clone()).collect(),
        album: Some(album.name.clone()).filter(|name| !name.is_empty()),
        album_artists: album.artists.iter().map(|a| a.name.clone()).collect(),
        year: Some(album.date.as_utc().year()),
        track_number: u32::try_from(track.number).ok(),
        disc_number: u32::try_from(track.disc_number).ok(),
        duration_secs: (track.duration.max(0) / 1_000) as u32,
        audio_format: audio_format(),
        image: album
            .covers
            .iter()
            .max_by_key(|cover| cover.width)
            .and_then(|cover| cover.id.to_base16().ok())
            .map(|id| ScannedImage::Path(format!("https://i.scdn.co/image/{id}"))),
        album_id: album.id.to_id().ok(),
        ids: track
            .id
            .to_id()
            .ok()
            .map(|id| (IdKind::Spotify, id))
            .into_iter()
            .chain(librespot_ids(&track.external_ids, "isrc", IdKind::Isrc))
            .collect(),
        album_ids: album
            .id
            .to_id()
            .ok()
            .map(|id| (IdKind::Spotify, id))
            .into_iter()
            .chain(librespot_ids(&album.external_ids, "upc", IdKind::Barcode))
            .collect(),
        artist_spotify_ids: librespot_artist_ids(&track.artists),
        album_artist_spotify_ids: librespot_artist_ids(&album.artists),
        ..ScannedTrack::default()
    }
}

/// A playlist's artwork as librespot has it: the URL Spotify made for it, as it does for its
/// own mixes, which the Web API keeps from developer apps, else the cover it was given.
fn playlist_picture(
    attributes: &librespot_metadata::playlist::attribute::PlaylistAttributes,
) -> Option<String> {
    attributes
        .picture_sizes
        .iter()
        .find(|size| size.target_name == "default")
        .or_else(|| attributes.picture_sizes.first())
        .map(|size| size.url.clone())
        .filter(|url| !url.is_empty())
        .or_else(|| {
            (!attributes.picture.is_empty()).then(|| {
                format!(
                    "https://i.scdn.co/image/{}",
                    hex::encode(&attributes.picture)
                )
            })
        })
}

#[derive(Default)]
struct Listed {
    tracks: HashMap<String, ScannedTrack>,
    playlists: HashMap<String, ScannedPlaylist>,
}

pub struct SpotifyLibrary {
    account: SpotifyAccount,
    runtime: Handle,
    listed: Arc<Mutex<Listed>>,
}

impl SpotifyLibrary {
    /// Needs a Tokio runtime, which Spotify is called on.
    pub fn new(account: SpotifyAccount) -> Self {
        Self {
            account,
            runtime: Handle::current(),
            listed: Arc::default(),
        }
    }

    async fn list_async(&self) -> Result<Listing, ProviderError> {
        let failed =
            |error: crate::account::AccountError| ProviderError::Unreadable(error.to_string());
        let mut listed = Listed::default();
        let mut entries = Vec::new();
        let mut seen = HashSet::new();
        let mut add_track = |id: &str, scanned: Option<ScannedTrack>, entries: &mut Vec<Entry>| {
            if let Some(scanned) = scanned {
                listed.tracks.insert(id.to_owned(), scanned);
            }
            if seen.insert(id.to_owned()) {
                entries.push(Entry {
                    id: id.to_owned(),
                    kind: EntryKind::Track,
                    stamp: SETTLED.to_owned(),
                });
            }
        };

        for saved in self
            .account
            .get_all("me/tracks?limit=50", |page| page)
            .await
            .map_err(failed)?
        {
            let track = &saved["track"];
            if let Some(id) = track["id"].as_str() {
                add_track(id, Some(scanned_web_track(track, None)), &mut entries);
            }
        }
        for saved in self
            .account
            .get_all("me/albums?limit=50", |page| page)
            .await
            .map_err(failed)?
        {
            let album = &saved["album"];
            let mut tracks = album["tracks"]["items"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            if let Some(next) = album["tracks"]["next"].as_str() {
                tracks.extend(
                    self.account
                        .get_all(next, |page| page)
                        .await
                        .map_err(failed)?,
                );
            }
            for track in &tracks {
                if let Some(id) = track["id"].as_str() {
                    add_track(
                        id,
                        Some(scanned_web_track(track, Some(album))),
                        &mut entries,
                    );
                }
            }
        }

        // Playlists' artwork, Spotify's mosaics included, from one paged list.
        let artwork: HashMap<String, String> = match self
            .account
            .get_all("me/playlists?limit=50", |page| page)
            .await
        {
            Ok(playlists) => playlists
                .iter()
                .filter_map(|p| {
                    let image = best_image(p)?;
                    let ScannedImage::Path(url) = image else {
                        return None;
                    };
                    Some((p["id"].as_str()?.to_owned(), url))
                })
                .collect(),
            Err(error) => {
                tracing::debug!(%error, "no artwork for Spotify playlists");
                HashMap::new()
            }
        };
        let session = self.account.session().await.map_err(failed)?;
        let rootlist = session
            .spclient()
            .get_rootlist(0, Some(1_000))
            .await
            .map_err(|e| ProviderError::Unreadable(e.to_string()))?;
        let rootlist =
            librespot_protocol::playlist4_external::SelectedListContent::parse_from_bytes(
                &rootlist,
            )
            .map_err(|e| ProviderError::Unreadable(e.to_string()))?;
        for item in &rootlist.contents.items {
            let Ok(uri @ SpotifyUri::Playlist { .. }) = SpotifyUri::from_uri(item.uri()) else {
                continue;
            };
            let Ok(playlist) = librespot_metadata::Playlist::get(&session, &uri).await else {
                tracing::debug!(playlist = item.uri(), "cannot read a Spotify playlist");
                continue;
            };
            let id = uri.to_id().unwrap_or_default();
            let image = artwork
                .get(&id)
                .cloned()
                .or_else(|| playlist_picture(&playlist.attributes));
            entries.push(Entry {
                id: id.clone(),
                kind: EntryKind::Playlist,
                // A new revision or new artwork reads it again.
                stamp: format!(
                    "{} {}",
                    hex::encode(&playlist.revision),
                    image.as_deref().unwrap_or_default()
                ),
            });
            listed.playlists.insert(
                id,
                ScannedPlaylist {
                    name: playlist.attributes.name.clone(),
                    // Spotify's catalog gives them, when the playlist is opened or played.
                    track_ids: Vec::new(),
                    image: image.map(ScannedImage::Path),
                },
            );
        }
        *self.listed.lock().unwrap() = listed;
        Ok(Listing {
            entries,
            unreadable: Vec::new(),
        })
    }
}

impl MusicProvider for SpotifyLibrary {
    fn domain(&self) -> &'static str {
        SPOTIFY
    }

    fn instance_id(&self) -> &str {
        SPOTIFY
    }

    fn name(&self) -> &str {
        "Spotify"
    }

    fn list(&self) -> Result<Listing, ProviderError> {
        if !self.account.web_logged_in() {
            return Err(ProviderError::Unreadable(
                "not logged in to Spotify's Web API".to_owned(),
            ));
        }
        self.runtime.block_on(self.list_async())
    }

    fn read_track(&self, id: &str) -> Result<ScannedTrack, ProviderError> {
        if let Some(track) = self.listed.lock().unwrap().tracks.get(id) {
            return Ok(track.clone());
        }
        let track_id = SpotifyId::from_base62(id)
            .map_err(|_| ProviderError::Unreadable(format!("not a Spotify track: {id}")))?;
        self.runtime.block_on(async {
            let session = self
                .account
                .session()
                .await
                .map_err(|e| ProviderError::Unreadable(e.to_string()))?;
            let track =
                librespot_metadata::Track::get(&session, &SpotifyUri::Track { id: track_id })
                    .await
                    .map_err(|e| ProviderError::Unreadable(e.to_string()))?;
            Ok(scanned_librespot_track(&track))
        })
    }

    fn read_playlist(&self, id: &str) -> Result<ScannedPlaylist, ProviderError> {
        self.listed
            .lock()
            .unwrap()
            .playlists
            .get(id)
            .cloned()
            .ok_or_else(|| ProviderError::Unreadable(format!("playlist {id} was not listed")))
    }

    fn image(&self, path: &str) -> Result<Vec<u8>, ProviderError> {
        self.runtime
            .block_on(self.account.download(path))
            .map_err(|e| ProviderError::Unreadable(e.to_string()))
    }

    fn file(&self, _id: &str) -> Option<std::path::PathBuf> {
        None
    }

    fn separate_albums(&self) -> bool {
        true
    }

    fn media(&self, id: &str) -> Option<Input> {
        let id = SpotifyId::from_base62(id).ok()?;
        Some(Input::Open(Arc::new(SpotifyMedia::new(
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

    #[test]
    fn a_saved_albums_track() {
        let album = json!({
            "id": "4aawyAB9vmqN3uQ7FjRGTy",
            "name": "Global Warming",
            "release_date": "2012-11-16",
            "external_ids": {"upc": "886443671584"},
            "artists": [{"id": "0TnOYISbd1XYRBk9myaseg", "name": "Pitbull"}],
            "images": [
                {"url": "https://i.scdn.co/image/a", "width": 640},
                {"url": "https://i.scdn.co/image/b", "width": 300}
            ]
        });
        let track = json!({
            "id": "6OmhkSOpvYBokMKQxpIGx2",
            "name": "Global Warming (feat. Sensato)",
            "duration_ms": 85400,
            "disc_number": 1,
            "track_number": 1,
            "external_ids": {"isrc": "USRC11200786"},
            "artists": [
                {"id": "0TnOYISbd1XYRBk9myaseg", "name": "Pitbull"},
                {"id": "7iJrDbKM5fEkGdm5kpjFzS", "name": "Sensato"}
            ]
        });
        let scanned = scanned_web_track(&track, Some(&album));
        assert_eq!(scanned.title, "Global Warming (feat. Sensato)");
        assert_eq!(scanned.artists, ["Pitbull", "Sensato"]);
        assert_eq!(scanned.album.as_deref(), Some("Global Warming"));
        assert_eq!(scanned.album_artists, ["Pitbull"]);
        assert_eq!(
            scanned.artist_spotify_ids,
            ["0TnOYISbd1XYRBk9myaseg", "7iJrDbKM5fEkGdm5kpjFzS"]
        );
        assert_eq!(scanned.album_artist_spotify_ids, ["0TnOYISbd1XYRBk9myaseg"]);
        assert_eq!(scanned.year, Some(2012));
        assert_eq!(scanned.duration_secs, 85);
        assert_eq!(scanned.album_id.as_deref(), Some("4aawyAB9vmqN3uQ7FjRGTy"));
        assert_eq!(
            scanned.ids,
            [
                (IdKind::Spotify, "6OmhkSOpvYBokMKQxpIGx2".to_owned()),
                (IdKind::Isrc, "USRC11200786".to_owned())
            ]
        );
        assert_eq!(
            scanned.album_ids,
            [
                (IdKind::Spotify, "4aawyAB9vmqN3uQ7FjRGTy".to_owned()),
                (IdKind::Barcode, "886443671584".to_owned())
            ]
        );
        assert_eq!(
            scanned.image,
            Some(ScannedImage::Path("https://i.scdn.co/image/a".to_owned()))
        );
    }
}
