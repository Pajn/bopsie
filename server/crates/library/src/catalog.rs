//! Catalogs: providers searched and browsed as they are, such as Spotify, rather than scanned
//! into the library. Their items carry the catalog's instance id as their provider and a URI
//! of its own, `<instance>://<type>/<id>`, and play through [`Catalog::media`].

use std::sync::Arc;
use std::time::Duration;

use bopsie_audio::engine::Input;
use bopsie_core::media::{Album, Audiobook, MediaItem, MediaType, Track};
use bopsie_core::{Error, ErrorCode};
use futures::future::BoxFuture;

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("{0}")]
    NotFound(String),
    /// Signed out, rate-limited or unreachable: it may work later.
    #[error("{0}")]
    Unavailable(String),
}

impl From<CatalogError> for Error {
    fn from(error: CatalogError) -> Self {
        match error {
            CatalogError::NotFound(what) => Error::new(ErrorCode::MediaNotFound, what),
            CatalogError::Unavailable(why) => Error::new(ErrorCode::ProviderUnavailable, why),
        }
    }
}

pub type CatalogResult<T> = Result<T, CatalogError>;

/// How far into an item listening is, for a catalog that keeps it, such as Audiobookshelf.
/// Positions are from the item's start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    /// Playing, this far in; reported about once a second.
    Playing(Duration),
    /// Paused, this far in.
    Paused(Duration),
    /// Moved to this far in, as by a seek or a chapter skip.
    Seeked(Duration),
    /// Played to its end.
    Ended,
    /// Left this far in, for another item or none.
    Stopped(Duration),
}

pub trait Catalog: Send + Sync {
    /// Its provider id, which is also its URIs' scheme, such as `spotify`.
    fn instance_id(&self) -> &str;
    /// Its kind, such as `spotify`.
    fn domain(&self) -> &str;
    /// What it is called, such as `Spotify`.
    fn name(&self) -> &str;
    /// Whether it can answer now, as when it is signed in.
    fn available(&self) -> bool {
        true
    }
    /// Items of `media_type` matching `query`, best first, at most `limit`.
    fn search<'a>(
        &'a self,
        query: &'a str,
        media_type: MediaType,
        limit: usize,
    ) -> BoxFuture<'a, CatalogResult<Vec<MediaItem>>>;
    fn item<'a>(
        &'a self,
        media_type: MediaType,
        id: &'a str,
    ) -> BoxFuture<'a, CatalogResult<MediaItem>>;
    fn album_tracks<'a>(&'a self, id: &'a str) -> BoxFuture<'a, CatalogResult<Vec<Track>>>;
    fn artist_albums<'a>(&'a self, id: &'a str) -> BoxFuture<'a, CatalogResult<Vec<Album>>>;
    fn artist_tracks<'a>(&'a self, id: &'a str) -> BoxFuture<'a, CatalogResult<Vec<Track>>>;
    fn playlist_tracks<'a>(&'a self, id: &'a str) -> BoxFuture<'a, CatalogResult<Vec<Track>>>;
    /// What track or audiobook `id` plays from.
    fn media(&self, track_id: &str) -> Option<Input>;
    /// A page of its audiobooks, in its own order: `limit` from `offset`.
    fn audiobooks<'a>(
        &'a self,
        _offset: usize,
        _limit: usize,
    ) -> BoxFuture<'a, CatalogResult<Vec<Audiobook>>> {
        Box::pin(async { Ok(Vec::new()) })
    }
    /// How many audiobooks it has.
    fn audiobook_count<'a>(&'a self) -> BoxFuture<'a, CatalogResult<usize>> {
        Box::pin(async { Ok(0) })
    }
    /// The audiobooks artist `id` wrote.
    fn artist_audiobooks<'a>(
        &'a self,
        _id: &'a str,
    ) -> BoxFuture<'a, CatalogResult<Vec<Audiobook>>> {
        Box::pin(async { Ok(Vec::new()) })
    }
    /// How far into its item `id` listening is, as playback reports it. It must not block.
    fn progress(&self, _id: &str, _progress: Progress) {}
    /// The bytes of an image it named by `path` with no URL of its own, as one its server
    /// serves only to those signed in to it. The library's image proxy serves it, once
    /// [`crate::Library::remember_images`] knows it.
    fn image<'a>(&'a self, _path: &'a str) -> BoxFuture<'a, CatalogResult<Option<Vec<u8>>>> {
        Box::pin(async { Ok(None) })
    }
    /// The URL of the largest image of each of `ids`'s artists the catalog has one of, by id.
    fn artist_images<'a>(
        &'a self,
        _ids: &'a [String],
    ) -> BoxFuture<'a, CatalogResult<Vec<(String, String)>>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

/// `spotify://album/abc`.
pub fn uri(instance: &str, media_type: MediaType, id: &str) -> String {
    format!("{instance}://{}/{id}", media_type.as_str())
}

/// The scheme, media type and id in a catalog URI.
pub fn parse_uri(uri: &str) -> Option<(&str, MediaType, &str)> {
    let (scheme, rest) = uri.split_once("://")?;
    let (kind, id) = rest.split_once('/')?;
    (!id.is_empty()).then_some((scheme, MediaType::parse(kind)?, id))
}

/// The catalogs the server has.
#[derive(Clone, Default)]
pub struct Catalogs {
    all: Arc<std::sync::RwLock<Vec<Arc<dyn Catalog>>>>,
}

impl Catalogs {
    pub fn add(&self, catalog: Arc<dyn Catalog>) {
        self.all.write().unwrap().push(catalog);
    }

    pub fn all(&self) -> Vec<Arc<dyn Catalog>> {
        self.all.read().unwrap().clone()
    }

    /// The catalog with this instance id, or of this domain.
    pub fn get(&self, instance_or_domain: &str) -> Option<Arc<dyn Catalog>> {
        let all = self.all.read().unwrap();
        all.iter()
            .find(|c| c.instance_id() == instance_or_domain)
            .or_else(|| all.iter().find(|c| c.domain() == instance_or_domain))
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uris_name_the_catalog_type_and_id() {
        assert_eq!(
            uri("spotify", MediaType::Album, "abc"),
            "spotify://album/abc"
        );
        assert_eq!(
            parse_uri("spotify://track/3eDY"),
            Some(("spotify", MediaType::Track, "3eDY"))
        );
        assert_eq!(parse_uri("spotify://track/"), None);
        assert_eq!(parse_uri("spotify://episode/1"), None);
        assert_eq!(parse_uri("not a uri"), None);
    }
}
