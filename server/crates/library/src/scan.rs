//! Writing scanned items into the library. Functions take a transaction and record what they
//! changed, so the scanner can publish events once its writes are committed.

use std::collections::BTreeSet;

use bopsie_core::media::{MediaType, sort_name};
use rusqlite::{Connection, OptionalExtension, Result, params};
use sha2::{Digest, Sha256};

use crate::ids::{IdKind, normalized, search_key};
use crate::provider::{Entry, ScannedPlaylist, ScannedTrack};
use crate::query::encode_format;

/// The provider an image or mapping belongs to.
#[derive(Debug, Clone)]
pub struct Source {
    pub domain: &'static str,
    pub instance: String,
    /// Its albums stay its own rather than merge with another provider's of the same name.
    pub separate_albums: bool,
    /// Which provider's copy comes first: see [`crate::provider::MusicProvider::priority`].
    pub priority: i64,
}

/// Items a scan added, changed or removed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changes {
    pub added: BTreeSet<(MediaType, i64)>,
    pub updated: BTreeSet<(MediaType, i64)>,
    pub deleted: BTreeSet<(MediaType, i64)>,
}

impl Changes {
    pub(crate) fn add(&mut self, item: (MediaType, i64)) {
        self.added.insert(item);
    }

    pub(crate) fn update(&mut self, item: (MediaType, i64)) {
        if !self.added.contains(&item) {
            self.updated.insert(item);
        }
    }

    fn delete(&mut self, item: (MediaType, i64)) {
        // Something added and removed in one scan never existed as far as clients know.
        if !self.added.remove(&item) {
            self.deleted.insert(item);
        }
        self.updated.remove(&item);
    }

    pub fn merge(&mut self, other: Changes) {
        for item in other.added {
            self.add(item);
        }
        for item in other.updated {
            self.update(item);
        }
        for item in other.deleted {
            self.delete(item);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.updated.is_empty() && self.deleted.is_empty()
    }
}

/// Where an image lives: in a provider, or in the library's artwork cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRef {
    pub provider: String,
    pub path: String,
}

/// The provider name images in the artwork cache are stored under.
pub const ARTWORK_CACHE: &str = "artwork";
/// Art from the Cover Art Archive, kept with [`ARTWORK_CACHE`]'s, for an album that had none:
/// art its own files bring replaces it.
pub const COVER_ART: &str = "coverartarchive";

/// Music Assistant's image proxy id: the SHA-256 of `provider/path`, in hex
/// (`create_thumb_hash` in its `helpers/images.py`).
pub fn proxy_id(provider: &str, path: &str) -> String {
    hex::encode(Sha256::digest(format!("{provider}/{path}")))
}

/// A name normalized for matching: case, surrounding and repeated whitespace do not count.
pub fn match_key(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Albums are the same album when their name and album artists match.
pub fn album_key(album_artists: &[String], album: &str) -> String {
    let artists: Vec<String> = album_artists.iter().map(|a| match_key(a)).collect();
    format!("{}\u{1e}{}", artists.join("\u{1f}"), match_key(album))
}

pub(crate) fn store_image(tx: &Connection, image: Option<&ImageRef>) -> Result<Option<String>> {
    let Some(image) = image else {
        return Ok(None);
    };
    let id = proxy_id(&image.provider, &image.path);
    tx.execute(
        "INSERT OR IGNORE INTO images (proxy_id, provider, path) VALUES (?1, ?2, ?3)",
        (&id, &image.provider, &image.path),
    )?;
    Ok(Some(id))
}

/// The table items of `media_type` live in.
pub(crate) fn table(media_type: MediaType) -> &'static str {
    match media_type {
        MediaType::Artist => "artists",
        MediaType::Album => "albums",
        MediaType::Track => "tracks",
        MediaType::Playlist => "playlists",
    }
}

