//! Reading the library into Music Assistant's models. Functions take a connection and run on
//! the database thread.

use bopsie_core::media::{
    Album, Artist, ArtistRef, AudioFormat, Image, ItemFields, ItemMapping, LIBRARY, MediaItem,
    MediaType, Metadata, Playlist, ProviderMapping, Track, library_uri,
};
use rusqlite::{Connection, OptionalExtension, Result, params};
use time::OffsetDateTime;
use time::macros::format_description;

use crate::search::fts_queries;

/// What a `library_items` or `count` command asks for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListQuery {
    pub favorite: Option<bool>,
    /// Only items whose name contains this, ignoring case.
    pub search: Option<String>,
    /// Only artists credited on an album.
    pub album_artists_only: bool,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub order_by: Option<String>,
}

/// Music Assistant's default page size.
pub const DEFAULT_LIMIT: u32 = 500;

fn table(media_type: MediaType) -> &'static str {
    match media_type {
        MediaType::Artist => "artists",
        MediaType::Album => "albums",
        MediaType::Track => "tracks",
        MediaType::Playlist => "playlists",
    }
}

/// The SQL ordering for Music Assistant's `order_by` keys, with the id as a tiebreak. Keys
/// that do not apply to the media type fall back to `sort_name`.
pub fn order_clause(media_type: MediaType, order_by: Option<&str>) -> String {
    let key = order_by.unwrap_or("sort_name");
    let (key, descending) = match key.strip_suffix("_desc") {
        Some(key) => (key, true),
        None => (key, false),
    };
    let column = match (key, media_type) {
        ("random", _) => return "RANDOM()".to_owned(),
        ("name", _) => "name COLLATE NOCASE",
        ("timestamp_added", _) => "date_added",
        ("year", MediaType::Album) => "year",
        ("duration", MediaType::Track) => "duration",
        _ => "sort_name",
    };
    let direction = if descending { "DESC" } else { "ASC" };
    format!("{column} {direction}, id {direction}")
}

fn filters(media_type: MediaType, query: &ListQuery) -> (String, Vec<rusqlite::types::Value>) {
    use rusqlite::types::Value;
    let mut clauses = Vec::new();
    let mut params = Vec::new();
    if let Some(favorite) = query.favorite {
        clauses.push("favorite = ?".to_owned());
        params.push(Value::Integer(favorite.into()));
    }
    if let Some(search) = query.search.as_deref().filter(|s| !s.trim().is_empty()) {
        clauses.push("instr(lower(name), lower(?)) > 0".to_owned());
        params.push(Value::Text(search.trim().to_owned()));
    }
    if media_type == MediaType::Artist && query.album_artists_only {
        clauses.push("id IN (SELECT artist_id FROM album_artists)".to_owned());
    }
    let clause = if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    };
    (clause, params)
}

pub fn list(conn: &Connection, media_type: MediaType, query: &ListQuery) -> Result<Vec<MediaItem>> {
    let (filter, mut params) = filters(media_type, query);
    params.push(query.limit.unwrap_or(DEFAULT_LIMIT).into());
    params.push(query.offset.unwrap_or(0).into());
    let sql = format!(
        "SELECT id FROM {} {filter} ORDER BY {} LIMIT ? OFFSET ?",
        table(media_type),
        order_clause(media_type, query.order_by.as_deref())
    );
    let ids = ids(conn, &sql, rusqlite::params_from_iter(params))?;
    items(conn, media_type, &ids)
}

pub fn count(conn: &Connection, media_type: MediaType, query: &ListQuery) -> Result<u32> {
    let (filter, params) = filters(media_type, query);
    conn.query_row(
        &format!("SELECT count(*) FROM {} {filter}", table(media_type)),
        rusqlite::params_from_iter(params),
        |row| row.get(0),
    )
}

fn ids(conn: &Connection, sql: &str, params: impl rusqlite::Params) -> Result<Vec<i64>> {
    let mut statement = conn.prepare_cached(sql)?;
    statement.query_map(params, |row| row.get(0))?.collect()
}

