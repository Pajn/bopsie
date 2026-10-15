//! Whether two items from different providers are the same: a track, or an album, that one
//! provider has as files and another streams. The rules follow Music Assistant's, which were
//! learnt on real libraries:
//!
//! - An answer is a [`Evidence::Match`], a [`Evidence::NoMatch`], or
//!   [`Evidence::Insufficient`] evidence to say, which never merges anything by itself.
//! - A version that changes the recording, such as live, a remix or a remaster, on one side and
//!   not the other means different items, whatever else agrees. Remasters stay apart even though
//!   MusicBrainz gives one the original's recording.
//! - MusicBrainz ids decide. ISRCs and barcodes are shared across remasters and pressings, so
//!   they count only with durations that agree.
//! - Names compare by [`search_key`], the same normalization candidates are found by.

use std::collections::BTreeSet;

use crate::ids::{IdKind, search_key};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Evidence {
    Match,
    NoMatch,
    Insufficient,
}

/// Versions that make a different recording.
const RECORDING_TOKENS: &[&str] = &[
    "live",
    "remix",
    "acoustic",
    "demo",
    "instrumental",
    "karaoke",
    "cover",
    "session",
    "remaster",
    "edit",
    "extended",
    "mono",
    "unplugged",
    "orchestral",
];

/// Versions of an album that hold more or other tracks of the same recordings.
const EDITION_TOKENS: &[&str] = &[
    "deluxe",
    "expanded",
    "anniversary",
    "bonus",
    "special",
    "collector",
    "complete",
];

/// A name without its version, and the version's tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Versioned {
    pub base: String,
    pub recording: BTreeSet<String>,
    pub edition: BTreeSet<String>,
}

fn token(word: &str) -> Option<String> {
    let word = word.to_lowercase();
    let word = match word.as_str() {
        "remastered" | "remasters" | "remastering" => "remaster".to_owned(),
        "remixed" | "remixes" => "remix".to_owned(),
        "unplugged" => "unplugged".to_owned(),
        "collectors" | "collector's" => "collector".to_owned(),
        "demos" => "demo".to_owned(),
        _ => word,
    };
    (RECORDING_TOKENS.contains(&word.as_str()) || EDITION_TOKENS.contains(&word.as_str()))
        .then_some(word)
}

/// Splits `name` into what it is and which version: "Song (2011 Remaster)" is "Song" with
/// remaster; "Album - Live at X" is "Album", live. Parentheses that say no version, as in
/// "Song (Interlude)", stay part of the name. Apple's " - EP" and " - Single" are dropped.
pub fn versioned(name: &str) -> Versioned {
    let mut base = name.trim().to_owned();
    for suffix in [" - EP", " - Single", " (EP)", " [EP]"] {
        if base.len() > suffix.len() && base.ends_with(suffix) {
            base.truncate(base.len() - suffix.len());
        }
    }
    let mut recording = BTreeSet::new();
    let mut edition = BTreeSet::new();
    let mut take = |part: &str| -> bool {
        let words: Vec<&str> = part
            .split(|c: char| !c.is_alphanumeric() && c != '\'')
            .filter(|w| !w.is_empty())
            .collect();
        let tokens: Vec<String> = words.iter().filter_map(|w| token(w)).collect();
        let is_feature = words.first().is_some_and(|w| {
            matches!(
                w.to_lowercase().as_str(),
                "feat" | "ft" | "featuring" | "with"
            )
        });
        if tokens.is_empty() && !is_feature {
            return false;
        }
        for token in tokens {
            if RECORDING_TOKENS.contains(&token.as_str()) {
                recording.insert(token);
            } else {
                edition.insert(token);
            }
        }
        true
    };
    // Bracketed parts, from the end.
    loop {
        let trimmed = base.trim_end();
        let Some(close) = trimmed.chars().last().filter(|c| matches!(c, ')' | ']')) else {
            break;
        };
        let open = if close == ')' { '(' } else { '[' };
        let Some(start) = trimmed.rfind(open) else {
            break;
        };
        let inner = &trimmed[start + 1..trimmed.len() - 1];
        if !take(inner) {
            break;
        }
        base = trimmed[..start].trim_end().to_owned();
    }
    // A dash-separated version: "Song - 2011 Remaster", "Album - Live at Wembley".
    if let Some(at) = base.rfind(" - ") {
        let (head, tail) = base.split_at(at);
        if take(&tail[3..]) {
            base = head.trim_end().to_owned();
        }
    }
    Versioned {
        base: search_key(&base),
        recording,
        edition,
    }
}

/// What a comparison knows of a track.
#[derive(Debug, Clone, Default)]
pub struct TrackFacts {
    pub name: String,
    pub artists: Vec<String>,
    pub duration_secs: u32,
    pub disc_number: Option<u32>,
    pub track_number: Option<u32>,
    pub ids: Vec<(IdKind, String)>,
}

