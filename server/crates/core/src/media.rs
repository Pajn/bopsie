//! Media items as Music Assistant serializes them (`music_assistant_models.media_items`), field
//! for field, in the full form its `get` commands return. Fields this server has no value for
//! are present and null, so clients that parse Music Assistant's models parse these.

use serde::Serialize;
use serde_json::Value;

/// The provider id of items in the library, and the scheme of their URIs.
pub const LIBRARY: &str = "library";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaType {
    Artist,
    Album,
    Track,
    Playlist,
    Audiobook,
}

impl MediaType {
    pub fn as_str(self) -> &'static str {
        match self {
            MediaType::Artist => "artist",
            MediaType::Album => "album",
            MediaType::Track => "track",
            MediaType::Playlist => "playlist",
            MediaType::Audiobook => "audiobook",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "artist" => Some(MediaType::Artist),
            "album" => Some(MediaType::Album),
            "track" => Some(MediaType::Track),
            "playlist" => Some(MediaType::Playlist),
            "audiobook" => Some(MediaType::Audiobook),
            _ => None,
        }
    }
}

/// `library://album/1`.
pub fn library_uri(media_type: MediaType, id: i64) -> String {
    format!("{LIBRARY}://{}/{id}", media_type.as_str())
}

/// The media type and library id in a `library://` URI.
pub fn parse_library_uri(uri: &str) -> Option<(MediaType, i64)> {
    let rest = uri.strip_prefix("library://")?;
    let (kind, id) = rest.split_once('/')?;
    Some((MediaType::parse(kind)?, id.parse().ok()?))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Image {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub path: String,
    pub provider: String,
    pub remotely_accessible: bool,
    /// The id `/imageproxy/<proxy_id>` serves this image under.
    pub proxy_id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct AudioFormat {
    pub content_type: String,
    pub codec_type: String,
    pub sample_rate: u32,
    pub bit_depth: u32,
    pub channels: u32,
    pub output_format_str: String,
    /// Kilobits per second.
    pub bit_rate: u32,
}

impl AudioFormat {
    /// What Music Assistant reports for items that are not audio files, such as albums.
    pub fn unknown() -> Self {
        Self {
            content_type: "?".to_owned(),
            codec_type: "?".to_owned(),
            sample_rate: 44100,
            bit_depth: 16,
            channels: 2,
            output_format_str: "?".to_owned(),
            bit_rate: 0,
        }
    }
}

/// Where a library item comes from: an item of one provider instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderMapping {
    pub item_id: String,
    pub provider_domain: String,
    pub provider_instance: String,
    pub available: bool,
    pub in_library: bool,
    pub is_unique: Option<bool>,
    pub audio_format: AudioFormat,
    pub url: Option<String>,
    pub details: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Metadata {
    pub description: Option<String>,
    pub description_language: Option<String>,
    pub life_span: Option<Value>,
    pub artist_entity_type: Option<String>,
    pub review: Option<String>,
    pub explicit: Option<bool>,
    pub images: Option<Vec<Image>>,
    pub grouping: Option<String>,
    pub genres: Option<Vec<String>>,
    pub mood: Option<String>,
    pub style: Option<String>,
    pub copyright: Option<String>,
    pub lyrics: Option<String>,
    pub lrc_lyrics: Option<String>,
    pub transcript: Option<String>,
    pub transcript_cues: Option<Value>,
    pub has_transcript: Option<bool>,
    pub label: Option<String>,
    pub links: Option<Value>,
    pub performers: Option<Value>,
    pub preview: Option<String>,
    pub popularity: Option<u32>,
    pub release_date: Option<String>,
    pub languages: Option<Vec<String>>,
    pub chapters: Option<Vec<Chapter>>,
    pub collections: Option<Value>,
    pub last_refresh: Option<i64>,
}

/// The fields every media item shares.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ItemFields {
    pub item_id: String,
    pub provider: String,
    pub name: String,
    pub version: String,
    pub sort_name: String,
    pub uri: String,
    pub external_ids: Vec<Value>,
    pub is_playable: bool,
    pub media_type: MediaType,
    pub provider_mappings: Vec<ProviderMapping>,
    pub metadata: Metadata,
    pub favorite: bool,
    pub position: Option<u32>,
    /// RFC 3339.
    pub date_added: Option<String>,
}

/// A short reference to another item, as albums list their artists and tracks their album.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ItemMapping {
    pub item_id: String,
    pub provider: String,
    pub name: String,
    pub version: String,
    pub sort_name: String,
    pub uri: String,
    pub external_ids: Vec<Value>,
    pub is_playable: bool,
    pub media_type: MediaType,
    pub available: bool,
    pub image: Option<Image>,
    pub year: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Artist {
    #[serde(flatten)]
    pub fields: ItemFields,
    /// Music Assistant's default, which it reports for every artist it knows nothing about.
    pub artist_type: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Album {
    #[serde(flatten)]
    pub fields: ItemFields,
    pub year: Option<i32>,
    pub artists: Vec<ArtistRef>,
    pub album_type: &'static str,
}

/// How an album names its artists: in full when the album is fetched on its own, as
/// references in lists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum ArtistRef {
    Full(Box<Artist>),
    Mapping(Box<ItemMapping>),
}

impl ArtistRef {
    pub fn name(&self) -> &str {
        match self {
            ArtistRef::Full(artist) => &artist.fields.name,
            ArtistRef::Mapping(mapping) => &mapping.name,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Track {
    #[serde(flatten)]
    pub fields: ItemFields,
    /// Whole seconds.
    pub duration: u32,
    pub artists: Vec<ItemMapping>,
    pub last_played: i64,
    pub album: Option<ItemMapping>,
    pub disc_number: u32,
    pub track_number: u32,
    pub audio_metadata: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Playlist {
    #[serde(flatten)]
    pub fields: ItemFields,
    pub owner: String,
    pub is_editable: bool,
    pub is_dynamic: bool,
    pub supported_mediatypes: Vec<MediaType>,
}

/// A chapter of an audiobook, as Music Assistant's `MediaItemChapter`, which gives its times in
/// seconds from the start of the book; kept here in milliseconds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Chapter {
    /// Its number, from 0, in the order chapters come.
    pub position: u32,
    pub name: String,
    #[serde(rename = "start", serialize_with = "seconds")]
    pub start_ms: u64,
    #[serde(rename = "end", serialize_with = "maybe_seconds")]
    pub end_ms: Option<u64>,
}

fn seconds<S: serde::Serializer>(ms: &u64, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_f64(*ms as f64 / 1_000.0)
}

fn maybe_seconds<S: serde::Serializer>(ms: &Option<u64>, serializer: S) -> Result<S::Ok, S::Error> {
    match ms {
        Some(ms) => seconds(ms, serializer),
        None => serializer.serialize_none(),
    }
}

/// The chapter playing at `ms` into a book with `chapters`, by index.
pub fn chapter_at(chapters: &[Chapter], ms: u64) -> Option<usize> {
    chapters.iter().rposition(|chapter| chapter.start_ms <= ms)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Audiobook {
    #[serde(flatten)]
    pub fields: ItemFields,
    pub publisher: Option<String>,
    /// Its authors, as artists of the source that serves the book, so each leads to their
    /// other books.
    pub authors: Vec<ItemMapping>,
    pub narrators: Vec<String>,
    /// Whole seconds.
    pub duration: u32,
    /// Finished, as its source says; `None` when it says nothing.
    pub fully_played: Option<bool>,
    /// Where listening left off, in milliseconds; `None` when it was never started.
    pub resume_position_ms: Option<u64>,
}

/// Any media item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum MediaItem {
    Artist(Box<Artist>),
    Album(Box<Album>),
    Track(Box<Track>),
    Playlist(Box<Playlist>),
    Audiobook(Box<Audiobook>),
}

impl MediaItem {
    pub fn fields(&self) -> &ItemFields {
        match self {
            MediaItem::Artist(item) => &item.fields,
            MediaItem::Album(item) => &item.fields,
            MediaItem::Track(item) => &item.fields,
            MediaItem::Playlist(item) => &item.fields,
            MediaItem::Audiobook(item) => &item.fields,
        }
    }
}

impl Audiobook {
    /// The book as a track, for what plays it to name and time it: its authors as its artists,
    /// no album.
    pub fn as_track(&self) -> Track {
        Track {
            fields: self.fields.clone(),
            duration: self.duration,
            artists: self.authors.clone(),
            last_played: 0,
            album: None,
            disc_number: 0,
            track_number: 0,
            audio_metadata: None,
        }
    }
}

/// How names sort: case-insensitively, ignoring a leading article.
pub fn sort_name(name: &str) -> String {
    let lower = name.trim().to_lowercase();
    for article in ["the ", "a ", "an "] {
        if let Some(rest) = lower.strip_prefix(article)
            && !rest.is_empty()
        {
            return rest.to_owned();
        }
    }
    lower
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uris() {
        assert_eq!(library_uri(MediaType::Album, 1), "library://album/1");
        assert_eq!(
            parse_library_uri("library://track/12"),
            Some((MediaType::Track, 12))
        );
        assert_eq!(parse_library_uri("spotify://track/12"), None);
        assert_eq!(parse_library_uri("library://radio/1"), None);
        assert_eq!(parse_library_uri("library://track/x"), None);
    }

    #[test]
    fn the_chapter_at_a_time() {
        let chapter = |position, start_ms| Chapter {
            position,
            name: format!("Chapter {position}"),
            start_ms,
            end_ms: None,
        };
        let chapters = [chapter(0, 0), chapter(1, 60_000), chapter(2, 125_500)];
        assert_eq!(chapter_at(&chapters, 0), Some(0));
        assert_eq!(chapter_at(&chapters, 59_999), Some(0));
        assert_eq!(chapter_at(&chapters, 60_000), Some(1));
        assert_eq!(chapter_at(&chapters, 4_000_000), Some(2));
        assert_eq!(chapter_at(&[], 10), None);
        // As Music Assistant has it: seconds.
        assert_eq!(
            serde_json::to_value(&chapters[2]).unwrap(),
            serde_json::json!({"position": 2, "name": "Chapter 2", "start": 125.5, "end": null})
        );
    }

    #[test]
    fn an_audiobook_has_music_assistants_fields() {
        let fields = ItemFields {
            item_id: "li_1".to_owned(),
            provider: "audiobookshelf--a".to_owned(),
            name: "The Hobbit".to_owned(),
            version: String::new(),
            sort_name: "hobbit".to_owned(),
            uri: "audiobookshelf--a://audiobook/li_1".to_owned(),
            external_ids: Vec::new(),
            is_playable: true,
            media_type: MediaType::Audiobook,
            provider_mappings: Vec::new(),
            metadata: Metadata::default(),
            favorite: false,
            position: None,
            date_added: None,
        };
        let author = ItemMapping {
            item_id: "aut_1".to_owned(),
            provider: "audiobookshelf--a".to_owned(),
            name: "J. R. R. Tolkien".to_owned(),
            version: String::new(),
            sort_name: "j. r. r. tolkien".to_owned(),
            uri: "audiobookshelf--a://artist/aut_1".to_owned(),
            external_ids: Vec::new(),
            is_playable: true,
            media_type: MediaType::Artist,
            available: true,
            image: None,
            year: None,
        };
        let book = Audiobook {
            fields,
            publisher: Some("HarperCollins".to_owned()),
            authors: vec![author],
            narrators: vec!["Andy Serkis".to_owned()],
            duration: 37_800,
            fully_played: Some(false),
            resume_position_ms: Some(120_000),
        };
        let json = serde_json::to_value(&book).unwrap();
        assert_eq!(json["media_type"], "audiobook");
        assert_eq!(json["authors"][0]["name"], "J. R. R. Tolkien");
        assert_eq!(json["narrators"], serde_json::json!(["Andy Serkis"]));
        assert_eq!(json["duration"], 37_800);
        assert_eq!(json["resume_position_ms"], 120_000);
        assert_eq!(json["fully_played"], false);
        assert_eq!(MediaType::parse("audiobook"), Some(MediaType::Audiobook));
        let track = book.as_track();
        assert_eq!(track.artists[0].name, "J. R. R. Tolkien");
        assert_eq!(track.duration, 37_800);
    }

    #[test]
    fn sort_names() {
        assert_eq!(sort_name("First Album"), "first album");
        assert_eq!(sort_name("The Beatles"), "beatles");
        assert_eq!(sort_name("A"), "a");
        assert_eq!(sort_name("Theory"), "theory");
    }
}