/// Makes the item findable by `name`: in search, and by its search key.
pub(crate) fn index(tx: &Connection, media_type: MediaType, id: i64, name: &str) -> Result<()> {
    tx.execute(
        &format!(
            "UPDATE {} SET search_key = ?2 WHERE id = ?1",
            table(media_type)
        ),
        params![id, search_key(name)],
    )?;
    unindex(tx, media_type, id)?;
    for table in ["search_trigram", "search_words"] {
        tx.execute(
            &format!("INSERT INTO {table} (media_type, item_id, name) VALUES (?1, ?2, ?3)"),
            params![media_type.as_str(), id, name],
        )?;
    }
    Ok(())
}

fn unindex(tx: &Connection, media_type: MediaType, id: i64) -> Result<()> {
    for table in ["search_trigram", "search_words"] {
        tx.execute(
            &format!("DELETE FROM {table} WHERE media_type = ?1 AND item_id = ?2"),
            params![media_type.as_str(), id],
        )?;
    }
    Ok(())
}

fn map(
    tx: &Connection,
    source: &Source,
    media_type: MediaType,
    item_id: i64,
    provider_item_id: &str,
) -> Result<()> {
    tx.execute(
        "INSERT OR IGNORE INTO provider_mappings
             (media_type, item_id, provider_domain, provider_instance, provider_item_id, priority)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            media_type.as_str(),
            item_id,
            source.domain,
            source.instance,
            provider_item_id,
            source.priority
        ],
    )?;
    Ok(())
}

/// Gives search keys to items from before there were any.
pub fn backfill_search_keys(tx: &Connection) -> Result<()> {
    for media_type in [
        MediaType::Artist,
        MediaType::Album,
        MediaType::Track,
        MediaType::Playlist,
    ] {
        let table = table(media_type);
        let missing: Vec<(i64, String)> = tx
            .prepare(&format!(
                "SELECT id, name FROM {table} WHERE search_key IS NULL"
            ))?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for (id, name) in missing {
            tx.execute(
                &format!("UPDATE {table} SET search_key = ?2 WHERE id = ?1"),
                params![id, search_key(&name)],
            )?;
        }
    }
    Ok(())
}

/// Puts the provider's ids for an item in place of what it said before.
fn set_ids(
    tx: &Connection,
    source: &Source,
    media_type: MediaType,
    id: i64,
    ids: &[(IdKind, String)],
) -> Result<()> {
    tx.execute(
        "DELETE FROM external_ids
         WHERE media_type = ?1 AND item_id = ?2 AND provider_instance = ?3",
        params![media_type.as_str(), id, source.instance],
    )?;
    add_ids(tx, source, media_type, id, ids)
}

/// Adds to the ids the provider reports for an item.
fn add_ids(
    tx: &Connection,
    source: &Source,
    media_type: MediaType,
    id: i64,
    ids: &[(IdKind, String)],
) -> Result<()> {
    for (kind, value) in normalized(ids) {
        tx.execute(
            "INSERT OR IGNORE INTO external_ids
                 (media_type, item_id, provider_instance, kind, value)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                media_type.as_str(),
                id,
                source.instance,
                kind.as_str(),
                value
            ],
        )?;
    }
    Ok(())
}

/// The MusicBrainz id at `index` of `ids`, when there is one for each of `count` names.
fn id_at(ids: &[String], count: usize, index: usize) -> Option<&str> {
    (ids.len() == count).then(|| ids[index].as_str())
}

fn artist(
    tx: &Connection,
    source: &Source,
    name: &str,
    mbid: Option<&str>,
    now: i64,
    changes: &mut Changes,
) -> Result<i64> {
    let id = artist_row(tx, source, name, now, changes)?;
    if let Some(mbid) = mbid {
        add_ids(
            tx,
            source,
            MediaType::Artist,
            id,
            &[(IdKind::MbArtist, mbid.to_owned())],
        )?;
    }
    Ok(id)
}

