//! The local filesystem `MusicProvider`: walks the library roots, reads tags with lofty and opens
//! files for decoding.
//!
//! Item ids are paths relative to the root with `/` separators, which is also how Music
//! Assistant's filesystem provider names them. Album art is a `cover`, `folder`, `front` or
//! `album` image beside the tracks, or else the art embedded in the file. Playlists are `.m3u` and
//! `.m3u8` files whose entries are paths relative to the playlist or absolute paths inside
//! the root.

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use bopsie_audio::loudness::{self, Loudness};
use bopsie_core::media::AudioFormat;
use bopsie_library::ids::IdKind;
use bopsie_library::provider::{
    Entry, EntryKind, Listing, MusicProvider, ProviderError, ScannedImage, ScannedPlaylist,
    ScannedTrack,
};
use lofty::config::ParseOptions;
use lofty::file::{FileType, TaggedFile};
use lofty::ogg::OpusFile;
use lofty::picture::{MimeType, PictureType};
use lofty::prelude::*;
use lofty::tag::{ItemKey, Tag};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

pub const DOMAIN: &str = "filesystem_local";

const AUDIO_EXTENSIONS: &[&str] = &[
    "aac", "aif", "aifc", "aiff", "alac", "ape", "flac", "m4a", "mp3", "mp4", "oga", "ogg", "opus",
    "wav", "wv",
];
const PLAYLIST_EXTENSIONS: &[&str] = &["m3u", "m3u8"];
const COVER_NAMES: &[&str] = &["cover", "folder", "front", "album"];
const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png"];

pub struct FilesystemProvider {
    root: PathBuf,
    instance: String,
}

impl FilesystemProvider {
    /// A provider for the folder at `root`. Its instance id derives from the path, so it stays
    /// the same across restarts as long as the folder does not move.
    pub fn new(root: PathBuf) -> Self {
        let digest = hex::encode(Sha256::digest(root.to_string_lossy().as_bytes()));
        Self {
            instance: format!("{DOMAIN}--{}", &digest[..8]),
            root,
        }
    }

    /// The file an id names. Ids that would leave the root name nothing.
    fn resolve(&self, id: &str) -> Result<PathBuf, ProviderError> {
        let relative = Path::new(id);
        if relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err(ProviderError::Unreadable(format!(
                "not a library path: {id}"
            )));
        }
        Ok(self.root.join(relative))
    }

    fn id_of(&self, path: &Path) -> Option<String> {
        let relative = path.strip_prefix(&self.root).ok()?;
        let parts: Option<Vec<&str>> = relative
            .components()
            .map(|c| match c {
                Component::Normal(part) => part.to_str(),
                _ => None,
            })
            .collect();
        Some(parts?.join("/"))
    }

    fn folder_art(&self, dir: &Path) -> Option<String> {
        let entries = fs::read_dir(dir).ok()?;
        let mut candidates: Vec<(usize, PathBuf)> = entries
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter_map(|path| {
                let stem = path.file_stem()?.to_str()?.to_lowercase();
                let extension = path.extension()?.to_str()?.to_lowercase();
                let rank = COVER_NAMES.iter().position(|name| *name == stem)?;
                IMAGE_EXTENSIONS
                    .contains(&extension.as_str())
                    .then_some((rank, path))
            })
            .collect();
        candidates.sort();
        self.id_of(&candidates.into_iter().next()?.1)
    }
}

fn extension(path: &Path) -> Option<String> {
    Some(path.extension()?.to_str()?.to_lowercase())
}

fn is_hidden(entry: &walkdir::DirEntry) -> bool {
    entry.depth() > 0
        && entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with('.'))
}

