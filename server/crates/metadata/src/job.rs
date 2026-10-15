//! Looking up the library's albums on MusicBrainz in the background, those with files first,
//! each again every few months. An album is found by the release its files are tagged with, by
//! its barcode, or by a search scored as beets scores it: a close match is kept, a near one
//! suggested to a person, and anything else left alone until the next time.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bopsie_library::Library;
use bopsie_library::enrich::{ACCEPT, AlbumToMatch, Identified, REJECT};
use bopsie_library::ids::IdKind;

use crate::musicbrainz::{MbError, MusicBrainz, Release, phrase};
use crate::score::{Scored, score};

/// How long until an album is looked up again.
pub const REFRESH: Duration = Duration::from_secs(90 * 24 * 3600);
/// Closer than this, with every track on both sides, is the release.
const ACCEPT_BELOW: f64 = 0.04;
/// Closer than this is suggested to a person.
const SUGGEST_BELOW: f64 = 0.25;
/// A release found by the album's barcode is it when the names are this close: the album may
/// hold only some of its tracks.
const BARCODE_NAMES_BELOW: f64 = 0.1;
/// Releases a search finds that are looked up and scored.
const CANDIDATES: usize = 3;

/// What one run did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Summary {
    pub matched: usize,
    pub suggested: usize,
    pub unmatched: usize,
    pub failed: usize,
}

enum Outcome {
    Matched,
    Suggested,
    Unmatched,
}

#[derive(Clone)]
pub struct Enricher {
    library: Library,
    musicbrainz: MusicBrainz,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// What is kept of `release` for `album`, its tracks paired as `scored` paired them.
fn identified(album: &AlbumToMatch, release: &Release, scored: &Scored) -> Identified {
    let mut ids = vec![(IdKind::MbRelease, release.id.clone())];
    if let Some(group) = &release.release_group {
        ids.push((IdKind::MbReleaseGroup, group.id.clone()));
    }
    if let Some(barcode) = &release.barcode {
        ids.push((IdKind::Barcode, barcode.clone()));
    }
    ids.extend(
        release
            .spotify_albums()
            .into_iter()
            .map(|id| (IdKind::Spotify, id)),
    );
    let tracks: Vec<_> = release.media.iter().flat_map(|m| m.tracks.iter()).collect();
    Identified {
        album: ids,
        artists: release
            .artist_credit
            .iter()
            .flat_map(|credit| {
                [
                    (credit.name.clone(), credit.artist.id.clone()),
                    (credit.artist.name.clone(), credit.artist.id.clone()),
                ]
            })
            .collect(),
        tracks: scored
            .pairs
            .iter()
            .map(|(ours, theirs)| {
                let track = tracks[*theirs];
                let mut ids = vec![
                    (IdKind::MbRecording, track.recording.id.clone()),
                    (IdKind::MbTrack, track.id.clone()),
                ];
                ids.extend(
                    track
                        .recording
                        .isrcs
                        .iter()
                        .map(|isrc| (IdKind::Isrc, isrc.clone())),
                );
                (album.track_ids[*ours], ids)
            })
            .collect(),
    }
}

impl Enricher {
    pub fn new(library: Library, musicbrainz: MusicBrainz) -> Self {
        Self {
            library,
            musicbrainz,
        }
    }

    /// Looks up every album that is due, and logs what came of it.
    pub async fn run(&self) -> Summary {
        let mut summary = Summary::default();
        let started = now();
        loop {
            let due = match self
                .library
                .albums_due(started - REFRESH.as_secs() as i64, 50)
                .await
            {
                Ok(due) if !due.is_empty() => due,
                Ok(_) => break,
                Err(error) => {
                    tracing::warn!(%error, "listing albums to look up failed");
                    break;
                }
            };
            for id in due {
                match self.album(id).await {
                    Ok(Outcome::Matched) => summary.matched += 1,
                    Ok(Outcome::Suggested) => summary.suggested += 1,
                    Ok(Outcome::Unmatched) => summary.unmatched += 1,
                    Err(error) => {
                        tracing::warn!(album = id, %error, "looking an album up failed");
                        summary.failed += 1;
                        // Tried again at the next run, unless MusicBrainz cannot be reached
                        // at all, which the next run finds out for itself.
                        if matches!(error, MbError::Unreachable(_) | MbError::Busy) {
                            return self.report(summary);
                        }
                        let _ = self.library.album_checked(id, None).await;
                    }
                }
            }
        }
        self.report(summary)
    }