fn artist_row(
    tx: &Connection,
    source: &Source,
    name: &str,
    now: i64,
    changes: &mut Changes,
) -> Result<i64> {
    let key = match_key(name);
    if let Some(id) = tx
        .query_row(
            "SELECT id FROM artists WHERE match_key = ?1",
            [&key],
            |row| row.get(0),
        )
        .optional()?
    {
        map(tx, source, MediaType::Artist, id, name.trim())?;
        return Ok(id);
    }
    let name = name.trim();
    tx.execute(
        "INSERT INTO artists (name, sort_name, match_key, date_added) VALUES (?1, ?2, ?3, ?4)",
        params![name, sort_name(name), key, now],
    )?;
    let id = tx.last_insert_rowid();
    map(tx, source, MediaType::Artist, id, name)?;
    index(tx, MediaType::Artist, id, name)?;
    changes.add((MediaType::Artist, id));
    Ok(id)
}

pub(crate) fn credit(
    tx: &Connection,
    table: &str,
    column: &str,
    id: i64,
    artists: &[i64],
) -> Result<()> {
    tx.execute(&format!("DELETE FROM {table} WHERE {column} = ?1"), [id])?;
    let mut seen = BTreeSet::new();
    let artists: Vec<i64> = artists
        .iter()
        .copied()
        .filter(|a| seen.insert(*a))
        .collect();
    for (position, artist_id) in artists.iter().enumerate() {
        tx.execute(
            &format!("INSERT INTO {table} ({column}, position, artist_id) VALUES (?1, ?2, ?3)"),
            params![id, position as i64, artist_id],
        )?;
    }
    Ok(())
}

/// A scanned track with its image resolved to where the library keeps it.
#[derive(Debug, Clone)]
pub struct PreparedTrack {
    pub track: ScannedTrack,
    pub image: Option<ImageRef>,
}

/// Names with blanks dropped, or `fallback` when none remain.
/// Names as given, each once: a file may carry its album artist under two keys.
fn names(names: &[String], fallback: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let names: Vec<String> = names
        .iter()
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty() && seen.insert(match_key(name)))
        .collect();
    if names.is_empty() {
        vec![fallback.to_owned()]
    } else {
        names
    }
}

/// The artist tracks without one are credited to.
pub const UNKNOWN_ARTIST: &str = "Unknown Artist";

