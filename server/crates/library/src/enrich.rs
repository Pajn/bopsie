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
    let Some(year) = conn
        .query_row("SELECT year FROM albums WHERE id = ?1", [id], |row| {
            row.get::<_, Option<i32>>(0)
        })
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

/// A release album `id` looks like, for a person to accept or reject; unless they rejected it.
pub fn suggest_release(
    conn: &Connection,
    id: i64,
    release_id: &str,
    title: &str,
    distance: f64,
    now: i64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO release_suggestions (album_id, release_id, title, distance, created)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (album_id) DO UPDATE SET
             release_id = excluded.release_id, title = excluded.title,
             distance = excluded.distance, decision = NULL
         WHERE release_suggestions.decision IS NULL
             OR release_suggestions.release_id != excluded.release_id",
        params![id, release_id, title, distance, now],
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
    pub distance: f64,
    pub decision: Option<String>,
}

/// Releases suggested for albums, the undecided ones only unless `all`.
pub fn release_suggestions(conn: &Connection, all: bool) -> Result<Vec<ReleaseSuggestion>> {
    let mut statement = conn.prepare(
        "SELECT album_id, release_id, title, distance, decision FROM release_suggestions
         WHERE ?1 OR decision IS NULL ORDER BY created, album_id",
    )?;
    statement
        .query_map([all], |row| {
            Ok(ReleaseSuggestion {
                album_id: row.get(0)?,
                release_id: row.get(1)?,
                title: row.get(2)?,
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
