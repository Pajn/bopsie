//! What a metadata service such as MusicBrainz says an album is: which albums are due to be
//! asked about, what is known of each to match it by, and keeping the ids it is identified by.
//! The ids are kept as the service's own, apart from what the album's providers report, and
//! feed [`crate::reconcile`]: a release linked to a Spotify album merges with it.

use bopsie_core::media::MediaType;
use rusqlite::{Connection, OptionalExtension, Result, params};

use crate::ids::{IdKind, normalized, search_key};
use crate::matching::AlbumFacts;
use crate::reconcile::album_facts;
use crate::scan::Changes;

/// The provider instance MusicBrainz's ids are kept under.
pub const MUSICBRAINZ: &str = "musicbrainz";
/// A person said the album is the suggested release.
pub const ACCEPT: &str = "accept";
/// A person said it is not.
pub const REJECT: &str = "reject";

/// What is known of an album to find it on a metadata service by.
#[derive(Debug, Clone)]
pub struct AlbumToMatch {
    pub id: i64,
    pub facts: AlbumFacts,
    /// Its tracks' library ids, in the order of `facts.tracks`.
    pub track_ids: Vec<i64>,
    pub year: Option<i32>,
    /// It has no artwork, or only what a metadata service gave it.
    pub needs_art: bool,
}

/// What a metadata service identified an album as.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Identified {
    /// The release, its release group, barcode, and links such as a Spotify album's.
    pub album: Vec<(IdKind, String)>,
    /// Album artists' ids, by the name the service credits them under.
    pub artists: Vec<(String, String)>,
    /// Ids for the album's tracks, by library id.
    pub tracks: Vec<(i64, Vec<(IdKind, String)>)>,
    /// Its genres, most voted first; none to leave what it has.
    pub genres: Vec<String>,
}

/// Albums not asked about since `before`, those with files first, at most `limit`.
pub fn due(conn: &Connection, before: i64, limit: usize) -> Result<Vec<i64>> {
    let mut statement = conn.prepare(
        "SELECT albums.id FROM albums
         WHERE coalesce(metadata_checked, 0) < ?1
         ORDER BY (SELECT max(priority) FROM provider_mappings
                   WHERE media_type = 'album' AND item_id = albums.id) DESC, albums.id
         LIMIT ?2",
    )?;
    statement
        .query_map(params![before, limit as i64], |row| row.get(0))?
        .collect()
}

/// What is known of album `id` to match it by, if it is still in the library.
pub fn album_to_match(conn: &Connection, id: i64) -> Result<Option<AlbumToMatch>> {
    let Some((year, needs_art)) = conn
        .query_row(
            "SELECT year, image IS NULL OR image IN (SELECT proxy_id FROM images WHERE provider = ?2)
             FROM albums WHERE id = ?1",
            params![id, crate::scan::COVER_ART],
            |row| Ok((row.get::<_, Option<i32>>(0)?, row.get::<_, bool>(1)?)),
        )
        .optional()?
    else {
        return Ok(None);
    };
    let (facts, track_ids) = album_facts(conn, id)?;
    Ok(Some(AlbumToMatch {
        id,
        facts,
        track_ids,
        year,
        needs_art,
    }))
}

/// Records that album `id` was asked about now, whatever was found.
pub fn checked(conn: &Connection, id: i64, now: i64) -> Result<()> {
    conn.execute(
        "UPDATE albums SET metadata_checked = ?2 WHERE id = ?1",
        params![id, now],
    )?;
    Ok(())
}

fn replace_ids(
    tx: &Connection,
    media_type: MediaType,
    id: i64,
    ids: &[(IdKind, String)],
) -> Result<()> {
    tx.execute(
        "DELETE FROM external_ids WHERE media_type = ?1 AND item_id = ?2 AND provider_instance = ?3",
        params![media_type.as_str(), id, MUSICBRAINZ],
    )?;
    for (kind, value) in normalized(ids) {
        tx.execute(
            "INSERT OR IGNORE INTO external_ids (media_type, item_id, provider_instance, kind, value)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![media_type.as_str(), id, MUSICBRAINZ, kind.as_str(), value],
        )?;
    }
    Ok(())
}

