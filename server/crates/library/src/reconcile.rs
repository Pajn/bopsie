//! One release held by several providers, such as an album as files and saved on Spotify, made
//! one album with each of them as a source. After each sync, albums of the same name from
//! different providers are compared by [`crate::matching`]: the same release merges, one that
//! may be the same, such as a deluxe edition, becomes a suggestion for a person to decide, and
//! anything else stays apart. A person's decision is kept, and outlasts later syncs.
//!
//! The album and tracks of the provider that comes first survive a merge, with the others'
//! mappings, ids, edits and playlist entries moved onto them; scans then only fill in what they
//! lack (see [`crate::scan::upsert_track`]).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use bopsie_core::media::MediaType;
use rusqlite::{Connection, OptionalExtension, Result, params};
use serde_json::{Map, Value};

use crate::ids::IdKind;
use crate::matching::{
    AlbumFacts, Evidence, TrackFacts, compare_album, compare_track, credited_artists, pair_tracks,
    versioned,
};
use crate::scan::{Changes, delete_item, index, table};

/// A person said these are the same.
pub const MERGE: &str = "merge";
/// A person said these are not the same.
pub const APART: &str = "apart";

/// Ids that tie two albums together whatever their names.
const LINKS: [IdKind; 3] = [IdKind::Spotify, IdKind::MbRelease, IdKind::MbReleaseGroup];

/// The providers holding each album, by album id.
fn album_instances(tx: &Connection) -> Result<HashMap<i64, BTreeSet<String>>> {
    let mut statement = tx.prepare(
        "SELECT item_id, provider_instance FROM provider_mappings WHERE media_type = 'album'",
    )?;
    let mut instances: HashMap<i64, BTreeSet<String>> = HashMap::new();
    for row in statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))? {
        let (id, instance) = row?;
        instances.entry(id).or_default().insert(instance);
    }
    Ok(instances)
}

fn instances_of(tx: &Connection, media_type: MediaType, id: i64) -> Result<BTreeSet<String>> {
    let mut statement = tx.prepare(
        "SELECT DISTINCT provider_instance FROM provider_mappings
         WHERE media_type = ?1 AND item_id = ?2",
    )?;
    statement
        .query_map(params![media_type.as_str(), id], |row| row.get(0))?
        .collect()
}

fn credited_names(tx: &Connection, credits: &str, column: &str, id: i64) -> Result<Vec<String>> {
    let mut statement = tx.prepare(&format!(
        "SELECT artists.name FROM {credits} JOIN artists ON artists.id = {credits}.artist_id
         WHERE {credits}.{column} = ?1 ORDER BY position"
    ))?;
    statement.query_map([id], |row| row.get(0))?.collect()
}