impl TrackFacts {
    /// Where it is on its album; a missing disc number is disc 1, as files often leave it out.
    fn position(&self) -> Option<(u32, u32)> {
        Some((self.disc_number.unwrap_or(1).max(1), self.track_number?))
    }
}

/// What a comparison knows of an album.
#[derive(Debug, Clone, Default)]
pub struct AlbumFacts {
    pub name: String,
    pub artists: Vec<String>,
    pub ids: Vec<(IdKind, String)>,
    pub tracks: Vec<TrackFacts>,
}

fn values(ids: &[(IdKind, String)], kind: IdKind) -> BTreeSet<&str> {
    ids.iter()
        .filter(|(k, _)| *k == kind)
        .map(|(_, v)| v.as_str())
        .collect()
}

/// What ids of `kind` say: `Some(true)` when the two share one, `Some(false)` when both have
/// some and share none, `None` when either has none.
fn shared(a: &[(IdKind, String)], b: &[(IdKind, String)], kind: IdKind) -> Option<bool> {
    let (a, b) = (values(a, kind), values(b, kind));
    if a.is_empty() || b.is_empty() {
        return None;
    }
    Some(!a.is_disjoint(&b))
}

fn artists_overlap(a: &[String], b: &[String]) -> bool {
    let a: BTreeSet<String> = a.iter().map(|n| search_key(n)).collect();
    b.iter().any(|n| a.contains(&search_key(n)))
}

/// Whether two tracks are the same recording. `same_album` says they are on albums already
/// known to be the same, where their positions count.
pub fn compare_track(a: &TrackFacts, b: &TrackFacts, same_album: bool) -> Evidence {
    let (va, vb) = (versioned(&a.name), versioned(&b.name));
    if va.recording != vb.recording {
        return Evidence::NoMatch;
    }
    match shared(&a.ids, &b.ids, IdKind::MbRecording) {
        Some(true) => return Evidence::Match,
        Some(false) => return Evidence::NoMatch,
        None => {}
    }
    let close = |seconds: u32| a.duration_secs.abs_diff(b.duration_secs) <= seconds;
    if shared(&a.ids, &b.ids, IdKind::Isrc) == Some(true) && close(8) {
        return Evidence::Match;
    }
    if va.base != vb.base || !artists_overlap(&a.artists, &b.artists) {
        return Evidence::NoMatch;
    }
    if same_album {
        match (a.position(), b.position()) {
            (Some(pa), Some(pb)) if pa == pb => return Evidence::Match,
            (Some(_), Some(_)) if !close(2) => return Evidence::NoMatch,
            _ => {}
        }
    }
    if close(2) {
        Evidence::Match
    } else {
        Evidence::Insufficient
    }
}

/// Whether two albums are the same release: one provider's files and another's stream.
pub fn compare_album(a: &AlbumFacts, b: &AlbumFacts) -> Evidence {
    let (va, vb) = (versioned(&a.name), versioned(&b.name));
    if va.recording != vb.recording {
        return Evidence::NoMatch;
    }
    let same_edition = va.edition == vb.edition;
    // A link to the same Spotify album, as MusicBrainz records, or the same release.
    for kind in [IdKind::Spotify, IdKind::MbRelease] {
        if shared(&a.ids, &b.ids, kind) == Some(true) {
            return Evidence::Match;
        }
    }
    // Different release groups are different albums; the same one may be another edition of
    // it, which the tracks tell.
    if shared(&a.ids, &b.ids, IdKind::MbReleaseGroup) == Some(false) {
        return Evidence::NoMatch;
    }
    if va.base != vb.base || !artists_overlap(&a.artists, &b.artists) {
        return Evidence::NoMatch;
    }
    if a.tracks.is_empty() || b.tracks.is_empty() {
        return Evidence::Insufficient;
    }
    let pairs = pair_tracks(&a.tracks, &b.tracks);
    // Any track at the same place that is not the same: a different album.
    for (ia, ib) in &pairs {
        let (ta, tb) = (&a.tracks[*ia], &b.tracks[*ib]);
        if ta.position().is_some()
            && ta.position() == tb.position()
            && compare_track(ta, tb, true) == Evidence::NoMatch
        {
            return Evidence::NoMatch;
        }
    }
    let matched = pairs
        .iter()
        .filter(|(ia, ib)| compare_track(&a.tracks[*ia], &b.tracks[*ib], true) == Evidence::Match)
        .count();
    let (fewer, more) = (
        a.tracks.len().min(b.tracks.len()),
        a.tracks.len().max(b.tracks.len()),
    );
    if matched == fewer && same_edition && (fewer == more || partial(&a.tracks, &b.tracks)) {
        Evidence::Match
    } else if matched == fewer {
        // One holds the other and more, as a deluxe edition does: for a person to say.
        Evidence::Insufficient
    } else if matched * 2 >= fewer {
        Evidence::Insufficient
    } else {
        Evidence::NoMatch
    }
}

