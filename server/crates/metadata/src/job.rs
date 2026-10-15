//! Looking up the library's albums on MusicBrainz in the background, those with files first,
//! each again every few months. An album is found by the release its files are tagged with, by
//! its barcode, or by a search scored as beets scores it: a close match is kept, a near one
//! suggested to a person, and anything else left alone until the next time.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bopsie_core::media::MediaType;
use bopsie_library::enrich::{ACCEPT, AlbumToMatch, ArtistToIllustrate, Identified, REJECT};
use bopsie_library::ids::IdKind;
use bopsie_library::{ItemImage, Library};

use std::collections::{BTreeMap, BTreeSet};

use crate::fanart::Fanart;
use crate::fingerprint::{AcoustId, Heard};
use crate::musicbrainz::{ArtistLinks, MbError, MusicBrainz, Release, phrase};
use crate::score::{Scored, score};
use crate::wikimedia::Wikimedia;

/// The source an artist's images from fanart.tv are kept as.
pub const FANART: &str = "fanarttv";
/// The source an artist's photo from Wikimedia Commons is kept as.
pub const WIKIMEDIA: &str = "wikimedia";
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
/// How long to leave MusicBrainz be when it stays busy.
const BUSY_PAUSE: Duration = Duration::from_secs(60);
/// AcoustID matches this sure are a track's recording.
const HEARD_SCORE: f64 = 0.5;

/// What one run did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Summary {
    pub matched: usize,
    pub suggested: usize,
    pub unmatched: usize,
    pub failed: usize,
    /// Artists given images.
    pub artists: usize,
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
    /// Where cover art comes from, if it is wanted.
    cover_art: Option<String>,
    genres: bool,
    /// Finds albums by their tracks' sound, if it is set up.
    acoustid: Option<AcoustId>,
    /// Looks for artists' images.
    artist_images: bool,
    /// Where artists no catalog has a photo of get theirs, if wanted.
    wikimedia: Option<Wikimedia>,
    /// Where artists' backgrounds, logos and banners come from, if set up.
    fanart: Option<Fanart>,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// How many of a release group's genres are kept.
const GENRES: usize = 3;