/// Keeps what album `id` was identified as, in place of what was known before.
pub fn identify(
    tx: &Connection,
    id: i64,
    identified: &Identified,
    now: i64,
    changes: &mut Changes,
) -> Result<()> {
    let exists: bool = tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM albums WHERE id = ?1)",
        [id],
        |row| row.get(0),
    )?;
    if !exists {
        return Ok(());
    }
    replace_ids(tx, MediaType::Album, id, &identified.album)?;
    if !identified.genres.is_empty() {
        tx.execute(
            "DELETE FROM item_genres
             WHERE media_type = 'album' AND item_id = ?1 AND provider_instance = ?2",
            params![id, MUSICBRAINZ],
        )?;
        for (position, genre) in identified.genres.iter().enumerate() {
            tx.execute(
                "INSERT INTO item_genres (media_type, item_id, provider_instance, position, genre)
                 VALUES ('album', ?1, ?2, ?3, ?4)",
                params![id, MUSICBRAINZ, position as i64, genre],
            )?;
        }
    }
    changes.update((MediaType::Album, id));
    for (track, ids) in &identified.tracks {
        let on_album: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM tracks WHERE id = ?1 AND album_id = ?2)",
            params![track, id],
            |row| row.get(0),
        )?;
        if on_album {
            replace_ids(tx, MediaType::Track, *track, ids)?;
            changes.update((MediaType::Track, *track));
        }
    }
    let mut statement = tx.prepare(
        "SELECT artists.id, artists.name FROM album_artists
         JOIN artists ON artists.id = album_artists.artist_id WHERE album_id = ?1",
    )?;
    let artists = statement
        .query_map([id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>>>()?;
    for (artist, name) in artists {
        let key = search_key(&name);
        if let Some((_, mbid)) = identified
            .artists
            .iter()
            .find(|(credited, _)| search_key(credited) == key)
        {
            replace_ids(
                tx,
                MediaType::Artist,
                artist,
                &[(IdKind::MbArtist, mbid.clone())],
            )?;
            changes.update((MediaType::Artist, artist));
        }
    }
    tx.execute(
        "DELETE FROM release_suggestions WHERE album_id = ?1 AND decision IS NULL",
        [id],
    )?;
    checked(tx, id, now)
}

/// What a release is, to compare with an album.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseDetails {
    /// Its credited artists, as it prints them.
    pub artist: String,
    pub title: String,
    pub year: Option<i32>,
    pub country: Option<String>,
    /// Its media's formats, such as "CD" or "Digital Media", each kind once.
    pub formats: Vec<String>,
    pub track_count: u32,
    pub disc_count: u32,
}

/// A release an album looks like but was not certain enough to take.
#[derive(Debug, Clone, PartialEq)]
pub struct SuggestedRelease {
    pub release_id: String,
    /// As a person reads it: "Artist – Title (2011, GB, CD)".
    pub description: String,
    pub details: ReleaseDetails,
    pub distance: f64,
}

/// A release album `id` looks like, for a person to accept or reject; unless they rejected it.
pub fn suggest_release(
    conn: &Connection,
    id: i64,
    release: &SuggestedRelease,
    now: i64,
) -> Result<()> {
    let details = &release.details;
    let formats = serde_json::to_string(&details.formats).unwrap_or_else(|_| "[]".to_owned());
    conn.execute(
        "INSERT INTO release_suggestions (album_id, release_id, title, distance, created,
             artist, release_title, year, country, formats, track_count, disc_count)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
         ON CONFLICT (album_id) DO UPDATE SET
             release_id = excluded.release_id, title = excluded.title,
             distance = excluded.distance, decision = NULL, artist = excluded.artist,
             release_title = excluded.release_title, year = excluded.year,
             country = excluded.country, formats = excluded.formats,
             track_count = excluded.track_count, disc_count = excluded.disc_count
         WHERE release_suggestions.decision IS NULL
             OR release_suggestions.release_id != excluded.release_id",
        params![
            id,
            release.release_id,
            release.description,
            release.distance,
            now,
            details.artist,
            details.title,
            details.year,
            details.country,
            formats,
            details.track_count,
            details.disc_count
        ],
    )?;
    Ok(())
}

