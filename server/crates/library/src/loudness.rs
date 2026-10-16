//! Measuring how loud files are that do not say: what is due, and what was found.
//!
//! A file is due when what is known of its loudness is not for the file as it is now: its
//! `loudness_stamp` differs from its `stamp`. One album's files, by the provider's album such as
//! a folder, are measured together, for the album's gain; so a folder where any file is due is
//! measured whole, and every file in it takes what was measured, so that one album's gains come
//! from one method.

use std::collections::BTreeMap;

use bopsie_audio::loudness::Loudness;
use rusqlite::{Connection, params, params_from_iter};

/// One file to measure: the provider's id for it and the stamp it is measured at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DueFile {
    pub item: String,
    pub stamp: String,
}

/// The files of one album to measure together, of provider `instance`; a file with no album
/// is one alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DueAlbum {
    pub instance: String,
    pub files: Vec<DueFile>,
}

/// Up to `limit` albums of `instances`' files that are due.
pub fn due(conn: &Connection, instances: &[String], limit: u32) -> rusqlite::Result<Vec<DueAlbum>> {
    if instances.is_empty() {
        return Ok(Vec::new());
    }
    let marks = vec!["?"; instances.len()].join(", ");
    let mut statement = conn.prepare(&format!(
        "SELECT provider_instance, provider_album, provider_item_id, stamp
         FROM provider_mappings
         WHERE media_type = 'track' AND stamp IS NOT NULL
             AND loudness_stamp IS NOT stamp AND provider_instance IN ({marks})
         ORDER BY provider_instance, provider_album, provider_item_id"
    ))?;
    let mut rows = statement.query(params_from_iter(instances))?;
    let mut albums: BTreeMap<(String, String), ()> = BTreeMap::new();
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        if out.len() + albums.len() >= limit as usize {
            break;
        }
        let instance: String = row.get(0)?;
        match row.get::<_, Option<String>>(1)? {
            Some(album) => {
                albums.insert((instance, album), ());
            }
            None => out.push(DueAlbum {
                instance,
                files: vec![DueFile {
                    item: row.get(2)?,
                    stamp: row.get(3)?,
                }],
            }),
        }
    }
    drop(rows);
    let mut files = conn.prepare(
        "SELECT provider_item_id, stamp FROM provider_mappings
         WHERE media_type = 'track' AND stamp IS NOT NULL
             AND provider_instance = ?1 AND provider_album = ?2
         ORDER BY provider_item_id",
    )?;
    for (instance, album) in albums.into_keys() {
        let files = files
            .query_map(params![instance, album], |row| {
                Ok(DueFile {
                    item: row.get(0)?,
                    stamp: row.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        out.push(DueAlbum { instance, files });
    }
    Ok(out)
}

/// Records what was measured of `instance`'s files, each at the stamp it was measured at: a
/// file that changed meanwhile is left due. A file that could not be measured is recorded as
/// such, and is not tried again until it changes.
pub fn measured(
    conn: &Connection,
    instance: &str,
    files: &[(DueFile, Option<Loudness>)],
) -> rusqlite::Result<()> {
    let mut statement = conn.prepare(
        "UPDATE provider_mappings SET track_gain = ?4, track_peak = ?5, album_gain = ?6,
             album_peak = ?7, loudness = ?8, loudness_stamp = ?3
         WHERE provider_instance = ?1 AND media_type = 'track' AND provider_item_id = ?2
             AND stamp IS ?3",
    )?;
    for (file, loudness) in files {
        statement.execute(params![
            instance,
            file.item,
            file.stamp,
            loudness.map(|l| l.track_gain_db),
            loudness.map(|l| l.track_peak),
            loudness.and_then(|l| l.album_gain_db),
            loudness.and_then(|l| l.album_peak),
            loudness.map(|_| "measured"),
        ])?;
    }
    Ok(())
}

/// How far the library's files are along, of `instances`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LoudnessStatus {
    pub files: u32,
    /// Known from their tags.
    pub tagged: u32,
    pub measured: u32,
    /// Still to be measured.
    pub due: u32,
}

pub fn status(conn: &Connection, instances: &[String]) -> rusqlite::Result<LoudnessStatus> {
    if instances.is_empty() {
        return Ok(LoudnessStatus::default());
    }
    let marks = vec!["?"; instances.len()].join(", ");
    conn.query_row(
        &format!(
            "SELECT count(*),
                 count(*) FILTER (WHERE loudness = 'tags'),
                 count(*) FILTER (WHERE loudness = 'measured'),
                 count(*) FILTER (WHERE stamp IS NOT NULL AND loudness_stamp IS NOT stamp)
             FROM provider_mappings
             WHERE media_type = 'track' AND provider_instance IN ({marks})"
        ),
        params_from_iter(instances),
        |row| {
            Ok(LoudnessStatus {
                files: row.get(0)?,
                tagged: row.get(1)?,
                measured: row.get(2)?,
                due: row.get(3)?,
            })
        },
    )
}