    fn report(&self, summary: Summary) -> Summary {
        if summary != Summary::default() {
            tracing::info!(
                matched = summary.matched,
                suggested = summary.suggested,
                unmatched = summary.unmatched,
                failed = summary.failed,
                "albums looked up on MusicBrainz"
            );
        }
        summary
    }

    async fn keep(
        &self,
        album: &AlbumToMatch,
        release: &Release,
        scored: &Scored,
    ) -> Result<Outcome, MbError> {
        self.library
            .identify(album.id, identified(album, release, scored))
            .await
            .map_err(|e| MbError::Refused(e.to_string()))?;
        Ok(Outcome::Matched)
    }

    async fn album(&self, id: i64) -> Result<Outcome, MbError> {
        let library_error = |e: bopsie_core::Error| MbError::Refused(e.to_string());
        let Some(album) = self
            .library
            .album_to_match(id)
            .await
            .map_err(library_error)?
        else {
            return Ok(Outcome::Unmatched);
        };
        if album.facts.tracks.is_empty() {
            self.library
                .album_checked(id, None)
                .await
                .map_err(library_error)?;
            return Ok(Outcome::Unmatched);
        }
        let decision = self
            .library
            .release_decision(id)
            .await
            .map_err(library_error)?;
        let rejected = decision
            .as_ref()
            .filter(|(_, decision)| decision == REJECT)
            .map(|(release, _)| release.clone());
        // What a person accepted, or what the files say they are.
        let given: Vec<String> = decision
            .iter()
            .filter(|(_, decision)| decision == ACCEPT)
            .map(|(release, _)| release.clone())
            .chain(
                album
                    .facts
                    .ids
                    .iter()
                    .filter(|(kind, _)| *kind == IdKind::MbRelease)
                    .map(|(_, id)| id.clone()),
            )
            .collect();
        for release_id in given {
            if let Some(release) = self.musicbrainz.release(&release_id).await? {
                let scored = score(&album, &release);
                return self.keep(&album, &release, &scored).await;
            }
        }

        let mut best: Option<(Release, Scored)> = None;
        let consider = |release: Release, best: &mut Option<(Release, Scored)>| {
            if rejected.as_deref() == Some(release.id.as_str()) {
                return;
            }
            let scored = score(&album, &release);
            if best
                .as_ref()
                .is_none_or(|(_, b)| scored.distance < b.distance)
            {
                *best = Some((release, scored));
            }
        };
        // By barcode: the release itself, though the album may hold only some of its tracks.
        let barcodes: Vec<&String> = album
            .facts
            .ids
            .iter()
            .filter(|(kind, _)| *kind == IdKind::Barcode)
            .map(|(_, code)| code)
            .collect();
        if let Some(code) = barcodes.first() {
            let code = code.trim_start_matches('0');
            let found = self
                .musicbrainz
                .search(&format!("barcode:{code} OR barcode:0{code}"), 5)
                .await?;
            for found in found.into_iter().take(2) {
                if let Some(release) = self.musicbrainz.release(&found.id).await? {
                    let scored = score(&album, &release);
                    if scored.name_distance < BARCODE_NAMES_BELOW
                        && rejected.as_deref() != Some(release.id.as_str())
                    {
                        return self.keep(&album, &release, &scored).await;
                    }
                }
            }
        }
        // By name, the releases with as many tracks first.
        let artist = album.facts.artists.join(" ");
        let mut query = format!("release:{}", phrase(&album.facts.name));
        if !artist.trim().is_empty() {
            query.push_str(&format!(" AND artist:{}", phrase(&artist)));
        }
        let mut found = self.musicbrainz.search(&query, 10).await?;
        let count = album.facts.tracks.len() as u32;
        found.sort_by_key(|f| (f.track_count != count, std::cmp::Reverse(f.score)));
        for found in found.into_iter().take(CANDIDATES) {
            if let Some(release) = self.musicbrainz.release(&found.id).await? {
                consider(release, &mut best);
            }
        }

        match best {
            Some((release, scored))
                if scored.distance < ACCEPT_BELOW && scored.missing == 0 && scored.extra == 0 =>
            {
                self.keep(&album, &release, &scored).await
            }
            Some((release, scored)) if scored.distance < SUGGEST_BELOW => {
                self.library
                    .album_checked(
                        id,
                        Some((release.id.clone(), release.describe(), scored.distance)),
                    )
                    .await
                    .map_err(library_error)?;
                Ok(Outcome::Suggested)
            }
            _ => {
                self.library
                    .album_checked(id, None)
                    .await
                    .map_err(library_error)?;
                Ok(Outcome::Unmatched)
            }
        }
    }
}