/// What is kept of `release` for `album`, its tracks paired as `scored` paired them.
fn identified(
    album: &AlbumToMatch,
    release: &Release,
    scored: &Scored,
    genres: bool,
) -> Identified {
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
    let mut voted: Vec<_> = release
        .release_group
        .iter()
        .flat_map(|group| group.genres.iter())
        .filter(|genre| genre.count > 0)
        .collect();
    voted.sort_by_key(|genre| std::cmp::Reverse(genre.count));
    Identified {
        genres: if genres {
            voted
                .into_iter()
                .take(GENRES)
                .map(|genre| genre.name.clone())
                .collect()
        } else {
            Vec::new()
        },
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
    /// Looks albums up, keeping their ids and genres, and giving the albums without art
    /// theirs from the Cover Art Archive.
    pub fn new(library: Library, musicbrainz: MusicBrainz) -> Self {
        Self {
            library,
            musicbrainz,
            cover_art: Some(crate::musicbrainz::COVER_ART_BASE.to_owned()),
            genres: true,
            acoustid: None,
            artist_images: true,
            wikimedia: None,
            fanart: None,
        }
    }

    /// Gives artists no catalog has a photo of theirs from Wikimedia Commons, with `wikimedia`.
    pub fn wikimedia(mut self, wikimedia: Option<Wikimedia>) -> Self {
        self.wikimedia = wikimedia;
        self
    }

    /// Gives artists their backgrounds, logo and banner from fanart.tv, with `fanart`.
    pub fn fanart(mut self, fanart: Option<Fanart>) -> Self {
        self.fanart = fanart;
        self
    }

    /// Looks for artists' images, or not.
    pub fn artist_images(mut self, artist_images: bool) -> Self {
        self.artist_images = artist_images;
        self
    }

    /// Finds albums a search cannot by their tracks' sound, with `acoustid`.
    pub fn by_sound_with(mut self, acoustid: Option<AcoustId>) -> Self {
        self.acoustid = acoustid;
        self
    }

    /// Takes cover art from the archive at `base`, or none.
    pub fn cover_art(mut self, base: Option<String>) -> Self {
        self.cover_art = base.map(|base| base.trim_end_matches('/').to_owned());
        self
    }

    /// Keeps genres, or not.
    pub fn genres(mut self, genres: bool) -> Self {
        self.genres = genres;
        self
    }

    /// Looks up every album that is due, and logs what came of it.
    pub async fn run(&self) -> Summary {
        self.library.metadata_ran(None);
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
                let mut outcome = self.album(id).await;
                if let Err(MbError::Busy) = outcome {
                    // Busy for longer than a request's own waits: once more, after a pause.
                    tokio::time::sleep(BUSY_PAUSE).await;
                    outcome = self.album(id).await;
                }
                match outcome {
                    Ok(Outcome::Matched) => summary.matched += 1,
                    Ok(Outcome::Suggested) => summary.suggested += 1,
                    Ok(Outcome::Unmatched) => summary.unmatched += 1,
                    Err(error) => {
                        tracing::warn!(album = id, %error, "looking an album up failed");
                        summary.failed += 1;
                        // Tried again at the next run, as is the rest, when MusicBrainz cannot
                        // be reached or stays busy.
                        if matches!(error, MbError::Unreachable(_) | MbError::Busy) {
                            return self.report(summary);
                        }
                        let _ = self.library.album_checked(id, None).await;
                    }
                }
            }
        }
        summary.artists = self.artists(started).await;
        self.report(summary)
    }

    /// Where an artist is on other services, as MusicBrainz links its first id: `None` for
    /// one without, or whose links cannot be read. Fails only when MusicBrainz cannot be
    /// reached or stays busy, for the run to stop and try again later.
    async fn links(&self, artist: &ArtistToIllustrate) -> Result<Option<ArtistLinks>, MbError> {
        let Some(mbid) = artist.musicbrainz.first() else {
            return Ok(None);
        };
        match self.musicbrainz.artist_links(mbid).await {
            Ok(links) => Ok(Some(links)),
            Err(error @ (MbError::Unreachable(_) | MbError::Busy)) => Err(error),
            Err(error) => {
                tracing::debug!(artist = artist.id, %error, "no links for an artist");
                Ok(None)
            }
        }
    }

    /// Gives artists not looked at since `started` less [`REFRESH`] their images: their photo
    /// from a catalog such as Spotify's, by the catalog's id for them, which MusicBrainz links
    /// an artist kept only as files to. Returns how many were given one.
    async fn artists(&self, started: i64) -> usize {
        if !self.artist_images {
            return 0;
        }
        let catalogs = self.library.catalogs().all();
        let mut illustrated = 0;
        loop {
            let due = match self
                .library
                .artists_due(started - REFRESH.as_secs() as i64, 50)
                .await
            {
                Ok(due) if !due.is_empty() => due,
                _ => return illustrated,
            };
            let mut batch: Vec<(ArtistToIllustrate, Option<ArtistLinks>)> =
                Vec::with_capacity(due.len());
            for id in due {
                let Ok(Some(mut artist)) = self.library.artist_to_illustrate(id).await else {
                    continue;
                };
                let mut links = None;
                if artist.spotify.is_empty() {
                    match self.links(&artist).await {
                        Ok(found) => {
                            if let Some(id) = found.as_ref().and_then(|l| l.spotify.clone()) {
                                let _ = self
                                    .library
                                    .link_artist(artist.id, vec![(IdKind::Spotify, id.clone())])
                                    .await;
                                artist.spotify.push(id);
                            }
                            links = found;
                        }
                        Err(_) => return illustrated,
                    }
                }
                batch.push((artist, links));
            }
            let ids: Vec<String> = batch
                .iter()
                .flat_map(|(artist, _)| artist.spotify.clone())
                .collect();
            let mut found: Vec<(String, String, String)> = Vec::new();
            for catalog in &catalogs {
                match catalog.artist_images(&ids).await {
                    Ok(images) => found.extend(
                        images
                            .into_iter()
                            .map(|(id, url)| (catalog.instance_id().to_owned(), id, url)),
                    ),
                    Err(error) => {
                        // Looked for again at the next run, once the catalog answers.
                        tracing::warn!(%error, "cannot fetch artists' images");
                        return illustrated;
                    }
                }
            }
            for (artist, mut links) in batch {
                let photo = found
                    .iter()
                    .find(|(_, id, _)| artist.spotify.contains(id))
                    .cloned();
                let mut given = false;
                if let Some((catalog, _, url)) = photo {
                    let image = ItemImage::Url {
                        provider: catalog.clone(),
                        url,
                    };
                    given = self
                        .library
                        .set_item_images(
                            MediaType::Artist,
                            artist.id,
                            catalog,
                            vec![("thumb".to_owned(), image)],
                        )
                        .await
                        .is_ok();
                } else if let Some(wikimedia) = &self.wikimedia {
                    // No catalog has a photo: Wikimedia Commons may, by Wikidata's entity.
                    if links.is_none() {
                        match self.links(&artist).await {
                            Ok(found) => links = found,
                            Err(_) => return illustrated,
                        }
                    }
                    if let Some(entity) = links.as_ref().and_then(|l| l.wikidata.as_deref()) {
                        match wikimedia.photo(entity).await {
                            Ok(Some((bytes, extension))) => {
                                given = self
                                    .library
                                    .set_item_images(
                                        MediaType::Artist,
                                        artist.id,
                                        WIKIMEDIA.to_owned(),
                                        vec![(
                                            "thumb".to_owned(),
                                            ItemImage::Data { bytes, extension },
                                        )],
                                    )
                                    .await
                                    .is_ok();
                            }
                            Ok(None) => {}
                            Err(error) => {
                                tracing::debug!(artist = artist.id, %error, "no Wikimedia photo");
                            }
                        }
                    }
                }
                // Backgrounds, a logo and a banner, besides a photo, by its MusicBrainz id.
                if let (Some(fanart), Some(mbid)) = (&self.fanart, artist.musicbrainz.first()) {
                    match fanart.artist(mbid).await {
                        Ok(images) if !images.is_empty() => {
                            let images = images
                                .into_iter()
                                .map(|(kind, bytes, extension)| {
                                    (kind, ItemImage::Data { bytes, extension })
                                })
                                .collect();
                            given |= self
                                .library
                                .set_item_images(
                                    MediaType::Artist,
                                    artist.id,
                                    FANART.to_owned(),
                                    images,
                                )
                                .await
                                .is_ok();
                        }
                        Ok(_) => {}
                        Err(MbError::Unreachable(_)) => return illustrated,
                        Err(error) => {
                            tracing::warn!(artist = artist.id, %error, "no images from fanart.tv");
                        }
                    }
                }
                if given {
                    illustrated += 1;
                }
                let _ = self.library.artist_checked(artist.id).await;
            }
        }
    }

    fn report(&self, summary: Summary) -> Summary {
        self.library
            .metadata_ran(Some(bopsie_library::MetadataRunSummary {
                finished: now(),
                matched: summary.matched,
                suggested: summary.suggested,
                unmatched: summary.unmatched,
                failed: summary.failed,
            }));
        if summary != Summary::default() {
            tracing::info!(
                matched = summary.matched,
                suggested = summary.suggested,
                unmatched = summary.unmatched,
                failed = summary.failed,
                artists = summary.artists,
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
            .identify(album.id, identified(album, release, scored, self.genres))
            .await
            .map_err(|e| MbError::Refused(e.to_string()))?;
        if let (Some(base), Some(group), true) =
            (&self.cover_art, &release.release_group, album.needs_art)
            && let Some((data, extension)) = self
                .musicbrainz
                .image(&format!("{base}/release-group/{}/front-500", group.id))
                .await?
        {
            self.library
                .set_cover_art(album.id, data, extension)
                .await
                .map_err(|e| MbError::Refused(e.to_string()))?;
        }
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
                if let Some(acoustid) = &self.acoustid
                    && let Some(outcome) =
                        self.by_sound(acoustid, &album, rejected.as_deref()).await?
                {
                    return Ok(outcome);
                }
                self.library
                    .album_checked(id, None)
                    .await
                    .map_err(library_error)?;
                Ok(Outcome::Unmatched)
            }
        }
    }

    /// Finds the album's release by its tracks' sound: the release most of its fingerprinted
    /// tracks are on, its tracks paired by recording. `None` when no release holds enough of
    /// them.
    async fn by_sound(
        &self,
        acoustid: &AcoustId,
        album: &AlbumToMatch,
        rejected: Option<&str>,
    ) -> Result<Option<Outcome>, MbError> {
        let files = self
            .library
            .track_files(album.id)
            .await
            .map_err(|e| MbError::Refused(e.to_string()))?;
        // The recordings each of the album's tracks sounds like.
        let mut heard: Vec<(usize, Vec<Heard>)> = Vec::new();
        for (track, path, stamp) in files {
            let Some(index) = album.track_ids.iter().position(|t| *t == track) else {
                continue;
            };
            let Some((duration, fingerprint)) = acoustid.fingerprint(&path, &stamp).await else {
                continue;
            };
            let found: Vec<Heard> = acoustid
                .lookup(duration, &fingerprint)
                .await?
                .into_iter()
                .filter(|heard| heard.score >= HEARD_SCORE)
                .collect();
            heard.push((index, found));
        }
        let mut on: BTreeMap<&str, BTreeSet<usize>> = BTreeMap::new();
        for (index, found) in &heard {
            for release in found.iter().flat_map(|heard| heard.releases.iter()) {
                on.entry(release).or_default().insert(*index);
            }
        }
        let ours = album.facts.tracks.len();
        let Some((release_id, held)) = on
            .into_iter()
            .filter(|(release, _)| Some(*release) != rejected)
            .max_by_key(|(_, held)| held.len())
        else {
            return Ok(None);
        };
        if held.len() * 10 <= ours * 6 {
            return Ok(None);
        }
        let Some(release) = self.musicbrainz.release(release_id).await? else {
            return Ok(None);
        };
        let recordings: Vec<&str> = release
            .media
            .iter()
            .flat_map(|medium| medium.tracks.iter())
            .map(|track| track.recording.id.as_str())
            .collect();
        let mut taken = vec![false; recordings.len()];
        let mut pairs = Vec::new();
        for (index, found) in &heard {
            let paired = recordings.iter().enumerate().position(|(at, recording)| {
                !taken[at] && found.iter().any(|heard| heard.recording == *recording)
            });
            if let Some(at) = paired {
                taken[at] = true;
                pairs.push((*index, at));
            }
        }
        pairs.sort_unstable();
        let theirs = recordings.len();
        let scored = Scored {
            distance: 1.0 - pairs.len() as f64 / ours.max(theirs) as f64,
            name_distance: 0.0,
            missing: theirs - pairs.len(),
            extra: ours - pairs.len(),
            pairs,
        };
        tracing::debug!(album = album.id, release = %release.id, ?scored, "found by sound");
        if scored.missing == 0 && scored.extra == 0 {
            return self.keep(album, &release, &scored).await.map(Some);
        }
        if scored.distance < SUGGEST_BELOW {
            self.library
                .album_checked(
                    album.id,
                    Some((release.id.clone(), release.describe(), scored.distance)),
                )
                .await
                .map_err(|e| MbError::Refused(e.to_string()))?;
            return Ok(Some(Outcome::Suggested));
        }
        Ok(None)
    }
}