/// The release suggested for album `id` that a person decided on, with their decision.
pub fn release_decision(conn: &Connection, id: i64) -> Result<Option<(String, String)>> {
    conn.query_row(
        "SELECT release_id, decision FROM release_suggestions
         WHERE album_id = ?1 AND decision IS NOT NULL",
        [id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
}

/// A release suggested for an album.
#[derive(Debug, Clone, PartialEq)]
pub struct ReleaseSuggestion {
    pub album_id: i64,
    pub release_id: String,
    pub title: String,
    /// None for a suggestion made before they were kept, until it is made again.
    pub details: Option<ReleaseDetails>,
    pub distance: f64,
    pub decision: Option<String>,
}

/// Releases suggested for albums, the undecided ones only unless `all`.
pub fn release_suggestions(conn: &Connection, all: bool) -> Result<Vec<ReleaseSuggestion>> {
    let mut statement = conn.prepare(
        "SELECT album_id, release_id, title, distance, decision, artist, release_title, year,
             country, formats, track_count, disc_count
         FROM release_suggestions
         WHERE ?1 OR decision IS NULL ORDER BY created, album_id",
    )?;
    statement
        .query_map([all], |row| {
            let artist: Option<String> = row.get(5)?;
            let details = match artist {
                Some(artist) => Some(ReleaseDetails {
                    artist,
                    title: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
                    year: row.get(7)?,
                    country: row.get(8)?,
                    formats: row
                        .get::<_, Option<String>>(9)?
                        .and_then(|formats| serde_json::from_str(&formats).ok())
                        .unwrap_or_default(),
                    track_count: row.get::<_, Option<u32>>(10)?.unwrap_or(0),
                    disc_count: row.get::<_, Option<u32>>(11)?.unwrap_or(0),
                }),
                None => None,
            };
            Ok(ReleaseSuggestion {
                album_id: row.get(0)?,
                release_id: row.get(1)?,
                title: row.get(2)?,
                details,
                distance: row.get(3)?,
                decision: row.get(4)?,
            })
        })?
        .collect()
}

/// Records a person's decision on the release suggested for album `id`: `accept`, which has
/// the album asked about again to take the release, or `reject`. Answers with the release, or
/// `None` when nothing was suggested.
pub fn decide_release(conn: &Connection, id: i64, decision: &str) -> Result<Option<String>> {
    let release = conn
        .query_row(
            "UPDATE release_suggestions SET decision = ?2 WHERE album_id = ?1 RETURNING release_id",
            params![id, decision],
            |row| row.get(0),
        )
        .optional()?;
    if release.is_some() && decision == ACCEPT {
        conn.execute(
            "UPDATE albums SET metadata_checked = NULL WHERE id = ?1",
            [id],
        )?;
    }
    Ok(release)
}

/// What is known of an artist to find its images by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtistToIllustrate {
    pub id: i64,
    pub name: String,
    /// Its Spotify ids, from Spotify's tracks or MusicBrainz's links.
    pub spotify: Vec<String>,
    /// Its MusicBrainz ids.
    pub musicbrainz: Vec<String>,
}

/// Artists whose images were not looked for since `before`, those credited on albums first,
/// at most `limit`.
pub fn artists_due(conn: &Connection, before: i64, limit: usize) -> Result<Vec<i64>> {
    let mut statement = conn.prepare(
        "SELECT id FROM artists WHERE coalesce(images_checked, 0) < ?1
         ORDER BY id NOT IN (SELECT artist_id FROM album_artists), id LIMIT ?2",
    )?;
    statement
        .query_map(params![before, limit as i64], |row| row.get(0))?
        .collect()
}

/// What is known of artist `id` to find its images by, if it is still in the library.
pub fn artist_to_illustrate(conn: &Connection, id: i64) -> Result<Option<ArtistToIllustrate>> {
    let Some(name) = conn
        .query_row("SELECT name FROM artists WHERE id = ?1", [id], |row| {
            row.get::<_, String>(0)
        })
        .optional()?
    else {
        return Ok(None);
    };
    let ids = |kind: IdKind| -> Result<Vec<String>> {
        let mut statement = conn.prepare(
            "SELECT DISTINCT value FROM external_ids
             WHERE media_type = 'artist' AND item_id = ?1 AND kind = ?2 ORDER BY value",
        )?;
        statement
            .query_map(params![id, kind.as_str()], |row| row.get(0))?
            .collect()
    };
    Ok(Some(ArtistToIllustrate {
        id,
        name,
        spotify: ids(IdKind::Spotify)?,
        musicbrainz: ids(IdKind::MbArtist)?,
    }))
}

/// Adds ids a metadata service links artist `id` to, beside those it has.
pub fn link_artist(conn: &Connection, id: i64, ids: &[(IdKind, String)]) -> Result<()> {
    for (kind, value) in normalized(ids) {
        conn.execute(
            "INSERT OR IGNORE INTO external_ids (media_type, item_id, provider_instance, kind, value)
             VALUES ('artist', ?1, ?2, ?3, ?4)",
            params![id, MUSICBRAINZ, kind.as_str(), value],
        )?;
    }
    Ok(())
}

/// Records that artist `id`'s images were looked for now.
pub fn artist_checked(conn: &Connection, id: i64, now: i64) -> Result<()> {
    conn.execute(
        "UPDATE artists SET images_checked = ?2 WHERE id = ?1",
        params![id, now],
    )?;
    Ok(())
}

/// Keeps `images`, by kind and stored image's proxy id, as what `source` has of item `id`, in
/// place of what it had.
pub fn set_item_images(
    conn: &Connection,
    media_type: MediaType,
    id: i64,
    source: &str,
    images: &[(String, String)],
) -> Result<()> {
    conn.execute(
        "DELETE FROM item_images WHERE media_type = ?1 AND item_id = ?2 AND provider_instance = ?3",
        params![media_type.as_str(), id, source],
    )?;
    let mut positions: std::collections::HashMap<&str, i64> = std::collections::HashMap::new();
    for (kind, proxy_id) in images {
        let position = positions.entry(kind.as_str()).or_default();
        conn.execute(
            "INSERT INTO item_images (media_type, item_id, provider_instance, kind, position, proxy_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![media_type.as_str(), id, source, kind, *position, proxy_id],
        )?;
        *position += 1;
    }
    Ok(())
}