fn album(
    tx: &Connection,
    source: &Source,
    prepared: &PreparedTrack,
    track_artists: &[String],
    image: Option<&str>,
    now: i64,
    changes: &mut Changes,
) -> Result<Option<i64>> {
    let track = &prepared.track;
    let Some(name) = track
        .album
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
    else {
        return Ok(None);
    };
    let album_artists = if track.album_artists.iter().any(|a| !a.trim().is_empty()) {
        names(&track.album_artists, UNKNOWN_ARTIST)
    } else {
        track_artists.to_vec()
    };
    let key = album_key(&album_artists, name);
    let key = if source.separate_albums {
        format!("{}\u{1d}{key}", source.instance)
    } else {
        key
    };
    let provider_album = track.album_id.as_deref().unwrap_or(name);
    // A service's album id is the album, wherever it was merged to; a folder is not, since its
    // files may be retagged as another album.
    let mapped: Option<i64> = if source.separate_albums {
        tx.query_row(
            "SELECT item_id FROM provider_mappings
             WHERE provider_instance = ?1 AND media_type = 'album' AND provider_item_id = ?2",
            [&source.instance, provider_album],
            |row| row.get(0),
        )
        .optional()?
    } else {
        None
    };
    let existing: Option<(i64, Option<i32>, Option<String>)> = match mapped {
        Some(id) => tx
            .query_row(
                "SELECT id, year, image FROM albums WHERE id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?,
        None => tx
            .query_row(
                "SELECT id, year, image FROM albums WHERE match_key = ?1",
                [&key],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?,
    };
    let id = match existing {
        Some((id, year, current_image)) => {
            // The first track to know the year or have art fills it in.
            let fill_year = year.is_none() && track.year.is_some();
            let fallback = match &current_image {
                Some(current) => tx
                    .query_row(
                        "SELECT provider = ?2 FROM images WHERE proxy_id = ?1",
                        params![current, COVER_ART],
                        |row| row.get::<_, bool>(0),
                    )
                    .optional()?
                    .unwrap_or(false),
                None => true,
            };
            let fill_image = fallback && image.is_some() && current_image.as_deref() != image;
            if fill_year || fill_image {
                tx.execute(
                    "UPDATE albums SET year = coalesce(year, ?2),
                         image = CASE WHEN ?4 THEN ?3 ELSE image END
                     WHERE id = ?1",
                    params![id, track.year, image, fill_image],
                )?;
                changes.update((MediaType::Album, id));
            }
            id
        }
        None => {
            tx.execute(
                "INSERT INTO albums (name, sort_name, match_key, year, image, date_added)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![name, sort_name(name), key, track.year, image, now],
            )?;
            let id = tx.last_insert_rowid();
            let artist_ids = album_artists
                .iter()
                .enumerate()
                .map(|(i, artist_name)| {
                    let mbid = id_at(&track.album_artist_ids, album_artists.len(), i);
                    artist(tx, source, artist_name, mbid, now, changes)
                })
                .collect::<Result<Vec<_>>>()?;
            credit(tx, "album_artists", "album_id", id, &artist_ids)?;
            index(tx, MediaType::Album, id, name)?;
            changes.add((MediaType::Album, id));
            id
        }
    };
    map(tx, source, MediaType::Album, id, provider_album)?;
    // Each track says what its album is; one that says nothing leaves what the others said.
    if !track.album_ids.is_empty() {
        set_ids(tx, source, MediaType::Album, id, &track.album_ids)?;
    }
    Ok(Some(id))
}

/// Adds or updates the track the provider calls `provider_id`.
pub fn upsert_track(
    tx: &Connection,
    source: &Source,
    provider_id: &str,
    stamp: &str,
    prepared: &PreparedTrack,
    now: i64,
    changes: &mut Changes,
) -> Result<i64> {
    let track = &prepared.track;
    let title = match track.title.trim() {
        "" => provider_id,
        title => title,
    };
    let track_artists = names(&track.artists, UNKNOWN_ARTIST);
    let image = store_image(tx, prepared.image.as_ref())?;
    let existing: Option<i64> = tx
        .query_row(
            "SELECT item_id FROM provider_mappings
             WHERE provider_instance = ?1 AND media_type = 'track' AND provider_item_id = ?2",
            [&source.instance, provider_id],
            |row| row.get(0),
        )
        .optional()?;
    // A provider that comes after another holding the track only fills in what it lacks.
    let outranked = match existing {
        Some(id) => tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM provider_mappings
                 WHERE media_type = 'track' AND item_id = ?1 AND provider_instance != ?2
                     AND priority > ?3)",
            params![id, source.instance, source.priority],
            |row| row.get::<_, bool>(0),
        )?,
        None => false,
    };
    // Nor does it bring its own album and artists, which the track no longer has; only an
    // album, if it has none.
    let has_album = match existing {
        Some(id) if outranked => tx.query_row(
            "SELECT album_id IS NOT NULL FROM tracks WHERE id = ?1",
            [id],
            |row| row.get::<_, bool>(0),
        )?,
        _ => false,
    };
    let album_id = if has_album {
        None
    } else {
        album(
            tx,
            source,
            prepared,
            &track_artists,
            image.as_deref(),
            now,
            changes,
        )?
    };
    let artist_ids = if outranked {
        Vec::new()
    } else {
        track_artists
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let mbid = id_at(&track.artist_ids, track_artists.len(), i);
                artist(tx, source, name, mbid, now, changes)
            })
            .collect::<Result<Vec<_>>>()?
    };
    let id = match existing {
        Some(id) if outranked => {
            tx.execute(
                "UPDATE tracks SET
                     duration = CASE WHEN duration = 0 THEN ?2 ELSE duration END,
                     album_id = coalesce(album_id, ?3),
                     disc_number = coalesce(disc_number, ?4),
                     track_number = coalesce(track_number, ?5),
                     image = coalesce(image, ?6)
                 WHERE id = ?1",
                params![
                    id,
                    track.duration_secs,
                    album_id,
                    track.disc_number,
                    track.track_number,
                    image
                ],
            )?;
            id
        }
        Some(id) => {
            tx.execute(
                "UPDATE tracks SET name = ?2, sort_name = ?3, duration = ?4, album_id = ?5,
                     disc_number = ?6, track_number = ?7, image = ?8
                 WHERE id = ?1",
                params![
                    id,
                    title,
                    sort_name(title),
                    track.duration_secs,
                    album_id,
                    track.disc_number,
                    track.track_number,
                    image
                ],
            )?;
            changes.update((MediaType::Track, id));
            id
        }
        None => {
            tx.execute(
                "INSERT INTO tracks (name, sort_name, duration, album_id, disc_number,
                     track_number, image, date_added)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    title,
                    sort_name(title),
                    track.duration_secs,
                    album_id,
                    track.disc_number,
                    track.track_number,
                    image,
                    now
                ],
            )?;
            let id = tx.last_insert_rowid();
            map(tx, source, MediaType::Track, id, provider_id)?;
            changes.add((MediaType::Track, id));
            id
        }
    };
    tx.execute(
        "UPDATE provider_mappings SET stamp = ?3, audio_format = ?4
         WHERE provider_instance = ?1 AND media_type = 'track' AND provider_item_id = ?2",
        params![
            source.instance,
            provider_id,
            stamp,
            encode_format(&track.audio_format)
        ],
    )?;
    set_ids(tx, source, MediaType::Track, id, &track.ids)?;
    if !outranked {
        credit(tx, "track_artists", "track_id", id, &artist_ids)?;
        index(tx, MediaType::Track, id, title)?;
    }
    Ok(id)
}