fn strings(tag: &Tag, key: ItemKey) -> Vec<String> {
    tag.get_strings(key)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The ids that identify a track, by the tag keys they are written under.
const TRACK_IDS: &[(ItemKey, IdKind)] = &[
    (ItemKey::Isrc, IdKind::Isrc),
    (ItemKey::MusicBrainzRecordingId, IdKind::MbRecording),
    (ItemKey::MusicBrainzTrackId, IdKind::MbTrack),
];

/// The ids that identify a track's album.
const ALBUM_IDS: &[(ItemKey, IdKind)] = &[
    (ItemKey::Barcode, IdKind::Barcode),
    (ItemKey::MusicBrainzReleaseId, IdKind::MbRelease),
    (ItemKey::MusicBrainzReleaseGroupId, IdKind::MbReleaseGroup),
];

/// The values of `key` in the first of `tags` that has any: a file can carry several tags,
/// and ids are often only in one of them.
fn all_strings(tags: &[Tag], key: ItemKey) -> Vec<String> {
    tags.iter()
        .map(|tag| strings(tag, key))
        .find(|values| !values.is_empty())
        .unwrap_or_default()
}

/// How loud the file is, as its ReplayGain tags say, else an Opus file's R128 tags. None without
/// a track gain.
fn loudness(file: &TaggedFile, path: &Path) -> Option<Loudness> {
    let first = |key| all_strings(file.tags(), key).into_iter().next();
    let peak = |key| first(key).and_then(|peak| loudness::parse_peak(&peak));
    if let Some(track_gain_db) =
        first(ItemKey::ReplayGainTrackGain).and_then(|gain| loudness::parse_gain(&gain))
    {
        return Some(Loudness {
            track_gain_db,
            track_peak: peak(ItemKey::ReplayGainTrackPeak).unwrap_or(0.0),
            album_gain_db: first(ItemKey::ReplayGainAlbumGain)
                .and_then(|gain| loudness::parse_gain(&gain)),
            album_peak: peak(ItemKey::ReplayGainAlbumPeak),
        });
    }
    if file.file_type() != FileType::Opus {
        return None;
    }
    // lofty keeps R128's comments only in the Opus file's own Vorbis comments.
    let opus = OpusFile::read_from(&mut fs::File::open(path).ok()?, ParseOptions::new()).ok()?;
    let comments = opus.vorbis_comments();
    Some(Loudness {
        track_gain_db: loudness::parse_r128(comments.get("R128_TRACK_GAIN")?)?,
        track_peak: 0.0,
        album_gain_db: comments
            .get("R128_ALBUM_GAIN")
            .and_then(loudness::parse_r128),
        album_peak: None,
    })
}

fn ids(tags: &[Tag], keys: &[(ItemKey, IdKind)]) -> Vec<(IdKind, String)> {
    keys.iter()
        .flat_map(|(key, kind)| {
            all_strings(tags, *key)
                .into_iter()
                .map(move |value| (*kind, value))
        })
        .collect()
}

/// The year from a full date tag or a bare year.
fn year(tag: &Tag) -> Option<i32> {
    if let Some(date) = tag.date() {
        return Some(i32::from(date.year));
    }
    let text = tag
        .get_string(ItemKey::Year)
        .or_else(|| tag.get_string(ItemKey::RecordingDate))?;
    text.get(..4)?.parse().ok()
}

fn content_type(file_type: FileType, path: &Path) -> String {
    match file_type {
        FileType::Flac => "flac".to_owned(),
        FileType::Mpeg => "mp3".to_owned(),
        FileType::Wav => "wav".to_owned(),
        FileType::Aiff => "aiff".to_owned(),
        FileType::Vorbis => "ogg".to_owned(),
        FileType::Opus => "opus".to_owned(),
        _ => extension(path).unwrap_or_else(|| "?".to_owned()),
    }
}

fn embedded_art(tag: &Tag) -> Option<ScannedImage> {
    let pictures = tag.pictures();
    let picture = pictures
        .iter()
        .find(|p| p.pic_type() == PictureType::CoverFront)
        .or_else(|| pictures.first())?;
    let extension = match picture.mime_type() {
        Some(MimeType::Png) => "png",
        _ => "jpg",
    };
    Some(ScannedImage::Embedded {
        data: picture.data().to_vec(),
        extension: extension.to_owned(),
    })
}

impl MusicProvider for FilesystemProvider {
    fn domain(&self) -> &'static str {
        DOMAIN
    }

    fn instance_id(&self) -> &str {
        &self.instance
    }

    fn name(&self) -> &str {
        "Local files"
    }

    fn list(&self) -> Result<Listing, ProviderError> {
        if !self.root.is_dir() {
            return Err(ProviderError::Unreadable(format!(
                "{} is not a folder",
                self.root.display()
            )));
        }
        let mut entries = Vec::new();
        let mut unreadable = Vec::new();
        for entry in WalkDir::new(&self.root)
            .follow_links(true)
            .into_iter()
            .filter_entry(|e| !is_hidden(e))
        {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    tracing::warn!(%error, "skipping an unreadable path");
                    if let Some(id) = error.path().and_then(|path| self.id_of(path)) {
                        unreadable.push(id);
                    }
                    continue;
                }
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let Some(extension) = extension(entry.path()) else {
                continue;
            };
            let kind = if AUDIO_EXTENSIONS.contains(&extension.as_str()) {
                EntryKind::Track
            } else if PLAYLIST_EXTENSIONS.contains(&extension.as_str()) {
                EntryKind::Playlist
            } else {
                continue;
            };
            let Some(id) = self.id_of(entry.path()) else {
                tracing::warn!(path = %entry.path().display(), "skipping a path that is not UTF-8");
                continue;
            };
            let metadata = entry.metadata().map_err(std::io::Error::other)?;
            let modified = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |since| since.as_nanos());
            entries.push(Entry {
                id,
                kind,
                stamp: format!("{}:{modified}", metadata.len()),
            });
        }
        Ok(Listing {
            entries,
            unreadable,
        })
    }

    fn priority(&self) -> i64 {
        2
    }

    fn read_track(&self, id: &str) -> Result<ScannedTrack, ProviderError> {
        let path = self.resolve(id)?;
        let file = lofty::read_from_path(&path)
            .map_err(|error| ProviderError::Unreadable(error.to_string()))?;
        let properties = file.properties();
        let empty = Tag::new(file.primary_tag_type());
        let tag = file
            .primary_tag()
            .or_else(|| file.first_tag())
            .unwrap_or(&empty);
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let content_type = content_type(file.file_type(), &path);
        let dir = path.parent().unwrap_or(&self.root);
        let image = self
            .folder_art(dir)
            .map(ScannedImage::Path)
            .or_else(|| embedded_art(tag));
        Ok(ScannedTrack {
            title: tag.title().map_or(stem, |title| title.into_owned()),
            artists: strings(tag, ItemKey::TrackArtist),
            album: tag.album().map(|album| album.into_owned()),
            album_artists: strings(tag, ItemKey::AlbumArtist),
            year: year(tag),
            track_number: tag.track(),
            disc_number: tag.disk(),
            duration_secs: properties.duration().as_secs_f64().round() as u32,
            audio_format: AudioFormat {
                codec_type: content_type.clone(),
                output_format_str: content_type.clone(),
                content_type,
                sample_rate: properties.sample_rate().unwrap_or(44_100),
                bit_depth: properties.bit_depth().map_or(16, u32::from),
                channels: properties.channels().map_or(2, u32::from),
                bit_rate: properties.audio_bitrate().unwrap_or(0),
            },
            image,
            album_id: path.parent().and_then(|dir| self.id_of(dir)),
            ids: ids(file.tags(), TRACK_IDS),
            album_ids: ids(file.tags(), ALBUM_IDS),
            artist_ids: all_strings(file.tags(), ItemKey::MusicBrainzArtistId),
            album_artist_ids: all_strings(file.tags(), ItemKey::MusicBrainzReleaseArtistId),
            artist_spotify_ids: Vec::new(),
            album_artist_spotify_ids: Vec::new(),
            loudness: loudness(&file, &path),
        })
    }

    fn read_playlist(&self, id: &str) -> Result<ScannedPlaylist, ProviderError> {
        let path = self.resolve(id)?;
        let bytes = fs::read(&path)?;
        let text = String::from_utf8_lossy(&bytes);
        let dir = path.parent().unwrap_or(&self.root);
        let track_ids = text
            .lines()
            .map(|line| line.trim().trim_start_matches('\u{feff}'))
            .filter(|line| !line.is_empty() && !line.starts_with('#') && !line.contains("://"))
            .filter_map(|line| {
                let entry = Path::new(line);
                let full = if entry.is_absolute() {
                    entry.to_path_buf()
                } else {
                    dir.join(entry)
                };
                self.id_of(&normalize(&full))
            })
            .collect();
        Ok(ScannedPlaylist {
            image: None,
            name: path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            track_ids,
        })
    }

    fn image(&self, path: &str) -> Result<Vec<u8>, ProviderError> {
        Ok(fs::read(self.resolve(path)?)?)
    }

    fn file(&self, id: &str) -> Option<PathBuf> {
        self.resolve(id).ok().filter(|path| path.is_file())
    }
}

