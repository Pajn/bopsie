//! Metadata edited by hand: kept apart from what the scan finds, and written through into the
//! library's tables so lists, search and order follow it. A sync writes what it scanned, then
//! applies every edit again, so an edit outlasts a change to its file.
//!
//! Resetting a field brings back what the scan found. A scan rewrites tracks and playlists
//! whole, so theirs are scanned again; it never renames albums or artists, so what they were
//! before their first edit is kept with the edit.

use bopsie_core::Error;
use bopsie_core::media::{MediaType, sort_name};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Map, Value, json};

use crate::scan::{Changes, UNKNOWN_ARTIST, album_key, credit, index, match_key};

/// The fields each kind of item can be edited in.
pub fn fields(media_type: MediaType) -> &'static [&'static str] {
    match media_type {
        MediaType::Track => &[
            "name",
            "sort_name",
            "track_number",
            "disc_number",
            "album",
            "artists",
            "image",
        ],
        MediaType::Album => &["name", "sort_name", "year", "artists", "image"],
        MediaType::Artist => &["name", "sort_name"],
        MediaType::Playlist => &["name"],
    }
}

fn table(media_type: MediaType) -> &'static str {
    match media_type {
        MediaType::Artist => "artists",
        MediaType::Album => "albums",
        MediaType::Track => "tracks",
        MediaType::Playlist => "playlists",
    }
}

/// Why an edit did not happen: the database failed, or the edit was refused.
#[derive(Debug)]
pub enum Failure {
    Db(rusqlite::Error),
    Refused(Error),
}

impl From<rusqlite::Error> for Failure {
    fn from(error: rusqlite::Error) -> Self {
        Self::Db(error)
    }
}

type Result<T> = std::result::Result<T, Failure>;
type DbResult<T> = rusqlite::Result<T>;

fn invalid(details: impl Into<String>) -> Failure {
    Failure::Refused(Error::invalid_data(details))
}

/// What is kept for an item: its edited fields, resolved to what the tables hold, and for
/// albums and artists the scanned values of those fields.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Stored {
    pub edits: Map<String, Value>,
    pub scanned: Map<String, Value>,
}