/// Removes the provider's items, and the library items no other provider still holds.
pub fn remove(
    tx: &Connection,
    source: &Source,
    media_type: MediaType,
    provider_ids: &[String],
    changes: &mut Changes,
) -> Result<()> {
    for provider_id in provider_ids {
        let Some(id) = tx
            .query_row(
                "DELETE FROM provider_mappings
                 WHERE provider_instance = ?1 AND media_type = ?2 AND provider_item_id = ?3
                 RETURNING item_id",
                params![source.instance, media_type.as_str(), provider_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
        else {
            continue;
        };
        tx.execute(
            "DELETE FROM external_ids
             WHERE media_type = ?1 AND item_id = ?2 AND provider_instance = ?3",
            params![media_type.as_str(), id, source.instance],
        )?;
        let still_held: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM provider_mappings WHERE media_type = ?1 AND item_id = ?2)",
            params![media_type.as_str(), id],
            |row| row.get(0),
        )?;
        if !still_held {
            delete_item(tx, media_type, id, changes)?;
        }
    }
    Ok(())
}

pub(crate) fn delete_item(
    tx: &Connection,
    media_type: MediaType,
    id: i64,
    changes: &mut Changes,
) -> Result<()> {
    tx.execute(
        &format!("DELETE FROM {} WHERE id = ?1", table(media_type)),
        [id],
    )?;
    for linked in ["provider_mappings", "external_ids", "item_genres"] {
        tx.execute(
            &format!("DELETE FROM {linked} WHERE media_type = ?1 AND item_id = ?2"),
            params![media_type.as_str(), id],
        )?;
    }
    tx.execute(
        "DELETE FROM match_suggestions WHERE media_type = ?1 AND (a = ?2 OR b = ?2)",
        params![media_type.as_str(), id],
    )?;
    if media_type == MediaType::Album {
        tx.execute("DELETE FROM release_suggestions WHERE album_id = ?1", [id])?;
    }
    unindex(tx, media_type, id)?;
    crate::edit::forget(tx, media_type, id)?;
    changes.delete((media_type, id));
    Ok(())
}