fn items(conn: &Connection, media_type: MediaType, ids: &[i64]) -> Result<Vec<MediaItem>> {
    ids.iter()
        .filter_map(|&id| item(conn, media_type, id).transpose())
        .collect()
}

pub fn item(conn: &Connection, media_type: MediaType, id: i64) -> Result<Option<MediaItem>> {
    Ok(match media_type {
        MediaType::Artist => artist(conn, id)?.map(|item| MediaItem::Artist(Box::new(item))),
        MediaType::Album => album_in_full(conn, id)?.map(|item| MediaItem::Album(Box::new(item))),
        MediaType::Track => track(conn, id)?.map(|item| MediaItem::Track(Box::new(item))),
        MediaType::Playlist => playlist(conn, id)?.map(|item| MediaItem::Playlist(Box::new(item))),
    })
}

fn date(seconds: i64) -> Option<String> {
    let format = format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]+00:00");
    OffsetDateTime::from_unix_timestamp(seconds)
        .ok()?
        .format(format)
        .ok()
}

pub fn image(conn: &Connection, proxy_id: Option<&str>) -> Result<Option<Image>> {
    let Some(proxy_id) = proxy_id else {
        return Ok(None);
    };
    conn.query_row(
        "SELECT provider, path FROM images WHERE proxy_id = ?1",
        [proxy_id],
        |row| {
            Ok(Image {
                kind: "thumb",
                provider: row.get(0)?,
                path: row.get(1)?,
                remotely_accessible: false,
                proxy_id: proxy_id.to_owned(),
            })
        },
    )
    .optional()
}

fn mappings(conn: &Connection, media_type: MediaType, id: i64) -> Result<Vec<ProviderMapping>> {
    let mut statement = conn.prepare_cached(
        "SELECT provider_domain, provider_instance, provider_item_id, stamp, audio_format
         FROM provider_mappings WHERE media_type = ?1 AND item_id = ?2
         ORDER BY priority DESC, provider_instance",
    )?;
    statement
        .query_map(params![media_type.as_str(), id], |row| {
            let item_id: String = row.get(2)?;
            let audio_format: Option<String> = row.get(4)?;
            let is_file = matches!(media_type, MediaType::Track | MediaType::Playlist);
            Ok(ProviderMapping {
                url: (!is_file).then(|| item_id.clone()),
                details: if is_file { row.get(3)? } else { None },
                item_id,
                provider_domain: row.get(0)?,
                provider_instance: row.get(1)?,
                available: true,
                in_library: true,
                is_unique: None,
                audio_format: audio_format
                    .and_then(|json| decode_format(&json))
                    .unwrap_or_else(AudioFormat::unknown),
            })
        })?
        .collect()
}

/// Audio formats are stored as `content_type,sample_rate,bit_depth,channels,bit_rate`.
pub fn encode_format(format: &AudioFormat) -> String {
    format!(
        "{},{},{},{},{}",
        format.content_type, format.sample_rate, format.bit_depth, format.channels, format.bit_rate
    )
}

fn decode_format(stored: &str) -> Option<AudioFormat> {
    let mut parts = stored.split(',');
    let content_type = parts.next()?.to_owned();
    let mut number = || parts.next()?.parse().ok();
    Some(AudioFormat {
        codec_type: content_type.clone(),
        output_format_str: content_type.clone(),
        content_type,
        sample_rate: number()?,
        bit_depth: number()?,
        channels: number()?,
        bit_rate: number()?,
    })
}

struct Common {
    name: String,
    sort_name: String,
    favorite: bool,
    date_added: i64,
}