/// Resolves `.` and `..` without touching the filesystem.
fn normalize(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                result.pop();
            }
            Component::CurDir => {}
            other => result.push(other),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library() -> (tempfile::TempDir, FilesystemProvider) {
        let dir = tempfile::tempdir().unwrap();
        bopsie_fixtures::write_library(dir.path(), 1).unwrap();
        let provider = FilesystemProvider::new(dir.path().to_owned());
        (dir, provider)
    }

    #[test]
    fn lists_tracks_and_playlists_by_relative_path() {
        let (_dir, provider) = library();
        let mut entries = provider.list().unwrap().entries;
        entries.sort_by(|a, b| a.id.cmp(&b.id));
        let listed: Vec<(&str, EntryKind)> =
            entries.iter().map(|e| (e.id.as_str(), e.kind)).collect();
        assert_eq!(
            listed,
            [
                ("Mix.m3u", EntryKind::Playlist),
                ("Other Artist/Second Album/01 - Alpha.wav", EntryKind::Track),
                ("Other Artist/Second Album/02 - Beta.wav", EntryKind::Track),
                ("Other Artist/Second Album/03 - Gamma.wav", EntryKind::Track),
                ("Test Artist/First Album/01 - Opening.wav", EntryKind::Track),
                ("Test Artist/First Album/02 - Middle.wav", EntryKind::Track),
                ("Test Artist/First Album/03 - Closing.wav", EntryKind::Track),
            ]
        );
    }

    #[test]
    fn reads_tags_format_and_folder_art() {
        let (_dir, provider) = library();
        let track = provider
            .read_track("Test Artist/First Album/01 - Opening.wav")
            .unwrap();
        assert_eq!(track.title, "Opening");
        assert_eq!(track.artists, ["Test Artist"]);
        assert_eq!(track.album.as_deref(), Some("First Album"));
        assert_eq!(track.album_artists, ["Test Artist"]);
        assert_eq!(track.year, Some(2021));
        assert_eq!(track.track_number, Some(1));
        assert_eq!(track.duration_secs, 1);
        assert_eq!(track.audio_format.content_type, "wav");
        assert_eq!(track.audio_format.sample_rate, 44_100);
        assert_eq!(track.audio_format.bit_depth, 16);
        assert_eq!(track.audio_format.channels, 2);
        assert_eq!(
            track.image,
            Some(ScannedImage::Path(
                "Test Artist/First Album/cover.jpg".to_owned()
            ))
        );
        assert_eq!(track.album_id.as_deref(), Some("Test Artist/First Album"));
        assert!(
            !provider
                .image("Test Artist/First Album/cover.jpg")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn reads_replaygain_tags() {
        let (dir, provider) = library();
        let id = "Test Artist/First Album/01 - Opening.wav";
        assert_eq!(provider.read_track(id).unwrap().loudness, None);
        let mut tag = Tag::new(lofty::tag::TagType::Id3v2);
        tag.set_title("Opening".to_owned());
        tag.insert_text(ItemKey::ReplayGainTrackGain, "-7.25 dB".to_owned());
        tag.insert_text(ItemKey::ReplayGainTrackPeak, "0.988525".to_owned());
        tag.insert_text(ItemKey::ReplayGainAlbumGain, "+1.50 dB".to_owned());
        tag.insert_text(ItemKey::ReplayGainAlbumPeak, "1.000000".to_owned());
        tag.save_to_path(dir.path().join(id), lofty::config::WriteOptions::default())
            .unwrap();
        assert_eq!(
            provider.read_track(id).unwrap().loudness,
            Some(Loudness {
                track_gain_db: -7.25,
                track_peak: 0.988525,
                album_gain_db: Some(1.5),
                album_peak: Some(1.0),
            })
        );
        // A track gain alone, with no peak.
        let mut tag = Tag::new(lofty::tag::TagType::Id3v2);
        tag.insert_text(ItemKey::ReplayGainTrackGain, "-3 dB".to_owned());
        tag.save_to_path(dir.path().join(id), lofty::config::WriteOptions::default())
            .unwrap();
        assert_eq!(
            provider.read_track(id).unwrap().loudness,
            Some(Loudness {
                track_gain_db: -3.0,
                track_peak: 0.0,
                album_gain_db: None,
                album_peak: None,
            })
        );
    }

    #[test]
    fn reads_playlists_relative_to_themselves() {
        let (dir, provider) = library();
        fs::create_dir(dir.path().join("lists")).unwrap();
        let absolute = dir.path().join("Other Artist/Second Album/01 - Alpha.wav");
        fs::write(
            dir.path().join("lists/Road.m3u8"),
            format!(
                "#EXTM3U\n#EXTINF:1,Opening\n../Test Artist/First Album/01 - Opening.wav\n\n{}\nhttp://radio.example/stream\n../../outside.wav\n",
                absolute.display()
            ),
        )
        .unwrap();
        let playlist = provider.read_playlist("lists/Road.m3u8").unwrap();
        assert_eq!(playlist.name, "Road");
        assert_eq!(
            playlist.track_ids,
            [
                "Test Artist/First Album/01 - Opening.wav",
                "Other Artist/Second Album/01 - Alpha.wav",
            ]
        );
    }

    #[test]
    fn ids_cannot_leave_the_root() {
        let (_dir, provider) = library();
        assert!(provider.image("../etc/passwd").is_err());
        assert!(provider.image("/etc/passwd").is_err());
        assert!(provider.read_track("Test Artist/../../x.wav").is_err());
    }

    #[test]
    fn instance_ids_are_stable_per_root() {
        let a = FilesystemProvider::new(PathBuf::from("/music"));
        let b = FilesystemProvider::new(PathBuf::from("/music"));
        let c = FilesystemProvider::new(PathBuf::from("/other"));
        assert_eq!(a.instance_id(), b.instance_id());
        assert_ne!(a.instance_id(), c.instance_id());
        assert!(a.instance_id().starts_with("filesystem_local--"));
    }
}
