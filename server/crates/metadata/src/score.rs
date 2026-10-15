//! How far a release is from an album, as beets measures it: a weighted mean of how far the
//! titles, artists, year, disc count and each track's title, length and place are apart, with
//! a penalty for each track only one side has. 0 is the same; 1 is nothing alike.
//! <https://beets.readthedocs.io/en/stable/reference/config.html#match>

use bopsie_library::enrich::AlbumToMatch;
use bopsie_library::ids::search_key;
use bopsie_library::matching::{TrackFacts, pair_tracks, versioned};

use crate::musicbrainz::{Release, credited};

/// A release's distance from an album, and how its tracks pair with the album's.
#[derive(Debug, Clone, PartialEq)]
pub struct Scored {
    pub distance: f64,
    /// How far the album's title and artists alone are, on the same scale.
    pub name_distance: f64,
    /// Release tracks the album lacks.
    pub missing: usize,
    /// Album tracks the release lacks.
    pub extra: usize,
    /// Album track index to release track, as [`Release::tracks`] numbers them.
    pub pairs: Vec<(usize, usize)>,
}

// beets' default weights.
const ALBUM: f64 = 3.0;
const ARTIST: f64 = 3.0;
const YEAR: f64 = 1.0;
const MEDIUMS: f64 = 1.0;
const TRACKS: f64 = 2.0;
const MISSING: f64 = 0.9;
const UNMATCHED: f64 = 0.6;
const TRACK_TITLE: f64 = 3.0;
const TRACK_LENGTH: f64 = 2.0;
const TRACK_INDEX: f64 = 1.0;
/// Lengths this close are the same.
const LENGTH_GRACE: f64 = 10.0;
/// Lengths this much further apart are as far as they count.
const LENGTH_MAX: f64 = 30.0;

/// Edit distance over the longer length, after the names are normalized as people read them
/// and a leading "the" is dropped.
pub fn string_distance(a: &str, b: &str) -> f64 {
    let normalize = |name: &str| {
        let key = search_key(&versioned(name).base);
        key.strip_prefix("the ").map(str::to_owned).unwrap_or(key)
    };
    let (a, b): (Vec<char>, Vec<char>) = (
        normalize(a).chars().collect(),
        normalize(b).chars().collect(),
    );
    let longest = a.len().max(b.len());
    if longest == 0 {
        return 0.0;
    }
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let substituted = previous + usize::from(ca != cb);
            previous = row[j + 1];
            row[j + 1] = substituted.min(row[j] + 1).min(row[j + 1] + 1);
        }
    }
    row[b.len()] as f64 / longest as f64
}

/// A sum of weighted distances, and the most it could have been.
#[derive(Default)]
struct Sum {
    total: f64,
    most: f64,
}

impl Sum {
    fn add(&mut self, weight: f64, distance: f64) {
        self.total += weight * distance.clamp(0.0, 1.0);
        self.most += weight;
    }

    fn distance(&self) -> f64 {
        if self.most == 0.0 {
            0.0
        } else {
            self.total / self.most
        }
    }
}

impl Release {
    /// Its tracks, in order, as an album's tracks are compared.
    pub fn tracks(&self) -> Vec<TrackFacts> {
        self.media
            .iter()
            .flat_map(|medium| {
                medium.tracks.iter().map(|track| TrackFacts {
                    name: track.title.clone(),
                    artists: track
                        .artist_credit
                        .iter()
                        .map(|credit| credit.name.clone())
                        .collect(),
                    duration_secs: track.length.map_or(0, |ms| (ms / 1000) as u32),
                    disc_number: Some(medium.position.max(1)),
                    track_number: Some(track.position),
                    ids: Vec::new(),
                })
            })
            .collect()
    }
}

fn track_distance(ours: &TrackFacts, theirs: &TrackFacts) -> f64 {
    let mut sum = Sum::default();
    sum.add(TRACK_TITLE, string_distance(&ours.name, &theirs.name));
    if ours.duration_secs > 0 && theirs.duration_secs > 0 {
        let apart = f64::from(ours.duration_secs.abs_diff(theirs.duration_secs));
        sum.add(TRACK_LENGTH, (apart - LENGTH_GRACE) / LENGTH_MAX);
    }
    let place = |track: &TrackFacts| (track.disc_number.unwrap_or(1).max(1), track.track_number);
    if ours.track_number.is_some() {
        sum.add(
            TRACK_INDEX,
            f64::from(u8::from(place(ours) != place(theirs))),
        );
    }
    sum.distance()
}