/// An album key with each album artist once.
fn undoubled_key(key: &str) -> String {
    let (prefix, rest) = match key.split_once('\u{1d}') {
        Some((prefix, rest)) => (Some(prefix), rest),
        None => (None, key),
    };
    let Some((artists, album)) = rest.split_once('\u{1e}') else {
        return key.to_owned();
    };
    let mut seen = BTreeSet::new();
    let artists: Vec<&str> = artists
        .split('\u{1f}')
        .filter(|a| seen.insert(*a))
        .collect();
    let key = format!("{}\u{1e}{album}", artists.join("\u{1f}"));
    match prefix {
        Some(prefix) => format!("{prefix}\u{1d}{key}"),
        None => key,
    }
}

/// Credits one artist once on items scanned before names were: files that carry their album
/// artist under two keys credited it twice. An album's key loses the second name too, keeping
/// its id, so its files find it again; one whose key is then another album's merges into it.
pub fn repair_doubled_credits(tx: &Connection, changes: &mut Changes) -> Result<()> {
    for (media_type, table, column) in [
        (MediaType::Track, "track_artists", "track_id"),
        (MediaType::Album, "album_artists", "album_id"),
    ] {
        let mut statement = tx.prepare(&format!(
            "SELECT {column} FROM {table} GROUP BY {column} HAVING count(*) > count(DISTINCT artist_id)"
        ))?;
        let doubled = statement
            .query_map([], |row| row.get::<_, i64>(0))?
            .collect::<Result<Vec<_>>>()?;
        for id in doubled {
            let mut statement = tx.prepare(&format!(
                "SELECT artist_id FROM {table} WHERE {column} = ?1 ORDER BY position"
            ))?;
            let artists = statement
                .query_map([id], |row| row.get::<_, i64>(0))?
                .collect::<Result<Vec<_>>>()?;
            credit(tx, table, column, id, &artists)?;
            changes.update((media_type, id));
        }
    }
    let mut statement = tx.prepare("SELECT id, match_key FROM albums")?;
    let keys = statement
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>>>()?;
    for (id, key) in keys {
        let fixed = undoubled_key(&key);
        if fixed == key {
            continue;
        }
        let other: Option<i64> = tx
            .query_row(
                "SELECT id FROM albums WHERE match_key = ?1",
                [&fixed],
                |row| row.get(0),
            )
            .optional()?;
        match other {
            Some(other) => crate::reconcile::merge_albums(tx, id, other, changes)?,
            None => {
                tx.execute(
                    "UPDATE albums SET match_key = ?2 WHERE id = ?1",
                    params![id, fixed],
                )?;
            }
        }
    }
    Ok(())
}

/// Removes albums without tracks, then artists credited on nothing.
pub fn prune(tx: &Connection, changes: &mut Changes) -> Result<()> {
    let orphans = |sql: &str| -> Result<Vec<i64>> {
        let mut statement = tx.prepare(sql)?;
        statement.query_map([], |row| row.get(0))?.collect()
    };
    // A provider no longer holding any of an album's tracks no longer holds the album, though
    // others still do.
    let mut statement = tx.prepare(
        "DELETE FROM provider_mappings AS album
         WHERE media_type = 'album' AND NOT EXISTS (
             SELECT 1 FROM tracks JOIN provider_mappings AS track
                 ON track.media_type = 'track' AND track.item_id = tracks.id
             WHERE tracks.album_id = album.item_id
                 AND track.provider_instance = album.provider_instance)
         RETURNING item_id, provider_instance",
    )?;
    let dropped = statement
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>>>()?;
    for (id, instance) in dropped {
        tx.execute(
            "DELETE FROM external_ids
             WHERE media_type = 'album' AND item_id = ?1 AND provider_instance = ?2",
            params![id, instance],
        )?;
        changes.update((MediaType::Album, id));
    }
    for id in orphans(
        "SELECT id FROM albums WHERE id NOT IN (SELECT album_id FROM tracks WHERE album_id IS NOT NULL)",
    )? {
        delete_item(tx, MediaType::Album, id, changes)?;
    }
    for id in orphans(
        "SELECT id FROM artists
         WHERE id NOT IN (SELECT artist_id FROM track_artists)
           AND id NOT IN (SELECT artist_id FROM album_artists)",
    )? {
        delete_item(tx, MediaType::Artist, id, changes)?;
    }
    Ok(())
}