fn fields(
    conn: &Connection,
    media_type: MediaType,
    id: i64,
    common: Common,
    image: Option<Image>,
) -> Result<ItemFields> {
    Ok(ItemFields {
        item_id: id.to_string(),
        provider: LIBRARY.to_owned(),
        name: common.name,
        version: String::new(),
        sort_name: common.sort_name,
        uri: library_uri(media_type, id),
        external_ids: external_ids(conn, media_type, id)?,
        is_playable: true,
        media_type,
        provider_mappings: mappings(conn, media_type, id)?,
        metadata: Metadata {
            images: image.map(|image| vec![image]),
            ..Metadata::default()
        },
        favorite: common.favorite,
        position: None,
        date_added: date(common.date_added),
    })
}

/// The item's external ids as Music Assistant lists them, `["isrc", "…"]`; links to a service,
/// which Music Assistant keeps as mappings, are left out.
fn external_ids(
    conn: &Connection,
    media_type: MediaType,
    id: i64,
) -> Result<Vec<serde_json::Value>> {
    let mut statement = conn.prepare_cached(
        "SELECT DISTINCT kind, value FROM external_ids
         WHERE media_type = ?1 AND item_id = ?2 AND kind != 'spotify'
         ORDER BY kind, value",
    )?;
    statement
        .query_map((media_type.as_str(), id), |row| {
            Ok(serde_json::json!([
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?
            ]))
        })?
        .collect()
}

fn mapping(
    media_type: MediaType,
    id: i64,
    name: String,
    sort_name: String,
    image: Option<Image>,
    year: Option<i32>,
) -> ItemMapping {
    ItemMapping {
        item_id: id.to_string(),
        provider: LIBRARY.to_owned(),
        name,
        version: String::new(),
        sort_name,
        uri: library_uri(media_type, id),
        external_ids: Vec::new(),
        is_playable: true,
        media_type,
        available: true,
        image,
        year,
    }
}

fn credited_artists(
    conn: &Connection,
    table: &str,
    column: &str,
    id: i64,
) -> Result<Vec<ItemMapping>> {
    let mut statement = conn.prepare_cached(&format!(
        "SELECT a.id, a.name, a.sort_name FROM {table} c JOIN artists a ON a.id = c.artist_id
         WHERE c.{column} = ?1 ORDER BY c.position"
    ))?;
    statement
        .query_map([id], |row| {
            Ok(mapping(
                MediaType::Artist,
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                None,
                None,
            ))
        })?
        .collect()
}

pub fn artist(conn: &Connection, id: i64) -> Result<Option<Artist>> {
    let Some(common) = conn
        .query_row(
            "SELECT name, sort_name, favorite, date_added FROM artists WHERE id = ?1",
            [id],
            |row| {
                Ok(Common {
                    name: row.get(0)?,
                    sort_name: row.get(1)?,
                    favorite: row.get(2)?,
                    date_added: row.get(3)?,
                })
            },
        )
        .optional()?
    else {
        return Ok(None);
    };
    Ok(Some(Artist {
        fields: fields(conn, MediaType::Artist, id, common, None)?,
        artist_type: "singer",
    }))
}