/// How far `release` is from `album`.
pub fn score(album: &AlbumToMatch, release: &Release) -> Scored {
    let facts = &album.facts;
    let theirs = release.tracks();
    let mut sum = Sum::default();
    sum.add(ALBUM, string_distance(&facts.name, &release.title));
    sum.add(
        ARTIST,
        string_distance(&facts.artists.join(", "), &credited(&release.artist_credit)),
    );
    let name_distance = sum.distance();
    let years = release.years();
    if let Some(year) = album.year
        && !years.is_empty()
    {
        let apart = years.iter().map(|y| y.abs_diff(year)).min().unwrap_or(0);
        sum.add(YEAR, f64::from(apart) / 10.0);
    }
    let discs = |tracks: &[TrackFacts]| {
        let mut discs: Vec<u32> = tracks
            .iter()
            .map(|t| t.disc_number.unwrap_or(1).max(1))
            .collect();
        discs.sort_unstable();
        discs.dedup();
        discs.len()
    };
    if discs(&facts.tracks) > 1 || release.media.len() > 1 {
        sum.add(
            MEDIUMS,
            f64::from(u8::from(discs(&facts.tracks) != release.media.len())),
        );
    }
    let pairs = pair_tracks(&facts.tracks, &theirs);
    for (ours, other) in &pairs {
        sum.add(
            TRACKS,
            track_distance(&facts.tracks[*ours], &theirs[*other]),
        );
    }
    let missing = theirs.len() - pairs.len();
    let extra = facts.tracks.len() - pairs.len();
    for _ in 0..missing {
        sum.add(MISSING, 1.0);
    }
    for _ in 0..extra {
        sum.add(UNMATCHED, 1.0);
    }
    Scored {
        distance: sum.distance(),
        name_distance,
        missing,
        extra,
        pairs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bopsie_library::matching::AlbumFacts;

    pub(crate) fn release(json: serde_json::Value) -> Release {
        serde_json::from_value(json).unwrap()
    }

    fn first_album() -> Release {
        release(serde_json::json!({
            "id": "11111111-1111-1111-1111-111111111111",
            "title": "First Album",
            "date": "2021-03-01",
            "artist-credit": [{"name": "Test Artist", "joinphrase": "",
                "artist": {"id": "22222222-2222-2222-2222-222222222222", "name": "Test Artist"}}],
            "media": [{"position": 1, "format": "Digital Media", "tracks": [
                {"id": "t1", "position": 1, "title": "Opening", "length": 200000,
                 "recording": {"id": "r1"}},
                {"id": "t2", "position": 2, "title": "Middle", "length": 181000,
                 "recording": {"id": "r2"}},
                {"id": "t3", "position": 3, "title": "Closing", "length": 240000,
                 "recording": {"id": "r3"}}
            ]}]
        }))
    }

    fn album(name: &str, tracks: &[(&str, u32)]) -> AlbumToMatch {
        AlbumToMatch {
            id: 1,
            facts: AlbumFacts {
                name: name.to_owned(),
                artists: vec!["Test Artist".to_owned()],
                ids: Vec::new(),
                tracks: tracks
                    .iter()
                    .enumerate()
                    .map(|(i, (title, seconds))| TrackFacts {
                        name: (*title).to_owned(),
                        duration_secs: *seconds,
                        track_number: Some(i as u32 + 1),
                        ..TrackFacts::default()
                    })
                    .collect(),
            },
            track_ids: (1..=tracks.len() as i64).collect(),
            year: Some(2021),
        }
    }

    #[test]
    fn strings_are_as_far_apart_as_their_edits() {
        assert_eq!(string_distance("The Beatles", "Beatles"), 0.0);
        assert_eq!(string_distance("Björk", "bjork"), 0.0);
        assert!((string_distance("Abbey Road", "Abbey Rd") - 0.2).abs() < 1e-9);
        assert_eq!(string_distance("abc", "xyz"), 1.0);
    }

    #[test]
    fn the_same_album_is_very_close() {
        let scored = score(
            &album(
                "First Album",
                &[("Opening", 201), ("Middle", 180), ("Closing", 238)],
            ),
            &first_album(),
        );
        assert!(scored.distance < 0.04, "{scored:?}");
        assert_eq!((scored.missing, scored.extra), (0, 0));
        assert_eq!(scored.pairs, [(0, 0), (1, 1), (2, 2)]);
    }

    #[test]
    fn a_missing_track_or_a_wrong_title_counts() {
        let missing = score(
            &album("First Album", &[("Opening", 200), ("Middle", 181)]),
            &first_album(),
        );
        assert_eq!(missing.missing, 1);
        assert!(
            missing.distance > 0.04 && missing.distance < 0.25,
            "{missing:?}"
        );
        let other = score(
            &album(
                "Something Else",
                &[("One", 90), ("Two", 300), ("Three", 30)],
            ),
            &first_album(),
        );
        assert!(other.distance > 0.25, "{other:?}");
    }
}