pub fn stored(conn: &Connection, media_type: MediaType, id: i64) -> DbResult<Stored> {
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT edits, scanned FROM metadata_edits WHERE media_type = ?1 AND item_id = ?2",
            params![media_type.as_str(), id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let parse = |json: &str| match serde_json::from_str(json) {
        Ok(Value::Object(map)) => map,
        _ => Map::new(),
    };
    Ok(row.map_or_else(Stored::default, |(edits, scanned)| Stored {
        edits: parse(&edits),
        scanned: parse(&scanned),
    }))
}

fn store(conn: &Connection, media_type: MediaType, id: i64, stored: &Stored) -> DbResult<()> {
    if stored.edits.is_empty() {
        conn.execute(
            "DELETE FROM metadata_edits WHERE media_type = ?1 AND item_id = ?2",
            params![media_type.as_str(), id],
        )?;
    } else {
        conn.execute(
            "INSERT INTO metadata_edits (media_type, item_id, edits, scanned)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (media_type, item_id)
             DO UPDATE SET edits = excluded.edits, scanned = excluded.scanned",
            params![
                media_type.as_str(),
                id,
                Value::Object(stored.edits.clone()).to_string(),
                Value::Object(stored.scanned.clone()).to_string()
            ],
        )?;
    }
    Ok(())
}

/// Forgets the edits of an item that is gone.
pub fn forget(conn: &Connection, media_type: MediaType, id: i64) -> DbResult<()> {
    conn.execute(
        "DELETE FROM metadata_edits WHERE media_type = ?1 AND item_id = ?2",
        params![media_type.as_str(), id],
    )?;
    Ok(())
}

fn exists(conn: &Connection, media_type: MediaType, id: i64) -> DbResult<bool> {
    conn.query_row(
        &format!(
            "SELECT EXISTS (SELECT 1 FROM {} WHERE id = ?1)",
            table(media_type)
        ),
        [id],
        |row| row.get(0),
    )
}

/// An artist by name: the one that name matches, else a new one.
fn artist_named(conn: &Connection, name: &str, now: i64, changes: &mut Changes) -> Result<i64> {
    let name = name.trim();
    if name.is_empty() {
        return Err(invalid("An artist needs a name"));
    }
    let key = match_key(name);
    if let Some(id) = conn
        .query_row(
            "SELECT id FROM artists WHERE match_key = ?1",
            [&key],
            |row| row.get(0),
        )
        .optional()?
    {
        return Ok(id);
    }
    conn.execute(
        "INSERT INTO artists (name, sort_name, match_key, date_added) VALUES (?1, ?2, ?3, ?4)",
        params![name, sort_name(name), key, now],
    )?;
    let id = conn.last_insert_rowid();
    index(conn, MediaType::Artist, id, name)?;
    changes.add((MediaType::Artist, id));
    Ok(id)
}

/// Artists given as `{"id": n}` or `{"name": "..."}`, to their ids.
fn artist_ids(
    conn: &Connection,
    value: &Value,
    now: i64,
    changes: &mut Changes,
) -> Result<Vec<i64>> {
    let Value::Array(entries) = value else {
        return Err(invalid("artists is a list"));
    };
    if entries.is_empty() {
        return Err(invalid("At least one artist"));
    }
    let mut ids = Vec::new();
    for entry in entries {
        let id = if let Some(id) = entry.get("id").and_then(Value::as_i64) {
            if !exists(conn, MediaType::Artist, id)? {
                return Err(invalid(format!("No artist {id}")));
            }
            id
        } else if let Some(name) = entry.get("name").and_then(Value::as_str) {
            artist_named(conn, name, now, changes)?
        } else {
            return Err(invalid("An artist is {\"id\": n} or {\"name\": \"...\"}"));
        };
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    Ok(ids)
}

/// An album given as `{"id": n}`, or `{"name": "...", "artists": [...]}`: the album with that
/// name and those album artists, else a new one.
fn album_id(
    conn: &Connection,
    value: &Value,
    track_artists: &[i64],
    now: i64,
    changes: &mut Changes,
) -> Result<i64> {
    if let Some(id) = value.get("id").and_then(Value::as_i64) {
        if !exists(conn, MediaType::Album, id)? {
            return Err(invalid(format!("No album {id}")));
        }
        return Ok(id);
    }
    let Some(name) = value.get("name").and_then(Value::as_str).map(str::trim) else {
        return Err(invalid("An album is {\"id\": n} or {\"name\": \"...\"}"));
    };
    if name.is_empty() {
        return Err(invalid("An album needs a name"));
    }
    let artists = match value.get("artists") {
        Some(artists) => artist_ids(conn, artists, now, changes)?,
        None => track_artists.to_vec(),
    };
    let names = artist_names(conn, &artists)?;
    let key = album_key(&names, name);
    if let Some(id) = conn
        .query_row(
            "SELECT id FROM albums WHERE match_key = ?1",
            [&key],
            |row| row.get(0),
        )
        .optional()?
    {
        return Ok(id);
    }
    conn.execute(
        "INSERT INTO albums (name, sort_name, match_key, date_added) VALUES (?1, ?2, ?3, ?4)",
        params![name, sort_name(name), key, now],
    )?;
    let id = conn.last_insert_rowid();
    credit(conn, "album_artists", "album_id", id, &artists)?;
    index(conn, MediaType::Album, id, name)?;
    changes.add((MediaType::Album, id));
    Ok(id)
}

fn artist_names(conn: &Connection, ids: &[i64]) -> DbResult<Vec<String>> {
    ids.iter()
        .map(|id| {
            Ok(conn
                .query_row("SELECT name FROM artists WHERE id = ?1", [id], |row| {
                    row.get(0)
                })
                .optional()?
                .unwrap_or_else(|| UNKNOWN_ARTIST.to_owned()))
        })
        .collect()
}

fn credited(conn: &Connection, table: &str, column: &str, id: i64) -> DbResult<Vec<i64>> {
    let mut statement = conn.prepare(&format!(
        "SELECT artist_id FROM {table} WHERE {column} = ?1 ORDER BY position"
    ))?;
    statement
        .query_map([id], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<i64>>>()
}

/// A field's value checked and resolved to what the tables hold.
fn resolve(
    conn: &Connection,
    media_type: MediaType,
    id: i64,
    field: &str,
    value: &Value,
    now: i64,
    changes: &mut Changes,
) -> Result<Value> {
    let text = |value: &Value| -> Result<String> {
        match value.as_str().map(str::trim) {
            Some(text) if !text.is_empty() => Ok(text.to_owned()),
            _ => Err(invalid(format!("{field} is a text that is not empty"))),
        }
    };
    let number = |value: &Value, max: i64| -> Result<Value> {
        match value.as_i64() {
            Some(n) if (0..=max).contains(&n) => Ok(json!(n)),
            _ => Err(invalid(format!(
                "{field} is a whole number from 0 to {max}"
            ))),
        }
    };
    Ok(match field {
        "name" => json!(text(value)?),
        "sort_name" => json!(text(value)?.to_lowercase()),
        "track_number" | "disc_number" => number(value, 9999)?,
        "year" => number(value, 9999)?,
        "artists" => json!(artist_ids(conn, value, now, changes)?),
        "album" => {
            let artists = credited(conn, "track_artists", "track_id", id)?;
            json!(album_id(conn, value, &artists, now, changes)?)
        }
        "image" => {
            let proxy_id = text(value)?;
            let known: bool = conn.query_row(
                "SELECT EXISTS (SELECT 1 FROM images WHERE proxy_id = ?1)",
                [&proxy_id],
                |row| row.get(0),
            )?;
            if !known {
                return Err(invalid(format!("No image {proxy_id}")));
            }
            json!(proxy_id)
        }
        other => {
            return Err(invalid(format!(
                "A {} has no field {other}; it has {}",
                media_type.as_str(),
                fields(media_type).join(", ")
            )));
        }
    })
}

/// What an album or artist holds in `field` now, before it is edited.
fn snapshot(conn: &Connection, media_type: MediaType, id: i64, field: &str) -> DbResult<Value> {
    Ok(match field {
        "artists" => json!(artist_names(
            conn,
            &credited(conn, "album_artists", "album_id", id)?
        )?),
        "name" | "sort_name" | "year" | "image" => conn
            .query_row(
                &format!("SELECT {field} FROM {} WHERE id = ?1", table(media_type)),
                [id],
                |row| row.get::<_, rusqlite::types::Value>(0),
            )
            .map(|value| match value {
                rusqlite::types::Value::Integer(n) => json!(n),
                rusqlite::types::Value::Text(text) => json!(text),
                _ => Value::Null,
            })?,
        _ => Value::Null,
    })
}

/// Writes `edits` into the item's row.
pub fn apply(
    conn: &Connection,
    media_type: MediaType,
    id: i64,
    edits: &Map<String, Value>,
) -> DbResult<()> {
    let table = table(media_type);
    for (field, value) in edits {
        match field.as_str() {
            "name" => {
                let name = value.as_str().unwrap_or_default();
                conn.execute(
                    &format!("UPDATE {table} SET name = ?2 WHERE id = ?1"),
                    params![id, name],
                )?;
                if !edits.contains_key("sort_name") {
                    conn.execute(
                        &format!("UPDATE {table} SET sort_name = ?2 WHERE id = ?1"),
                        params![id, sort_name(name)],
                    )?;
                }
                index(conn, media_type, id, name)?;
            }
            "sort_name" | "track_number" | "disc_number" | "year" | "image" => {
                let value = match value {
                    Value::Number(n) => rusqlite::types::Value::Integer(n.as_i64().unwrap_or(0)),
                    Value::String(s) => rusqlite::types::Value::Text(s.clone()),
                    _ => rusqlite::types::Value::Null,
                };
                conn.execute(
                    &format!("UPDATE {table} SET {field} = ?2 WHERE id = ?1"),
                    params![id, value],
                )?;
            }
            "album" => {
                conn.execute(
                    "UPDATE tracks SET album_id = ?2 WHERE id = ?1",
                    params![id, value.as_i64()],
                )?;
            }
            "artists" => {
                let ids: Vec<i64> = value
                    .as_array()
                    .map(|ids| ids.iter().filter_map(Value::as_i64).collect())
                    .unwrap_or_default();
                // Artists since gone are skipped; with none left the scanned ones stay.
                let ids: Vec<i64> = ids
                    .into_iter()
                    .filter(|id| exists(conn, MediaType::Artist, *id).unwrap_or(false))
                    .collect();
                if ids.is_empty() {
                    continue;
                }
                match media_type {
                    MediaType::Track => credit(conn, "track_artists", "track_id", id, &ids)?,
                    _ => credit(conn, "album_artists", "album_id", id, &ids)?,
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Every edit, written again over what a sync scanned.
pub fn apply_all(conn: &Connection) -> DbResult<()> {
    let mut statement = conn.prepare("SELECT media_type, item_id, edits FROM metadata_edits")?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<(String, i64, String)>>>()?;
    for (media_type, id, edits) in rows {
        let Some(media_type) = MediaType::parse(&media_type) else {
            continue;
        };
        if let Ok(Value::Object(edits)) = serde_json::from_str(&edits)
            && exists(conn, media_type, id)?
        {
            apply(conn, media_type, id, &edits)?;
        }
    }
    Ok(())
}

/// What an edit asks to be scanned again: the item's file, for tracks and playlists.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub rescan: bool,
}

/// Edits `id`: each field of `changes` is set, or with `null` reset to what was scanned.
pub fn edit(
    conn: &Connection,
    media_type: MediaType,
    id: i64,
    changes: &Map<String, Value>,
    now: i64,
    touched: &mut Changes,
) -> Result<Outcome> {
    if !exists(conn, media_type, id)? {
        return Err(Failure::Refused(Error::new(
            bopsie_core::ErrorCode::MediaNotFound,
            format!("No {} {id}", media_type.as_str()),
        )));
    }
    let mut stored = stored(conn, media_type, id)?;
    let old_album: Option<i64> = match media_type {
        MediaType::Track => {
            conn.query_row("SELECT album_id FROM tracks WHERE id = ?1", [id], |row| {
                row.get(0)
            })?
        }
        _ => None,
    };
    let rewritten_by_scan = matches!(media_type, MediaType::Track | MediaType::Playlist);
    let mut outcome = Outcome::default();
    let mut resets = Vec::new();
    for (field, value) in changes {
        if !fields(media_type).contains(&field.as_str()) {
            return Err(invalid(format!(
                "A {} has no field {field}; it has {}",
                media_type.as_str(),
                fields(media_type).join(", ")
            )));
        }
        if value.is_null() {
            if stored.edits.remove(field).is_some() {
                if rewritten_by_scan {
                    outcome.rescan = true;
                } else {
                    resets.push(field.clone());
                }
            }
            continue;
        }
        let resolved = resolve(conn, media_type, id, field, value, now, touched)?;
        if !rewritten_by_scan {
            // A name edit changes the sort name with it, so both are kept as scanned.
            let kept: &[&str] = if field == "name" {
                &["name", "sort_name"]
            } else {
                &[field.as_str()]
            };
            for kept in kept {
                if !stored.scanned.contains_key(*kept) {
                    stored
                        .scanned
                        .insert((*kept).to_owned(), snapshot(conn, media_type, id, kept)?);
                }
            }
        }
        stored.edits.insert(field.clone(), resolved);
    }
    // Albums and artists go back to what they were scanned as.
    for field in &resets {
        let mut restore = Map::new();
        let scanned = stored
            .scanned
            .get(field.as_str())
            .cloned()
            .unwrap_or(Value::Null);
        if field == "artists" {
            let names: Vec<String> = scanned
                .as_array()
                .map(|names| {
                    names
                        .iter()
                        .filter_map(|n| n.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            let ids = names
                .iter()
                .map(|name| artist_named(conn, name, now, touched))
                .collect::<Result<Vec<i64>>>()?;
            restore.insert(field.clone(), json!(ids));
        } else {
            restore.insert(field.clone(), scanned);
        }
        if field == "name"
            && !stored.edits.contains_key("sort_name")
            && let Some(sort) = stored.scanned.get("sort_name")
        {
            restore.insert("sort_name".to_owned(), sort.clone());
        }
        apply(conn, media_type, id, &restore)?;
    }
    // What is no longer edited needs no scanned value kept, but for the sort name of an
    // edited name.
    let edits = stored.edits.clone();
    stored.scanned.retain(|field, _| {
        edits.contains_key(field) || (field == "sort_name" && edits.contains_key("name"))
    });
    store(conn, media_type, id, &stored)?;
    apply(conn, media_type, id, &stored.edits)?;
    touched.update((media_type, id));
    if media_type == MediaType::Track {
        let new_album: Option<i64> =
            conn.query_row("SELECT album_id FROM tracks WHERE id = ?1", [id], |row| {
                row.get(0)
            })?;
        for album in [old_album, new_album].into_iter().flatten() {
            touched.update((MediaType::Album, album));
        }
    }
    Ok(outcome)
}