pub fn album(conn: &Connection, id: i64) -> Result<Option<Album>> {
    let Some((common, year, image_id)) = conn
        .query_row(
            "SELECT name, sort_name, favorite, date_added, year, image FROM albums WHERE id = ?1",
            [id],
            |row| {
                Ok((
                    Common {
                        name: row.get(0)?,
                        sort_name: row.get(1)?,
                        favorite: row.get(2)?,
                        date_added: row.get(3)?,
                    },
                    row.get::<_, Option<i32>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            },
        )
        .optional()?
    else {
        return Ok(None);
    };
    let image = image(conn, image_id.as_deref())?;
    Ok(Some(Album {
        fields: fields(conn, MediaType::Album, id, common, image)?,
        year,
        artists: credited_artists(conn, "album_artists", "album_id", id)?
            .into_iter()
            .map(|mapping| ArtistRef::Mapping(Box::new(mapping)))
            .collect(),
        album_type: "unknown",
    }))
}

/// An album with its artists in full, as an album fetched on its own is.
pub fn album_in_full(conn: &Connection, id: i64) -> Result<Option<Album>> {
    let Some(mut album) = album(conn, id)? else {
        return Ok(None);
    };
    let mut artists = Vec::with_capacity(album.artists.len());
    for credited in album.artists {
        let ArtistRef::Mapping(mapping) = &credited else {
            artists.push(credited);
            continue;
        };
        match mapping
            .item_id
            .parse()
            .ok()
            .map(|id| artist(conn, id))
            .transpose()?
        {
            Some(Some(full)) => artists.push(ArtistRef::Full(Box::new(full))),
            _ => artists.push(credited),
        }
    }
    album.artists = artists;
    Ok(Some(album))
}

fn album_mapping(conn: &Connection, id: i64) -> Result<Option<ItemMapping>> {
    let Some((name, sort_name, year, image_id)) = conn
        .query_row(
            "SELECT name, sort_name, year, image FROM albums WHERE id = ?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<i32>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()?
    else {
        return Ok(None);
    };
    let image = image(conn, image_id.as_deref())?;
    Ok(Some(mapping(
        MediaType::Album,
        id,
        name,
        sort_name,
        image,
        year,
    )))
}

pub fn track(conn: &Connection, id: i64) -> Result<Option<Track>> {
    let Some((common, duration, album_id, disc, number, image_id)) = conn
        .query_row(
            "SELECT name, sort_name, favorite, date_added, duration, album_id, disc_number,
                    track_number, image
             FROM tracks WHERE id = ?1",
            [id],
            |row| {
                Ok((
                    Common {
                        name: row.get(0)?,
                        sort_name: row.get(1)?,
                        favorite: row.get(2)?,
                        date_added: row.get(3)?,
                    },
                    row.get::<_, u32>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, Option<u32>>(6)?,
                    row.get::<_, Option<u32>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                ))
            },
        )
        .optional()?
    else {
        return Ok(None);
    };
    let image = image(conn, image_id.as_deref())?;
    let album = match album_id {
        Some(album_id) => album_mapping(conn, album_id)?,
        None => None,
    };
    Ok(Some(Track {
        fields: fields(conn, MediaType::Track, id, common, image)?,
        duration,
        artists: credited_artists(conn, "track_artists", "track_id", id)?,
        last_played: 0,
        album,
        disc_number: disc.unwrap_or(0),
        track_number: number.unwrap_or(0),
        audio_metadata: None,
    }))
}

pub fn playlist(conn: &Connection, id: i64) -> Result<Option<Playlist>> {
    let Some(common) = conn
        .query_row(
            "SELECT name, sort_name, favorite, date_added, image FROM playlists WHERE id = ?1",
            [id],
            |row| {
                Ok((
                    Common {
                        name: row.get(0)?,
                        sort_name: row.get(1)?,
                        favorite: row.get(2)?,
                        date_added: row.get(3)?,
                    },
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        )
        .optional()?
    else {
        return Ok(None);
    };
    let (common, image_id) = common;
    let image = image(conn, image_id.as_deref())?;
    Ok(Some(Playlist {
        fields: fields(conn, MediaType::Playlist, id, common, image)?,
        owner: String::new(),
        is_editable: false,
        is_dynamic: false,
        supported_mediatypes: vec![MediaType::Track],
    }))
}

fn tracks(conn: &Connection, ids: &[i64]) -> Result<Vec<Track>> {
    ids.iter()
        .filter_map(|&id| track(conn, id).transpose())
        .collect()
}

pub fn album_tracks(conn: &Connection, album_id: i64) -> Result<Vec<Track>> {
    let ids = ids(
        conn,
        "SELECT id FROM tracks WHERE album_id = ?1
         ORDER BY coalesce(disc_number, 0), coalesce(track_number, 0), sort_name, id",
        [album_id],
    )?;
    tracks(conn, &ids)
}

/// Albums the artist is credited on, or has tracks on, oldest first.
pub fn artist_albums(conn: &Connection, artist_id: i64) -> Result<Vec<Album>> {
    let ids = ids(
        conn,
        "SELECT id FROM albums
         WHERE id IN (SELECT album_id FROM album_artists WHERE artist_id = ?1)
            OR id IN (SELECT t.album_id FROM tracks t
                      JOIN track_artists ta ON ta.track_id = t.id WHERE ta.artist_id = ?1)
         ORDER BY year IS NULL, year, sort_name, id",
        [artist_id],
    )?;
    ids.iter()
        .filter_map(|&id| album(conn, id).transpose())
        .collect()
}

pub fn artist_tracks(conn: &Connection, artist_id: i64) -> Result<Vec<Track>> {
    let ids = ids(
        conn,
        "SELECT t.id FROM tracks t JOIN track_artists ta ON ta.track_id = t.id
         WHERE ta.artist_id = ?1 ORDER BY t.sort_name, t.id",
        [artist_id],
    )?;
    tracks(conn, &ids)
}

/// The playlist's tracks in order, each with its `position`.
pub fn playlist_tracks(conn: &Connection, playlist_id: i64) -> Result<Vec<Track>> {
    let mut statement = conn.prepare_cached(
        "SELECT track_id, position FROM playlist_tracks WHERE playlist_id = ?1 ORDER BY position",
    )?;
    let entries: Vec<(i64, u32)> = statement
        .query_map([playlist_id], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_>>()?;
    let mut result = Vec::with_capacity(entries.len());
    for (track_id, position) in entries {
        if let Some(mut track) = track(conn, track_id)? {
            track.fields.position = Some(position);
            result.push(track);
        }
    }
    Ok(result)
}

/// Items of `media_type` whose names match `query`, best first.
pub fn search(
    conn: &Connection,
    media_type: MediaType,
    query: &str,
    limit: u32,
) -> Result<Vec<MediaItem>> {
    let fts = fts_queries(query);
    let mut parts = Vec::new();
    let mut params: Vec<rusqlite::types::Value> = Vec::new();
    if let Some(trigram) = fts.trigram {
        parts.push("SELECT item_id, rank FROM search_trigram WHERE search_trigram MATCH ? AND media_type = ?");
        params.push(trigram.into());
        params.push(media_type.as_str().to_owned().into());
    }
    if let Some(words) = fts.words {
        parts.push(
            "SELECT item_id, rank FROM search_words WHERE search_words MATCH ? AND media_type = ?",
        );
        params.push(words.into());
        params.push(media_type.as_str().to_owned().into());
    }
    if parts.is_empty() {
        return Ok(Vec::new());
    }
    params.push(limit.into());
    let sql = format!(
        "SELECT item_id FROM ({}) GROUP BY item_id ORDER BY min(rank) LIMIT ?",
        parts.join(" UNION ALL ")
    );
    let ids = ids(conn, &sql, rusqlite::params_from_iter(params))?;
    items(conn, media_type, &ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orderings() {
        assert_eq!(
            order_clause(MediaType::Album, None),
            "sort_name ASC, id ASC"
        );
        assert_eq!(
            order_clause(MediaType::Album, Some("year_desc")),
            "year DESC, id DESC"
        );
        assert_eq!(
            order_clause(MediaType::Artist, Some("year")),
            "sort_name ASC, id ASC"
        );
        assert_eq!(
            order_clause(MediaType::Track, Some("timestamp_added_desc")),
            "date_added DESC, id DESC"
        );
        assert_eq!(order_clause(MediaType::Track, Some("random")), "RANDOM()");
        assert_eq!(
            order_clause(MediaType::Track, Some("; DROP TABLE tracks")),
            "sort_name ASC, id ASC"
        );
    }

    #[test]
    fn audio_formats_round_trip() {
        let format = AudioFormat {
            content_type: "flac".to_owned(),
            codec_type: "flac".to_owned(),
            sample_rate: 48000,
            bit_depth: 24,
            channels: 2,
            output_format_str: "flac".to_owned(),
            bit_rate: 1411,
        };
        assert_eq!(decode_format(&encode_format(&format)), Some(format));
    }
}