/// Adds or updates the playlist the provider listed as `entry`. Its tracks are resolved against
/// the same provider's tracks; ones it does not hold are left out.
pub fn upsert_playlist(
    tx: &Connection,
    source: &Source,
    entry: &Entry,
    playlist: &ScannedPlaylist,
    image: Option<&ImageRef>,
    now: i64,
    changes: &mut Changes,
) -> Result<i64> {
    let (provider_id, stamp) = (entry.id.as_str(), entry.stamp.as_str());
    let name = match playlist.name.trim() {
        "" => provider_id,
        name => name,
    };
    let image = store_image(tx, image)?;
    let existing: Option<i64> = tx
        .query_row(
            "SELECT item_id FROM provider_mappings
             WHERE provider_instance = ?1 AND media_type = 'playlist' AND provider_item_id = ?2",
            [&source.instance, provider_id],
            |row| row.get(0),
        )
        .optional()?;
    let id = match existing {
        Some(id) => {
            tx.execute(
                "UPDATE playlists SET name = ?2, sort_name = ?3, image = ?4 WHERE id = ?1",
                params![id, name, sort_name(name), image],
            )?;
            changes.update((MediaType::Playlist, id));
            id
        }
        None => {
            tx.execute(
                "INSERT INTO playlists (name, sort_name, image, date_added)
                 VALUES (?1, ?2, ?3, ?4)",
                params![name, sort_name(name), image, now],
            )?;
            let id = tx.last_insert_rowid();
            map(tx, source, MediaType::Playlist, id, provider_id)?;
            changes.add((MediaType::Playlist, id));
            id
        }
    };
    tx.execute(
        "UPDATE provider_mappings SET stamp = ?3
         WHERE provider_instance = ?1 AND media_type = 'playlist' AND provider_item_id = ?2",
        params![source.instance, provider_id, stamp],
    )?;
    tx.execute("DELETE FROM playlist_tracks WHERE playlist_id = ?1", [id])?;
    let mut position = 0i64;
    for track_id in &playlist.track_ids {
        let Some(track) = tx
            .query_row(
                "SELECT item_id FROM provider_mappings
                 WHERE provider_instance = ?1 AND media_type = 'track' AND provider_item_id = ?2",
                [&source.instance, track_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
        else {
            continue;
        };
        tx.execute(
            "INSERT INTO playlist_tracks (playlist_id, position, track_id) VALUES (?1, ?2, ?3)",
            params![id, position, track],
        )?;
        position += 1;
    }
    index(tx, MediaType::Playlist, id, name)?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys() {
        assert_eq!(match_key("  The   Beatles "), "the beatles");
        assert_eq!(
            album_key(&["A".to_owned(), "B".to_owned()], "X"),
            album_key(&["a".to_owned(), " b ".to_owned()], "x ")
        );
        assert_ne!(
            album_key(&["A".to_owned()], "Greatest Hits"),
            album_key(&["B".to_owned()], "Greatest Hits")
        );
    }

    #[test]
    fn proxy_ids_match_music_assistant() {
        // From Music Assistant 2.10.6 serving the same library.
        assert_eq!(
            proxy_id(
                "filesystem_local--v9jzS3CL",
                "Test Artist/First Album/cover.jpg"
            ),
            "88866263f2f93840615c85163507319403f4ff1d4207de3e7c3b16f7c3c163b8"
        );
    }

    #[test]
    fn changes_resolve_within_a_scan() {
        let mut changes = Changes::default();
        let a = (MediaType::Track, 1);
        let b = (MediaType::Track, 2);
        changes.add(a);
        changes.update(a);
        changes.update(b);
        changes.delete(a);
        changes.delete(b);
        assert!(changes.added.is_empty());
        assert!(changes.updated.is_empty());
        assert_eq!(changes.deleted, BTreeSet::from([b]));
    }
}