fn ids_of(tx: &Connection, media_type: MediaType, id: i64) -> Result<Vec<(IdKind, String)>> {
    let mut statement = tx.prepare(
        "SELECT DISTINCT kind, value FROM external_ids WHERE media_type = ?1 AND item_id = ?2",
    )?;
    let rows = statement.query_map(params![media_type.as_str(), id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut ids = Vec::new();
    for row in rows {
        let (kind, value) = row?;
        if let Some(kind) = IdKind::parse(&kind) {
            ids.push((kind, value));
        }
    }
    Ok(ids)
}

/// What a comparison knows of an album, and its tracks' ids in the same order.
pub(crate) fn album_facts(tx: &Connection, id: i64) -> Result<(AlbumFacts, Vec<i64>)> {
    let name: String = tx.query_row("SELECT name FROM albums WHERE id = ?1", [id], |row| {
        row.get(0)
    })?;
    let mut statement = tx.prepare(
        "SELECT id, name, duration, disc_number, track_number FROM tracks
         WHERE album_id = ?1 ORDER BY coalesce(disc_number, 1), track_number, id",
    )?;
    let rows = statement
        .query_map([id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u32>(2)?,
                row.get::<_, Option<u32>>(3)?,
                row.get::<_, Option<u32>>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>>>()?;
    let mut tracks = Vec::with_capacity(rows.len());
    let mut track_ids = Vec::with_capacity(rows.len());
    for (track_id, name, duration_secs, disc_number, track_number) in rows {
        tracks.push(TrackFacts {
            name,
            artists: credited_names(tx, "track_artists", "track_id", track_id)?,
            duration_secs,
            disc_number,
            track_number,
            ids: ids_of(tx, MediaType::Track, track_id)?,
        });
        track_ids.push(track_id);
    }
    let facts = AlbumFacts {
        name,
        artists: credited_names(tx, "album_artists", "album_id", id)?,
        ids: ids_of(tx, MediaType::Album, id)?,
        tracks,
    };
    Ok((facts, track_ids))
}

// The id pairs below name their index: without statistics, SQLite walks the primary key for
// both sides instead, which on a library of some thousands of tracks takes seconds, and holds
// up every other read and write meanwhile.

/// Pairs of albums, from providers apart, that may be the same: of the same name, but for its
/// version, and an album artist; or tied by an id.
fn candidates(tx: &Connection) -> Result<BTreeSet<(i64, i64)>> {
    let instances = album_instances(tx)?;
    let mut by_name: BTreeMap<String, Vec<(i64, BTreeSet<String>)>> = BTreeMap::new();
    {
        let mut statement = tx.prepare("SELECT id, name FROM albums")?;
        let albums = statement
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>>>()?;
        for (id, name) in albums {
            if !instances.contains_key(&id) {
                continue;
            }
            let artists = credited_artists(&credited_names(tx, "album_artists", "album_id", id)?);
            by_name
                .entry(versioned(&name).base)
                .or_default()
                .push((id, artists));
        }
    }
    let apart = |a: i64, b: i64| match (instances.get(&a), instances.get(&b)) {
        (Some(a), Some(b)) => a.is_disjoint(b),
        _ => false,
    };
    let mut pairs = BTreeSet::new();
    for albums in by_name.values() {
        for (i, (a, artists_a)) in albums.iter().enumerate() {
            for (b, artists_b) in &albums[i + 1..] {
                if !artists_a.is_disjoint(artists_b) && apart(*a, *b) {
                    pairs.insert((*a.min(b), *a.max(b)));
                }
            }
        }
    }
    let mut statement = tx.prepare(
        "SELECT DISTINCT x.item_id, y.item_id FROM external_ids x
         JOIN external_ids y INDEXED BY external_ids_value ON y.kind = x.kind AND y.value = x.value AND y.item_id > x.item_id
         WHERE x.media_type = 'album' AND y.media_type = 'album' AND x.kind IN (?1, ?2, ?3)",
    )?;
    let linked = statement.query_map(
        params![LINKS[0].as_str(), LINKS[1].as_str(), LINKS[2].as_str()],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
    )?;
    for pair in linked {
        let (a, b) = pair?;
        if apart(a, b) {
            pairs.insert((a, b));
        }
    }
    Ok(pairs)
}

/// What a person decided of two items, if they did.
fn decision(tx: &Connection, media_type: MediaType, a: i64, b: i64) -> Result<Option<String>> {
    Ok(tx
        .query_row(
            "SELECT decision FROM match_suggestions WHERE media_type = ?1 AND a = ?2 AND b = ?3",
            params![media_type.as_str(), a.min(b), a.max(b)],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten())
}

/// Records that two items may be the same, unless that is already known.
fn suggest(
    tx: &Connection,
    media_type: MediaType,
    a: i64,
    b: i64,
    reason: &str,
    now: i64,
) -> Result<()> {
    tx.execute(
        "INSERT INTO match_suggestions (media_type, a, b, reason, created)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (media_type, a, b) DO UPDATE SET reason = excluded.reason
             WHERE decision IS NULL",
        params![media_type.as_str(), a.min(b), a.max(b), reason, now],
    )?;
    Ok(())
}

/// Records what a person decided of two items.
pub fn decide(
    tx: &Connection,
    media_type: MediaType,
    a: i64,
    b: i64,
    decision: &str,
    reason: &str,
    now: i64,
) -> Result<()> {
    tx.execute(
        "INSERT INTO match_suggestions (media_type, a, b, reason, decision, created)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT (media_type, a, b) DO UPDATE SET decision = excluded.decision",
        params![
            media_type.as_str(),
            a.min(b),
            a.max(b),
            reason,
            decision,
            now
        ],
    )?;
    Ok(())
}

/// Why two albums may be the same, as a person reads it.
fn reason(a: &AlbumFacts, b: &AlbumFacts) -> String {
    let matched = pair_tracks(&a.tracks, &b.tracks)
        .into_iter()
        .filter(|(ia, ib)| compare_track(&a.tracks[*ia], &b.tracks[*ib], true) == Evidence::Match)
        .count();
    let more = a.tracks.len().max(b.tracks.len());
    let mut reason = format!("{matched} of {more} tracks are the same");
    if versioned(&a.name).edition != versioned(&b.name).edition {
        reason.push_str(", in another edition");
    }
    reason
}

/// The provider priority an item comes first by: its best source's.
fn priority(tx: &Connection, media_type: MediaType, id: i64) -> Result<i64> {
    tx.query_row(
        "SELECT coalesce(max(priority), 0) FROM provider_mappings
         WHERE media_type = ?1 AND item_id = ?2",
        params![media_type.as_str(), id],
        |row| row.get(0),
    )
}

/// Compares every pair of albums that may be the same, and merges, suggests, or leaves them.
pub fn reconcile(tx: &Connection, now: i64, changes: &mut Changes) -> Result<()> {
    let mut merged: HashMap<i64, i64> = HashMap::new();
    let survivor = |merged: &HashMap<i64, i64>, mut id: i64| {
        while let Some(into) = merged.get(&id) {
            id = *into;
        }
        id
    };
    for (a, b) in candidates(tx)? {
        let (a, b) = (survivor(&merged, a), survivor(&merged, b));
        if a == b
            || !instances_of(tx, MediaType::Album, a)?.is_disjoint(&instances_of(
                tx,
                MediaType::Album,
                b,
            )?)
        {
            continue;
        }
        let evidence = match decision(tx, MediaType::Album, a, b)?.as_deref() {
            Some(APART) => continue,
            Some(MERGE) => Evidence::Match,
            _ => {
                let (facts_a, _) = album_facts(tx, a)?;
                let (facts_b, _) = album_facts(tx, b)?;
                let evidence = compare_album(&facts_a, &facts_b);
                if evidence == Evidence::Insufficient {
                    suggest(tx, MediaType::Album, a, b, &reason(&facts_a, &facts_b), now)?;
                }
                evidence
            }
        };
        match evidence {
            Evidence::Match => {
                let (from, into) = merge_order(tx, a, b)?;
                merge_albums(tx, from, into, changes)?;
                merged.insert(from, into);
            }
            Evidence::NoMatch => {
                // What changed since it was suggested says otherwise.
                tx.execute(
                    "DELETE FROM match_suggestions
                     WHERE media_type = 'album' AND a = ?1 AND b = ?2 AND decision IS NULL",
                    params![a.min(b), a.max(b)],
                )?;
            }
            Evidence::Insufficient => {}
        }
    }
    reconcile_tracks(tx, changes)
}

/// What a comparison knows of track `id`, and its album.
fn track_facts(tx: &Connection, id: i64) -> Result<Option<(TrackFacts, Option<i64>)>> {
    let Some((name, duration_secs, disc_number, track_number, album)) = tx
        .query_row(
            "SELECT name, duration, disc_number, track_number, album_id FROM tracks WHERE id = ?1",
            [id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()?
    else {
        return Ok(None);
    };
    let facts = TrackFacts {
        name,
        artists: credited_names(tx, "track_artists", "track_id", id)?,
        duration_secs,
        disc_number,
        track_number,
        ids: ids_of(tx, MediaType::Track, id)?,
    };
    Ok(Some((facts, album)))
}

/// The same recording held by providers apart on different albums, as a liked song on its
/// single and the album it is on as files, becomes one track. Not on albums that may be
/// another edition, or that a person or Separate keeps apart: that is for them to say.
fn reconcile_tracks(tx: &Connection, changes: &mut Changes) -> Result<()> {
    let mut statement = tx.prepare(
        "SELECT DISTINCT x.item_id, y.item_id FROM external_ids x
         JOIN external_ids y INDEXED BY external_ids_value ON y.kind = x.kind AND y.value = x.value AND y.item_id > x.item_id
         WHERE x.media_type = 'track' AND y.media_type = 'track' AND x.kind IN (?1, ?2)
         UNION
         SELECT a.id, b.id FROM tracks a
         JOIN tracks b ON b.search_key = a.search_key AND b.id > a.id
             AND abs(b.duration - a.duration) <= 2",
    )?;
    let pairs = statement
        .query_map(
            params![IdKind::Isrc.as_str(), IdKind::MbRecording.as_str()],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )?
        .collect::<Result<Vec<_>>>()?;
    let mut merged: HashMap<i64, i64> = HashMap::new();
    for (a, b) in pairs {
        let survivor = |mut id: i64| {
            while let Some(into) = merged.get(&id) {
                id = *into;
            }
            id
        };
        let (a, b) = (survivor(a), survivor(b));
        if a == b {
            continue;
        }
        let (Some((facts_a, album_a)), Some((facts_b, album_b))) =
            (track_facts(tx, a)?, track_facts(tx, b)?)
        else {
            continue;
        };
        let (pa, pb) = (
            priority(tx, MediaType::Track, a)?,
            priority(tx, MediaType::Track, b)?,
        );
        let (from, into) = if pa > pb || (pa == pb && a < b) {
            (b, a)
        } else {
            (a, b)
        };
        let (from_instances, into_instances) = (
            instances_of(tx, MediaType::Track, from)?,
            instances_of(tx, MediaType::Track, into)?,
        );
        // Providers apart; or, on one album, a second copy a provider holds of a track: one
        // whose files come first, as a service holding an album twice under two ids does, or
        // at the same place, as a file kept twice, in the album's folder and a playlist's.
        let apart = from_instances.is_disjoint(&into_instances);
        let place = |t: &TrackFacts| (t.disc_number.unwrap_or(1).max(1), t.track_number);
        let same_place = facts_a.track_number.is_some() && place(&facts_a) == place(&facts_b);
        let second_copy = album_a.is_some()
            && album_a == album_b
            && (pa != pb || same_place)
            && from_instances.is_subset(&into_instances);
        if !apart && !second_copy {
            continue;
        }
        if let (Some(album_a), Some(album_b)) = (album_a, album_b)
            && album_a != album_b
        {
            let said: bool = tx.query_row(
                "SELECT EXISTS (SELECT 1 FROM match_suggestions
                     WHERE media_type = 'album' AND a = ?1 AND b = ?2
                         AND coalesce(decision, '') != ?3)",
                params![album_a.min(album_b), album_a.max(album_b), MERGE],
                |row| row.get(0),
            )?;
            if said {
                continue;
            }
        }
        if compare_track(&facts_a, &facts_b, album_a == album_b) != Evidence::Match {
            continue;
        }
        merge_tracks(tx, from, into, changes)?;
        merged.insert(from, into);
    }
    Ok(())
}

/// Which of two albums merges into which: into the one whose provider comes first, else the
/// older.
fn merge_order(tx: &Connection, a: i64, b: i64) -> Result<(i64, i64)> {
    let (pa, pb) = (
        priority(tx, MediaType::Album, a)?,
        priority(tx, MediaType::Album, b)?,
    );
    Ok(if pa > pb || (pa == pb && a < b) {
        (b, a)
    } else {
        (a, b)
    })
}

/// Merges album `from` into `into`: tracks that are the same become one, the rest move over.
pub fn merge_albums(tx: &Connection, from: i64, into: i64, changes: &mut Changes) -> Result<()> {
    let (from_facts, from_tracks) = album_facts(tx, from)?;
    let (into_facts, into_tracks) = album_facts(tx, into)?;
    let mut paired = BTreeSet::new();
    for (ii, fi) in pair_tracks(&into_facts.tracks, &from_facts.tracks) {
        if compare_track(&into_facts.tracks[ii], &from_facts.tracks[fi], true) == Evidence::Match {
            merge_tracks(tx, from_tracks[fi], into_tracks[ii], changes)?;
            paired.insert(fi);
        }
    }
    for (fi, track) in from_tracks.iter().enumerate() {
        if !paired.contains(&fi) {
            tx.execute(
                "UPDATE tracks SET album_id = ?2 WHERE id = ?1",
                params![track, into],
            )?;
            changes.update((MediaType::Track, *track));
        }
    }
    tx.execute(
        "UPDATE albums SET year = coalesce(year, (SELECT year FROM albums WHERE id = ?2))
         WHERE id = ?1",
        params![into, from],
    )?;
    merge_item(tx, MediaType::Album, from, into, changes)
}

/// Merges track `from` into `into`, playlists and all.
fn merge_tracks(tx: &Connection, from: i64, into: i64, changes: &mut Changes) -> Result<()> {
    let mut statement =
        tx.prepare("SELECT DISTINCT playlist_id FROM playlist_tracks WHERE track_id = ?1")?;
    let playlists = statement
        .query_map([from], |row| row.get::<_, i64>(0))?
        .collect::<Result<Vec<_>>>()?;
    for playlist in playlists {
        changes.update((MediaType::Playlist, playlist));
    }
    tx.execute(
        "UPDATE playlist_tracks SET track_id = ?2 WHERE track_id = ?1",
        params![from, into],
    )?;
    merge_item(tx, MediaType::Track, from, into, changes)
}

/// Moves what is kept of an item from `from` onto `into`, then deletes `from`: its sources,
/// ids, edits (`into`'s win), favorite and art.
fn merge_item(
    tx: &Connection,
    media_type: MediaType,
    from: i64,
    into: i64,
    changes: &mut Changes,
) -> Result<()> {
    for linked in ["provider_mappings", "external_ids", "item_genres"] {
        tx.execute(
            &format!(
                "UPDATE OR IGNORE {linked} SET item_id = ?3 WHERE media_type = ?1 AND item_id = ?2"
            ),
            params![media_type.as_str(), from, into],
        )?;
    }
    let table = table(media_type);
    tx.execute(
        &format!(
            "UPDATE {table} SET
                 favorite = max(favorite, (SELECT favorite FROM {table} WHERE id = ?2)),
                 image = coalesce(image, (SELECT image FROM {table} WHERE id = ?2))
             WHERE id = ?1"
        ),
        params![into, from],
    )?;
    carry_edits(tx, media_type, from, into)?;
    // What was merged into `from` before is `into` now too.
    tx.execute(
        "UPDATE merged_items SET into_id = ?3 WHERE media_type = ?1 AND into_id = ?2",
        params![media_type.as_str(), from, into],
    )?;
    tx.execute(
        "INSERT OR REPLACE INTO merged_items (media_type, from_id, into_id) VALUES (?1, ?2, ?3)",
        params![media_type.as_str(), from, into],
    )?;
    delete_item(tx, media_type, from, changes)?;
    changes.update((media_type, into));
    Ok(())
}

fn edits_of(tx: &Connection, media_type: MediaType, id: i64) -> Result<Option<String>> {
    tx.query_row(
        "SELECT edits FROM metadata_edits WHERE media_type = ?1 AND item_id = ?2",
        params![media_type.as_str(), id],
        |row| row.get(0),
    )
    .optional()
}

fn carry_edits(tx: &Connection, media_type: MediaType, from: i64, into: i64) -> Result<()> {
    let Some(from_edits) = edits_of(tx, media_type, from)? else {
        return Ok(());
    };
    match edits_of(tx, media_type, into)? {
        None => {
            tx.execute(
                "UPDATE metadata_edits SET item_id = ?3 WHERE media_type = ?1 AND item_id = ?2",
                params![media_type.as_str(), from, into],
            )?;
        }
        Some(into_edits) => {
            let parse = |edits: &str| match serde_json::from_str(edits) {
                Ok(Value::Object(edits)) => edits,
                _ => Map::new(),
            };
            let mut edits = parse(&from_edits);
            edits.extend(parse(&into_edits));
            tx.execute(
                "UPDATE metadata_edits SET edits = ?3 WHERE media_type = ?1 AND item_id = ?2",
                params![media_type.as_str(), into, Value::Object(edits).to_string()],
            )?;
        }
    }
    Ok(())
}

/// Takes provider `instance` back out of album `id`, with the tracks only it holds, into an
/// album of its own, and keeps the two apart from then on. For a provider whose albums are
/// found by name (`separates` false), it is the others that leave, so that its next scan finds
/// its album again. Tracks it shares with the album's other providers are split in two, and
/// read again at its next sync. Returns the new album, or `None` when there is nothing to
/// separate.
pub fn separate(
    tx: &Connection,
    id: i64,
    instance: &str,
    separates: bool,
    now: i64,
    changes: &mut Changes,
) -> Result<Option<i64>> {
    let instances = instances_of(tx, MediaType::Album, id)?;
    if !instances.contains(instance) || instances.len() < 2 {
        return Ok(None);
    }
    let leaving: BTreeSet<String> = if separates {
        BTreeSet::from([instance.to_owned()])
    } else {
        instances
            .iter()
            .filter(|i| *i != instance)
            .cloned()
            .collect()
    };
    let prefix = leaving.first().expect("another provider holds it");
    let (name, sort_name, key, year, image): (String, String, String, Option<i32>, Option<String>) =
        tx.query_row(
            "SELECT name, sort_name, match_key, year, image FROM albums WHERE id = ?1",
            [id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )?;
    let key = key
        .split_once('\u{1d}')
        .map_or(key.as_str(), |(_, key)| key);
    let mut new_key = format!("{prefix}\u{1d}{key}");
    let taken = |key: &str| -> Result<bool> {
        tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM albums WHERE match_key = ?1)",
            [key],
            |row| row.get(0),
        )
    };
    if taken(&new_key)? {
        // Its provider finds it by its own id for it, so the key need only be its own.
        new_key = format!("{prefix}\u{1d}{now}\u{1d}{key}");
    }
    tx.execute(
        "INSERT INTO albums (name, sort_name, match_key, year, image, date_added)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![name, sort_name, new_key, year, image, now],
    )?;
    let new = tx.last_insert_rowid();
    tx.execute(
        "INSERT INTO album_artists (album_id, position, artist_id)
         SELECT ?2, position, artist_id FROM album_artists WHERE album_id = ?1",
        params![id, new],
    )?;
    index(tx, MediaType::Album, new, &name)?;
    move_sources(tx, MediaType::Album, id, new, &leaving)?;
    changes.add((MediaType::Album, new));
    changes.update((MediaType::Album, id));

    let mut statement = tx.prepare("SELECT id FROM tracks WHERE album_id = ?1")?;
    let tracks = statement
        .query_map([id], |row| row.get::<_, i64>(0))?
        .collect::<Result<Vec<_>>>()?;
    for track in tracks {
        let held = instances_of(tx, MediaType::Track, track)?;
        if held.is_subset(&leaving) {
            tx.execute(
                "UPDATE tracks SET album_id = ?2 WHERE id = ?1",
                params![track, new],
            )?;
            changes.update((MediaType::Track, track));
        } else if !held.is_disjoint(&leaving) {
            tx.execute(
                "INSERT INTO tracks (name, sort_name, duration, album_id, disc_number,
                     track_number, image, date_added)
                 SELECT name, sort_name, duration, ?2, disc_number, track_number, image, ?3
                 FROM tracks WHERE id = ?1",
                params![track, new, now],
            )?;
            let copy = tx.last_insert_rowid();
            tx.execute(
                "INSERT INTO track_artists (track_id, position, artist_id)
                 SELECT ?2, position, artist_id FROM track_artists WHERE track_id = ?1",
                params![track, copy],
            )?;
            let name: String =
                tx.query_row("SELECT name FROM tracks WHERE id = ?1", [copy], |row| {
                    row.get(0)
                })?;
            index(tx, MediaType::Track, copy, &name)?;
            move_sources(tx, MediaType::Track, track, copy, &leaving)?;
            // Read again, for what its own provider says of it.
            tx.execute(
                "UPDATE provider_mappings SET stamp = NULL
                 WHERE media_type = 'track' AND item_id = ?1",
                [copy],
            )?;
            changes.add((MediaType::Track, copy));
            changes.update((MediaType::Track, track));
        }
    }
    decide(tx, MediaType::Album, id, new, APART, "Separated", now)?;
    Ok(Some(new))
}

/// Moves the mappings and ids `instances` hold for item `from` onto `into`.
fn move_sources(
    tx: &Connection,
    media_type: MediaType,
    from: i64,
    into: i64,
    instances: &BTreeSet<String>,
) -> Result<()> {
    for instance in instances {
        for linked in ["provider_mappings", "external_ids"] {
            tx.execute(
                &format!(
                    "UPDATE {linked} SET item_id = ?3
                     WHERE media_type = ?1 AND item_id = ?2 AND provider_instance = ?4"
                ),
                params![media_type.as_str(), from, into, instance],
            )?;
        }
    }
    Ok(())
}

/// A suggestion awaiting a person, or decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub id: i64,
    pub media_type: MediaType,
    pub a: i64,
    pub b: i64,
    pub reason: String,
    pub decision: Option<String>,
}

/// Suggestions, the undecided ones only unless `all`.
pub fn suggestions(conn: &Connection, all: bool) -> Result<Vec<Suggestion>> {
    let mut statement = conn.prepare(
        "SELECT id, media_type, a, b, reason, decision FROM match_suggestions
         WHERE ?1 OR decision IS NULL ORDER BY created, id",
    )?;
    let rows = statement.query_map([all], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
            row.get(5)?,
        ))
    })?;
    let mut suggestions = Vec::new();
    for row in rows {
        let (id, media_type, a, b, reason, decision) = row?;
        if let Some(media_type) = MediaType::parse(&media_type) {
            suggestions.push(Suggestion {
                id,
                media_type,
                a,
                b,
                reason,
                decision,
            });
        }
    }
    Ok(suggestions)
}