/// Whether the album with fewer tracks holds some of the other's rather than a shorter
/// edition of it: its tracks are not the other's first ones, as when only the songs someone
/// liked are saved. A shorter edition leaves out the end, where bonus tracks go.
fn partial(a: &[TrackFacts], b: &[TrackFacts]) -> bool {
    let (fewer, more) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    let mut held: Vec<_> = fewer.iter().filter_map(TrackFacts::position).collect();
    let mut first: Vec<_> = more.iter().filter_map(TrackFacts::position).collect();
    held.sort_unstable();
    first.sort_unstable();
    first.truncate(held.len());
    held.len() == fewer.len() && held != first
}

/// Pairs two albums' tracks: by place, then by name among what is left. Returns indices.
pub fn pair_tracks(a: &[TrackFacts], b: &[TrackFacts]) -> Vec<(usize, usize)> {
    let mut pairs = Vec::new();
    let mut taken_a = vec![false; a.len()];
    let mut taken_b = vec![false; b.len()];
    for (ia, ta) in a.iter().enumerate() {
        let Some(position) = ta.position() else {
            continue;
        };
        if let Some(ib) =
            (0..b.len()).find(|ib| !taken_b[*ib] && b[*ib].position() == Some(position))
        {
            taken_a[ia] = true;
            taken_b[ib] = true;
            pairs.push((ia, ib));
        }
    }
    for (ia, ta) in a.iter().enumerate() {
        if taken_a[ia] {
            continue;
        }
        let name = versioned(&ta.name);
        if let Some(ib) = (0..b.len()).find(|ib| !taken_b[*ib] && versioned(&b[*ib].name) == name) {
            taken_a[ia] = true;
            taken_b[ib] = true;
            pairs.push((ia, ib));
        }
    }
    pairs.sort_unstable();
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(name: &str, number: u32, seconds: u32) -> TrackFacts {
        TrackFacts {
            name: name.to_owned(),
            artists: vec!["Test Artist".to_owned()],
            duration_secs: seconds,
            disc_number: Some(1),
            track_number: Some(number),
            ids: Vec::new(),
        }
    }

    fn album(name: &str, tracks: &[(&str, u32)]) -> AlbumFacts {
        AlbumFacts {
            name: name.to_owned(),
            artists: vec!["Test Artist".to_owned()],
            ids: Vec::new(),
            tracks: tracks
                .iter()
                .enumerate()
                .map(|(i, (name, seconds))| track(name, i as u32 + 1, *seconds))
                .collect(),
        }
    }

    const TRACKS: &[(&str, u32)] = &[("Opening", 200), ("Middle", 180), ("Closing", 240)];

    #[test]
    fn versions_are_read_from_names() {
        let v = versioned("Song (2011 Remaster)");
        assert_eq!(v.base, "song");
        assert!(v.recording.contains("remaster"));
        let v = versioned("Song - Remastered 2009");
        assert_eq!((v.base.as_str(), v.recording.len()), ("song", 1));
        let v = versioned("Album (Deluxe Edition)");
        assert!(v.recording.is_empty() && v.edition.contains("deluxe"));
        assert_eq!(versioned("Song (Interlude)").base, "song interlude");
        assert_eq!(versioned("Song (feat. Someone)").base, "song");
        assert_eq!(versioned("Album - EP").base, "album");
        assert_eq!(versioned("Live Forever").base, "live forever");
        let v = versioned("Album - Live at Wembley");
        assert_eq!(
            (v.base.as_str(), v.recording.contains("live")),
            ("album", true)
        );
    }

    #[test]
    fn the_same_album_matches() {
        let files = album("First Album", TRACKS);
        let mut stream = album(
            "First Album",
            &[("Opening", 201), ("Middle", 179), ("Closing", 240)],
        );
        stream.tracks[1].disc_number = None;
        assert_eq!(compare_album(&files, &stream), Evidence::Match);
        // Apple's suffix does not tell them apart.
        let ep = album("First Album - EP", TRACKS);
        assert_eq!(compare_album(&files, &ep), Evidence::Match);
    }

    #[test]
    fn remasters_and_live_albums_stay_apart() {
        let files = album("First Album", TRACKS);
        let remaster = album("First Album (2020 Remaster)", TRACKS);
        assert_eq!(compare_album(&files, &remaster), Evidence::NoMatch);
        let live = album("First Album (Live)", TRACKS);
        assert_eq!(compare_album(&files, &live), Evidence::NoMatch);
        // Even with the same release group, as MusicBrainz groups remasters.
        let mut grouped = remaster.clone();
        let mut original = files.clone();
        for item in [&mut grouped, &mut original] {
            item.ids.push((IdKind::MbReleaseGroup, "g".to_owned()));
        }
        assert_eq!(compare_album(&original, &grouped), Evidence::NoMatch);
    }

    #[test]
    fn a_deluxe_edition_is_for_a_person_to_say() {
        let files = album("First Album", TRACKS);
        let deluxe = album(
            "First Album (Deluxe)",
            &[
                ("Opening", 200),
                ("Middle", 180),
                ("Closing", 240),
                ("Bonus", 100),
            ],
        );
        assert_eq!(compare_album(&files, &deluxe), Evidence::Insufficient);
        // Without saying so, more tracks is still not certain.
        let longer = album(
            "First Album",
            &[
                ("Opening", 200),
                ("Middle", 180),
                ("Closing", 240),
                ("Bonus", 100),
            ],
        );
        assert_eq!(compare_album(&files, &longer), Evidence::Insufficient);
    }

    #[test]
    fn some_of_an_albums_tracks_are_that_album() {
        let files = album("First Album", TRACKS);
        // The songs someone liked, saved with their album.
        let liked = AlbumFacts {
            tracks: vec![track("Middle", 2, 180), track("Closing", 3, 240)],
            ..album("First Album", &[])
        };
        assert_eq!(compare_album(&files, &liked), Evidence::Match);
        // Its first tracks may be a shorter edition, which is for a person to say.
        let first = album("First Album", &TRACKS[..2]);
        assert_eq!(compare_album(&files, &first), Evidence::Insufficient);
    }

    #[test]
    fn different_albums_of_the_same_name_do_not_match() {
        let files = album("Greatest Hits", TRACKS);
        let other = album(
            "Greatest Hits",
            &[("Alpha", 100), ("Beta", 300), ("Gamma", 90)],
        );
        assert_eq!(compare_album(&files, &other), Evidence::NoMatch);
        let mut other_artist = album("First Album", TRACKS);
        other_artist.artists = vec!["Someone Else".to_owned()];
        assert_eq!(compare_album(&files, &other_artist), Evidence::NoMatch);
    }

    #[test]
    fn ids_decide_before_names() {
        let mut files = album("First Album", TRACKS);
        let mut renamed = album("1st Album", TRACKS);
        files.ids.push((IdKind::MbRelease, "r1".to_owned()));
        renamed.ids.push((IdKind::MbRelease, "r1".to_owned()));
        assert_eq!(compare_album(&files, &renamed), Evidence::Match);
        let mut linked = album("Something Else", &[]);
        linked.ids.push((IdKind::Spotify, "abc".to_owned()));
        files.ids.push((IdKind::Spotify, "abc".to_owned()));
        assert_eq!(compare_album(&files, &linked), Evidence::Match);
    }

    #[test]
    fn tracks_match_by_recording_isrc_or_name() {
        let a = track("Opening", 1, 200);
        assert_eq!(
            compare_track(&a, &track("Opening", 1, 201), false),
            Evidence::Match
        );
        // A remaster of it, even with the original's recording id.
        let mut original = a.clone();
        let mut remaster = track("Opening (2011 Remaster)", 1, 200);
        original.ids.push((IdKind::MbRecording, "x".to_owned()));
        remaster.ids.push((IdKind::MbRecording, "x".to_owned()));
        assert_eq!(
            compare_track(&original, &remaster, false),
            Evidence::NoMatch
        );
        // A shared ISRC needs durations that agree.
        let mut near = track("Intro", 1, 205);
        let mut far = track("Intro", 1, 225);
        let mut isrc = a.clone();
        for t in [&mut near, &mut far, &mut isrc] {
            t.ids.push((IdKind::Isrc, "USUM71703861".to_owned()));
        }
        assert_eq!(compare_track(&isrc, &near, false), Evidence::Match);
        assert_ne!(compare_track(&isrc, &far, false), Evidence::Match);
        // Same place on the same album, whatever its length.
        assert_eq!(
            compare_track(&a, &track("Opening", 1, 260), true),
            Evidence::Match
        );
        assert_eq!(
            compare_track(&a, &track("Opening", 1, 260), false),
            Evidence::Insufficient
        );
    }

    #[test]
    fn tracks_pair_by_place_then_name() {
        let a = vec![track("A", 1, 1), track("B", 2, 1), track("C", 0, 1)];
        let mut b = vec![track("C", 9, 1), track("B", 2, 1), track("A", 1, 1)];
        b[0].track_number = None;
        assert_eq!(pair_tracks(&a, &b), [(0, 2), (1, 1), (2, 0)]);
    }
}
