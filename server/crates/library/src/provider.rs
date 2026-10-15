//! The `MusicProvider` trait: a source of music the library scans. Methods block, and the
//! scanner calls them on blocking threads.

use bopsie_audio::engine::Input;
use bopsie_core::media::AudioFormat;

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Unreadable(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Track,
    Playlist,
}

/// One item a provider holds, as cheap to list as possible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Stable within the provider, such as a path relative to its root.
    pub id: String,
    pub kind: EntryKind,
    /// Changes whenever the item does, such as its size and modification time. The scanner
    /// reads only items whose stamp differs from the last scan.
    pub stamp: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScannedImage {
    /// An image file the provider serves through [`MusicProvider::image`].
    Path(String),
    /// Art embedded in the item. The library stores it in its artwork cache.
    Embedded { data: Vec<u8>, extension: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedTrack {
    pub title: String,
    pub artists: Vec<String>,
    pub album: Option<String>,
    /// Empty when the tags name none; the album then belongs to the track artists.
    pub album_artists: Vec<String>,
    pub year: Option<i32>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub duration_secs: u32,
    pub audio_format: AudioFormat,
    pub image: Option<ScannedImage>,
    /// What the provider calls the album, such as its folder, for the album's provider mapping.
    pub album_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedPlaylist {
    pub name: String,
    /// Provider ids of the tracks, in order. Ids the provider does not hold are skipped.
    pub track_ids: Vec<String>,
    pub image: Option<ScannedImage>,
}

/// What a provider has, as far as it could see.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listing {
    pub entries: Vec<Entry>,
    /// Item id prefixes it could not look under, such as an unreadable folder. What the library
    /// knew there is kept, not taken as gone.
    pub unreadable: Vec<String>,
}

impl Listing {
    /// Whether item `id` lies where the provider could not look.
    pub fn hidden(&self, id: &str) -> bool {
        self.unreadable.iter().any(|prefix| {
            prefix.is_empty()
                || id == prefix
                || id
                    .strip_prefix(prefix.as_str())
                    .is_some_and(|rest| rest.starts_with('/'))
        })
    }
}

pub trait MusicProvider: Send + Sync + 'static {
    /// The kind of provider, such as `filesystem_local`.
    fn domain(&self) -> &'static str;
    /// This instance, such as `filesystem_local--3f2a9c1e`. Stable across restarts.
    fn instance_id(&self) -> &str;
    fn list(&self) -> Result<Listing, ProviderError>;
    fn read_track(&self, id: &str) -> Result<ScannedTrack, ProviderError>;
    fn read_playlist(&self, id: &str) -> Result<ScannedPlaylist, ProviderError>;
    /// The bytes of an image named by [`ScannedImage::Path`].
    fn image(&self, path: &str) -> Result<Vec<u8>, ProviderError>;
    /// The local file a track plays from, if the provider has one.
    fn file(&self, id: &str) -> Option<std::path::PathBuf>;
    /// Whether its albums stay its own rather than merge with another provider's of the same
    /// name and artists, as a streaming service's do beside local files.
    fn separate_albums(&self) -> bool {
        false
    }
    /// What a track plays from: its file, or, for a provider that streams, its stream.
    fn media(&self, id: &str) -> Option<Input> {
        self.file(id).map(Input::File)
    }
}
