//! The SQLite library: schema and migrations, the scanner, FTS5 search, the artwork cache, and
//! the `MusicProvider` trait with its registry.

pub mod catalog;
pub mod edit;
pub mod enrich;
pub mod ids;
pub mod loudness;
pub mod matching;
pub mod provider;
pub mod query;
pub mod reconcile;
pub mod scan;
mod schema;
pub mod search;

use bopsie_audio::engine::Playable;
use bopsie_audio::loudness::Loudness;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use bopsie_core::db::Database;
use bopsie_core::events::{Event, EventBus, EventType};
use bopsie_core::media::{MediaItem, MediaType, Track, library_uri, parse_library_uri};
use bopsie_core::{Error, Result};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

use catalog::Catalogs;
use provider::{EntryKind, MusicProvider, ScannedImage};
use scan::{ARTWORK_CACHE, Changes, ImageRef, PreparedTrack, Source};

pub use query::ListQuery;
pub use schema::MIGRATIONS;

/// Tracks are read and written this many at a time, one transaction per batch.
const BATCH: usize = 64;

#[derive(Clone)]
pub struct Library {
    db: Database,
    events: EventBus,
    providers: Arc<Vec<Arc<dyn MusicProvider>>>,
    /// Providers searched and browsed as they are, such as Spotify.
    catalogs: Catalogs,
    /// Embedded art, stored by content hash.
    artwork_dir: PathBuf,
    /// One sync at a time.
    syncing: Arc<tokio::sync::Mutex<()>>,
    last_sync: Arc<std::sync::Mutex<Option<LastSync>>>,
    /// Woken when an album should be looked up on a metadata service now.
    metadata_wanted: Arc<tokio::sync::Notify>,
    /// The metadata job: whether it runs, and how its last run went.
    metadata_run: Arc<std::sync::Mutex<MetadataRun>>,
    /// Woken after a sync, which may leave files to measure.
    loudness_wanted: Arc<tokio::sync::Notify>,
    /// Whether files are being measured.
    measuring: Arc<std::sync::atomic::AtomicBool>,
}

/// What a run of measuring files did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Measured {
    pub albums: usize,
    pub files: usize,
    /// Files that could not be measured.
    pub failed: usize,
}

/// An image for [`Library::set_item_images`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemImage {
    /// At a URL provider instance `provider` fetches, as Spotify's image server's.
    Url { provider: String, url: String },
    /// Downloaded: kept with the library's artwork.
    Data {
        bytes: Vec<u8>,
        extension: &'static str,
    },
}

/// How looking albums up on a metadata service goes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetadataRun {
    pub running: bool,
    /// The last run that finished: when, in Unix seconds, and what came of it.
    pub last: Option<MetadataRunSummary>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetadataRunSummary {
    pub finished: i64,
    pub matched: usize,
    pub suggested: usize,
    pub unmatched: usize,
    pub failed: usize,
}

/// How far the library's albums have been looked up.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetadataStatus {
    pub albums: u32,
    /// Asked about since they last changed or were refreshed.
    pub checked: u32,
    /// Identified as a release.
    pub identified: u32,
    /// With a release suggested, awaiting a person.
    pub suggested: u32,
}

/// How the last sync went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastSync {
    /// Unix seconds.
    pub started: i64,
    pub finished: i64,
    pub added: usize,
    pub updated: usize,
    pub deleted: usize,
    /// Providers whose scan failed, and why.
    pub failed: Vec<(String, String)>,
}

fn now() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|_| Error::unexpected())
}

/// How many changed items are read at a time to publish them.
const PUBLISH_BATCH: usize = 200;

/// A provider holding a track: its id there, and how loud the track is there.
struct TrackSource {
    instance: String,
    item: String,
    loudness: Option<Loudness>,
}

impl Library {
    pub fn new(
        db: Database,
        events: EventBus,
        providers: Vec<Arc<dyn MusicProvider>>,
        artwork_dir: PathBuf,
    ) -> Self {
        Self {
            db,
            events,
            providers: Arc::new(providers),
            catalogs: Catalogs::default(),
            artwork_dir,
            syncing: Arc::default(),
            last_sync: Arc::default(),
            metadata_wanted: Arc::default(),
            metadata_run: Arc::default(),
            loudness_wanted: Arc::default(),
            measuring: Arc::default(),
        }
    }

    /// Runs `f` on the database thread; a slow run is logged as where `read` was called.
    #[track_caller]
    fn read<T, F>(&self, f: F) -> impl std::future::Future<Output = Result<T>> + use<T, F>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> rusqlite::Result<T> + Send + 'static,
    {
        let call = self.db.call(move |conn| Ok(f(conn)?));
        async move { Ok(call.await?) }
    }

    pub async fn list(&self, media_type: MediaType, query: ListQuery) -> Result<Vec<MediaItem>> {
        self.read(move |conn| query::list(conn, media_type, &query))
            .await
    }

    pub async fn count(&self, media_type: MediaType, query: ListQuery) -> Result<u32> {
        self.read(move |conn| query::count(conn, media_type, &query))
            .await
    }

    /// The item with library id `id`; or, for one merged into another, the item it became,
    /// as every lookup by id here does.
    pub async fn item(&self, media_type: MediaType, id: i64) -> Result<Option<MediaItem>> {
        self.read(move |conn| {
            let id = query::resolve(conn, media_type, id)?;
            query::item(conn, media_type, id)
        })
        .await
    }

    pub async fn album_tracks(&self, album_id: i64) -> Result<Vec<bopsie_core::media::Track>> {
        self.read(move |conn| {
            let id = query::resolve(conn, MediaType::Album, album_id)?;
            query::album_tracks(conn, id)
        })
        .await
    }

    pub async fn artist_albums(&self, artist_id: i64) -> Result<Vec<bopsie_core::media::Album>> {
        self.read(move |conn| {
            let id = query::resolve(conn, MediaType::Artist, artist_id)?;
            query::artist_albums(conn, id)
        })
        .await
    }

    pub async fn artist_tracks(&self, artist_id: i64) -> Result<Vec<bopsie_core::media::Track>> {
        self.read(move |conn| {
            let id = query::resolve(conn, MediaType::Artist, artist_id)?;
            query::artist_tracks(conn, id)
        })
        .await
    }

    /// The item library id `id` of `media_type` is now: itself, or what it was merged into.
    async fn resolved(&self, media_type: MediaType, id: i64) -> Result<i64> {
        self.read(move |conn| query::resolve(conn, media_type, id))
            .await
    }

    /// A playlist's tracks in order. A playlist a catalog keeps, such as a Spotify one, is
    /// asked for them, so that its tracks need not be in the library.
    pub async fn playlist_tracks(
        &self,
        playlist_id: i64,
    ) -> Result<Vec<bopsie_core::media::Track>> {
        let playlist_id = self.resolved(MediaType::Playlist, playlist_id).await?;
        if let Some((catalog, id)) = self
            .kept_by_catalog(MediaType::Playlist, playlist_id)
            .await?
        {
            return Ok(catalog.playlist_tracks(&id).await?);
        }
        self.read(move |conn| query::playlist_tracks(conn, playlist_id))
            .await
    }

    /// The catalog that keeps library item `id`, and its id there, if one does.
    async fn kept_by_catalog(
        &self,
        media_type: MediaType,
        id: i64,
    ) -> Result<Option<(Arc<dyn catalog::Catalog>, String)>> {
        let mappings: Vec<(String, String)> = self
            .read(move |conn| {
                let mut statement = conn.prepare(
                    "SELECT provider_instance, provider_item_id FROM provider_mappings
                     WHERE media_type = ?1 AND item_id = ?2 ORDER BY priority DESC",
                )?;
                statement
                    .query_map((media_type.as_str(), id), |row| {
                        Ok((row.get(0)?, row.get(1)?))
                    })?
                    .collect()
            })
            .await?;
        Ok(mappings
            .into_iter()
            .find_map(|(instance, item)| Some((self.catalogs.get(&instance)?, item))))
    }

    pub async fn search(
        &self,
        media_type: MediaType,
        text: String,
        limit: u32,
    ) -> Result<Vec<MediaItem>> {
        self.read(move |conn| query::search(conn, media_type, &text, limit))
            .await
    }

    /// What the track plays from: the first of its providers that can play it.
    pub async fn track_media(&self, track_id: i64) -> Result<Option<Playable>> {
        let track_id = self.resolved(MediaType::Track, track_id).await?;
        let mappings = self.track_sources(track_id).await?;
        self.media_from(mappings).await
    }

    /// The providers holding a track, its id with each and how loud it is there, the
    /// preferred first.
    async fn track_sources(&self, track_id: i64) -> Result<Vec<TrackSource>> {
        self.read(move |conn| {
            let mut statement = conn.prepare(
                "SELECT provider_instance, provider_item_id, track_gain, track_peak, album_gain,
                     album_peak
                 FROM provider_mappings
                 WHERE media_type = 'track' AND item_id = ?1 ORDER BY priority DESC",
            )?;
            statement
                .query_map([track_id], |row| {
                    let track_gain: Option<f32> = row.get(2)?;
                    Ok(TrackSource {
                        instance: row.get(0)?,
                        item: row.get(1)?,
                        loudness: match track_gain {
                            Some(track_gain_db) => Some(Loudness {
                                track_gain_db,
                                track_peak: row.get::<_, Option<f32>>(3)?.unwrap_or(0.0),
                                album_gain_db: row.get(4)?,
                                album_peak: row.get(5)?,
                            }),
                            None => None,
                        },
                    })
                })?
                .collect()
        })
        .await
    }

    /// What the first of `sources` that can play its item plays.
    async fn media_from(&self, sources: Vec<TrackSource>) -> Result<Option<Playable>> {
        let providers = self.providers.clone();
        let catalogs = self.catalogs.clone();
        blocking(move || {
            sources.into_iter().find_map(|source| {
                let input = match providers
                    .iter()
                    .find(|p| p.instance_id() == source.instance)
                {
                    Some(provider) => provider.media(&source.item),
                    None => catalogs.get(&source.instance)?.media(&source.item),
                }?;
                Some(Playable {
                    input,
                    loudness: source.loudness,
                })
            })
        })
        .await
    }

    /// The providers searched and browsed as they are, such as Spotify.
    pub fn catalogs(&self) -> &Catalogs {
        &self.catalogs
    }

    /// The library item that `instance`'s item `id` is, if the library holds it.
    pub async fn by_mapping(
        &self,
        instance: &str,
        media_type: MediaType,
        id: &str,
    ) -> Result<Option<MediaItem>> {
        let (instance, id) = (instance.to_owned(), id.to_owned());
        let item_id: Option<i64> = self
            .read(move |conn| {
                use rusqlite::OptionalExtension;
                conn.query_row(
                    "SELECT item_id FROM provider_mappings
                     WHERE provider_instance = ?1 AND media_type = ?2 AND provider_item_id = ?3",
                    (&instance, media_type.as_str(), &id),
                    |row| row.get(0),
                )
                .optional()
            })
            .await?;
        match item_id {
            Some(item_id) => self.item(media_type, item_id).await,
            None => Ok(None),
        }
    }

    /// The tracks a URI stands for, with what they play from: a track itself, an album's or
    /// artist's tracks, or a playlist's, in order, from the library or from a catalog. Tracks
    /// nothing can play are left out.
    pub async fn playable(&self, uri: &str) -> Result<Vec<(Track, Playable)>> {
        let not_found = || {
            Error::new(
                bopsie_core::ErrorCode::MediaNotFound,
                format!("{uri} not found"),
            )
        };
        let Some((media_type, id)) = parse_library_uri(uri) else {
            let (scheme, media_type, id) = catalog::parse_uri(uri).ok_or_else(not_found)?;
            let catalog = self.catalogs.get(scheme).ok_or_else(not_found)?;
            let tracks = match media_type {
                MediaType::Track => match catalog.item(MediaType::Track, id).await? {
                    MediaItem::Track(track) => vec![*track],
                    _ => return Err(not_found()),
                },
                MediaType::Album => catalog.album_tracks(id).await?,
                MediaType::Artist => catalog.artist_tracks(id).await?,
                MediaType::Playlist => catalog.playlist_tracks(id).await?,
            };
            return Ok(tracks
                .into_iter()
                .filter_map(|track| {
                    let media = catalog.media(&track.fields.item_id)?;
                    Some((track, media.into()))
                })
                .collect());
        };
        let tracks = match media_type {
            MediaType::Track => match self.item(MediaType::Track, id).await? {
                Some(MediaItem::Track(track)) => vec![*track],
                _ => return Err(not_found()),
            },
            MediaType::Album => self.album_tracks(id).await?,
            MediaType::Artist => self.artist_tracks(id).await?,
            MediaType::Playlist => self.playlist_tracks(id).await?,
        };
        let mut sources = Vec::with_capacity(tracks.len());
        for track in &tracks {
            sources.push(match track.fields.item_id.parse() {
                Ok(track_id) if track.fields.provider == bopsie_core::media::LIBRARY => {
                    Some(self.track_sources(track_id).await?)
                }
                _ => None,
            });
        }
        // An album plays from one source when one holds every track, so that it stays gapless
        // and evenly loud; the preferred such source wins.
        let shared: Option<String> = sources
            .iter()
            .flatten()
            .next()
            .and_then(|first| {
                first.iter().map(|s| &s.instance).find(|instance| {
                    sources
                        .iter()
                        .flatten()
                        .all(|mappings| mappings.iter().any(|s| s.instance == **instance))
                })
            })
            .cloned();
        let mut playable = Vec::with_capacity(tracks.len());
        for (track, mappings) in tracks.into_iter().zip(sources) {
            // A catalog's playlist holds the catalog's own tracks.
            let media = match mappings {
                Some(mut mappings) => {
                    if let Some(shared) = &shared {
                        mappings.sort_by_key(|source| &source.instance != shared);
                    }
                    self.media_from(mappings).await?
                }
                _ => self
                    .catalogs
                    .get(&track.fields.provider)
                    .and_then(|catalog| catalog.media(&track.fields.item_id))
                    .map(Playable::from),
            };
            if let Some(media) = media {
                playable.push((track, media));
            }
        }
        Ok(playable)
    }

    /// The bytes of the image with this proxy id, if the library knows it.
    pub async fn image(&self, proxy_id: String) -> Result<Option<Vec<u8>>> {
        let Some(image) = self
            .read(move |conn| query::image(conn, Some(&proxy_id)))
            .await?
        else {
            return Ok(None);
        };
        if image.provider == ARTWORK_CACHE || image.provider == scan::COVER_ART {
            let path = self.artwork_dir.join(&image.path);
            return Ok(blocking(move || std::fs::read(path)).await?.ok());
        }
        let Some(provider) = self
            .providers
            .iter()
            .find(|p| p.instance_id() == image.provider)
            .cloned()
        else {
            return Ok(None);
        };
        Ok(blocking(move || provider.image(&image.path)).await?.ok())
    }

    /// Brings the library up to date with every provider, then publishes what changed.
    /// Providers that fail are logged and skipped, so one unreachable source does not hold
    /// back the rest.
    pub async fn sync(&self) -> Result<Changes> {
        self.sync_with(false).await
    }

    /// As [`Library::sync`]. A provider that lists nothing while the library still has
    /// tracks from it, such as a folder whose disk is not mounted, is taken as unavailable
    /// and its tracks are kept, unless `force` says it really is empty now.
    pub async fn sync_with(&self, force: bool) -> Result<Changes> {
        let _syncing = self.syncing.lock().await;
        let started = now();
        let mut changes = Changes::default();
        let mut failed = Vec::new();
        for provider in self.providers.iter() {
            match self.scan(provider.clone(), force).await {
                Ok(provider_changes) => changes.merge(provider_changes),
                Err(error) => {
                    tracing::warn!(provider = provider.instance_id(), %error, "scan failed");
                    failed.push((provider.instance_id().to_owned(), error.to_string()));
                }
            }
        }
        let pruned = self
            .db
            .call(|conn| {
                let tx = conn.transaction()?;
                let mut pruned = Changes::default();
                scan::repair_doubled_credits(&tx, &mut pruned)?;
                // One release from several providers becomes one album.
                reconcile::reconcile(&tx, now(), &mut pruned)?;
                // What was edited by hand is written again over what was scanned.
                edit::apply_all(&tx)?;
                scan::backfill_search_keys(&tx)?;
                scan::prune(&tx, &mut pruned)?;
                tx.commit()?;
                Ok(pruned)
            })
            .await?;
        changes.merge(pruned);
        self.publish(&changes, true).await?;
        tracing::info!(
            added = changes.added.len(),
            updated = changes.updated.len(),
            deleted = changes.deleted.len(),
            "library synced"
        );
        *self.last_sync.lock().unwrap() = Some(LastSync {
            started,
            finished: now(),
            added: changes.added.len(),
            updated: changes.updated.len(),
            deleted: changes.deleted.len(),
            failed,
        });
        self.loudness_wanted.notify_one();
        Ok(changes)
    }

    /// The providers whose files are measured.
    fn measured_instances(&self) -> Vec<String> {
        self.providers
            .iter()
            .filter(|provider| provider.local_files())
            .map(|provider| provider.instance_id().to_owned())
            .collect()
    }

    /// Measures up to `albums` albums of files whose loudness is not known, decoding with
    /// `ffmpeg` what Symphonia cannot, one album at a time off the async threads.
    pub async fn measure_loudness(&self, ffmpeg: Option<PathBuf>, albums: u32) -> Result<Measured> {
        let instances = self.measured_instances();
        let due = self
            .read(move |conn| loudness::due(conn, &instances, albums))
            .await?;
        let mut measured = Measured::default();
        for album in due {
            let provider = self
                .providers
                .iter()
                .find(|provider| provider.instance_id() == album.instance)
                .cloned();
            let ffmpeg = ffmpeg.clone();
            let instance = album.instance.clone();
            let files = blocking(move || {
                let paths: Vec<Option<PathBuf>> = album
                    .files
                    .iter()
                    .map(|file| provider.as_ref()?.file(&file.item))
                    .collect();
                let found: Vec<PathBuf> = paths.iter().flatten().cloned().collect();
                let mut loudness =
                    bopsie_audio::loudness::measure_album(&found, ffmpeg.as_deref()).into_iter();
                album
                    .files
                    .into_iter()
                    .zip(paths)
                    .map(|(file, path)| (file, path.and_then(|_| loudness.next().flatten())))
                    .collect::<Vec<_>>()
            })
            .await?;
            measured.albums += 1;
            measured.files += files.len();
            measured.failed += files.iter().filter(|(_, l)| l.is_none()).count();
            self.db
                .call(move |conn| Ok(loudness::measured(conn, &instance, &files)?))
                .await?;
        }
        Ok(measured)
    }

    /// Woken after a sync, which may leave files to measure, for the measuring job to wait on.
    pub fn loudness_wanted(&self) -> Arc<tokio::sync::Notify> {
        self.loudness_wanted.clone()
    }

    /// The measuring job says it started or stopped.
    pub fn measuring(&self, running: bool) {
        self.measuring
            .store(running, std::sync::atomic::Ordering::Relaxed);
    }

    /// How far the library's files' loudness is known, and whether the job is measuring.
    pub async fn loudness_status(&self) -> Result<(bool, loudness::LoudnessStatus)> {
        let instances = self.measured_instances();
        let status = self
            .read(move |conn| loudness::status(conn, &instances))
            .await?;
        Ok((
            self.measuring.load(std::sync::atomic::Ordering::Relaxed),
            status,
        ))
    }

    /// Edits item `id`: each field of `changes` is set, or with `null` reset to what was
    /// scanned. See [`edit`] for the fields of each kind of item. Announces what changed.
    pub async fn edit(
        &self,
        media_type: MediaType,
        id: i64,
        changes: serde_json::Map<String, serde_json::Value>,
    ) -> Result<()> {
        let id = self.resolved(media_type, id).await?;
        let (touched, outcome) = self
            .db
            .call(move |conn| {
                let tx = conn.transaction()?;
                let mut touched = Changes::default();
                let outcome = match edit::edit(&tx, media_type, id, &changes, now(), &mut touched) {
                    Ok(outcome) => outcome,
                    Err(edit::Failure::Refused(error)) => return Ok(Err(error)),
                    Err(edit::Failure::Db(error)) => return Err(error.into()),
                };
                if outcome.rescan {
                    // Its file is read again at the next sync, which brings back the scan's
                    // values for what is no longer edited.
                    tx.execute(
                        "UPDATE provider_mappings SET stamp = NULL WHERE media_type = ?1 AND item_id = ?2",
                        rusqlite::params![media_type.as_str(), id],
                    )?;
                }
                // An album or artist an edit left without tracks goes, as after a sync.
                scan::prune(&tx, &mut touched)?;
                tx.commit()?;
                Ok(Ok((touched, outcome)))
            })
            .await??;
        self.publish(&touched, false).await?;
        if outcome.rescan {
            let library = self.clone();
            tokio::spawn(async move {
                if let Err(error) = library.sync().await {
                    tracing::warn!(%error, "cannot read an item's file again");
                }
            });
        }
        Ok(())
    }

    /// Keeps uploaded artwork in the artwork cache, under its content's hash as embedded art
    /// is, and answers with the proxy id an edit's `image` names it by.
    pub async fn store_artwork(&self, data: Vec<u8>, extension: &'static str) -> Result<String> {
        let artwork_dir = self.artwork_dir.clone();
        let image = blocking(move || {
            prepare_image(
                ARTWORK_CACHE,
                &artwork_dir,
                Some(ScannedImage::Embedded {
                    data,
                    extension: extension.to_owned(),
                }),
            )
        })
        .await?
        .ok_or_else(Error::unexpected)?;
        self.db
            .call(move |conn| {
                Ok(scan::store_image(conn, Some(&image))?.expect("an image was given"))
            })
            .await
            .map_err(Error::from)
    }

    /// The fields of item `id` that were edited by hand, with their values.
    pub async fn edits(
        &self,
        media_type: MediaType,
        id: i64,
    ) -> Result<serde_json::Map<String, serde_json::Value>> {
        let id = self.resolved(media_type, id).await?;
        self.db
            .call(move |conn| Ok(edit::stored(conn, media_type, id)?.edits))
            .await
            .map_err(Error::from)
    }

    /// Items that may be the same, awaiting a person; with those decided too when `all`.
    pub async fn suggestions(&self, all: bool) -> Result<Vec<reconcile::Suggestion>> {
        self.read(move |conn| reconcile::suggestions(conn, all))
            .await
    }

    /// Decides suggestion `id`: [`reconcile::MERGE`] merges the two at once,
    /// [`reconcile::APART`] keeps them apart. Either holds at every later sync.
    pub async fn decide(&self, id: i64, decision: String) -> Result<()> {
        if ![reconcile::MERGE, reconcile::APART].contains(&decision.as_str()) {
            return Err(Error::invalid_data(format!("No decision {decision}")));
        }
        let _syncing = self.syncing.lock().await;
        let changes = self
            .db
            .call(move |conn| {
                let tx = conn.transaction()?;
                let updated = tx.execute(
                    "UPDATE match_suggestions SET decision = ?2 WHERE id = ?1",
                    rusqlite::params![id, decision],
                )?;
                let mut changes = Changes::default();
                if updated == 0 {
                    return Ok(None);
                }
                reconcile::reconcile(&tx, now(), &mut changes)?;
                edit::apply_all(&tx)?;
                scan::prune(&tx, &mut changes)?;
                tx.commit()?;
                Ok(Some(changes))
            })
            .await?
            .ok_or_else(|| {
                Error::new(
                    bopsie_core::ErrorCode::MediaNotFound,
                    format!("No suggestion {id}"),
                )
            })?;
        self.publish(&changes, false).await
    }

    /// Forgets what a person decided of suggestion `id`, merging or keeping apart, and compares
    /// the two again at once: the same release merges, which undoes a Separate, and one that
    /// may be is suggested again.
    pub async fn reconsider(&self, id: i64) -> Result<()> {
        let _syncing = self.syncing.lock().await;
        let changes = self
            .db
            .call(move |conn| {
                let tx = conn.transaction()?;
                let deleted = tx.execute(
                    "DELETE FROM match_suggestions WHERE id = ?1 AND decision IS NOT NULL",
                    [id],
                )?;
                if deleted == 0 {
                    return Ok(None);
                }
                let mut changes = Changes::default();
                reconcile::reconcile(&tx, now(), &mut changes)?;
                edit::apply_all(&tx)?;
                scan::prune(&tx, &mut changes)?;
                tx.commit()?;
                Ok(Some(changes))
            })
            .await?
            .ok_or_else(|| {
                Error::new(
                    bopsie_core::ErrorCode::MediaNotFound,
                    format!("No decision {id}"),
                )
            })?;
        self.publish(&changes, false).await
    }

    /// Forgets what a person decided of the release suggested for album `id`, and what an
    /// accepted one gave it, its and its tracks' ids and its genres, and looks the album up
    /// again now.
    pub async fn reconsider_release(&self, id: i64) -> Result<()> {
        let id = self.resolved(MediaType::Album, id).await?;
        let deleted = self
            .db
            .call(move |conn| {
                let tx = conn.transaction()?;
                let deleted = tx.execute(
                    "DELETE FROM release_suggestions WHERE album_id = ?1 AND decision IS NOT NULL",
                    [id],
                )?;
                if deleted == 0 {
                    return Ok(0);
                }
                tx.execute(
                    "DELETE FROM external_ids WHERE provider_instance = ?2
                         AND ((media_type = 'album' AND item_id = ?1)
                             OR (media_type = 'track'
                                 AND item_id IN (SELECT id FROM tracks WHERE album_id = ?1)))",
                    rusqlite::params![id, enrich::MUSICBRAINZ],
                )?;
                tx.execute(
                    "DELETE FROM item_genres
                     WHERE media_type = 'album' AND item_id = ?1 AND provider_instance = ?2",
                    rusqlite::params![id, enrich::MUSICBRAINZ],
                )?;
                tx.execute(
                    "UPDATE albums SET metadata_checked = NULL WHERE id = ?1",
                    [id],
                )?;
                tx.commit()?;
                Ok(deleted)
            })
            .await?;
        if deleted == 0 {
            return Err(Error::new(
                bopsie_core::ErrorCode::MediaNotFound,
                format!("No decision on a release for album {id}"),
            ));
        }
        self.metadata_wanted.notify_one();
        Ok(())
    }

    /// Takes provider `instance` back out of album `id`, which then stays apart from what is
    /// left. Returns the album it now has, if it shared the album with another provider.
    pub async fn separate(&self, id: i64, instance: String) -> Result<Option<i64>> {
        let id = self.resolved(MediaType::Album, id).await?;
        let separates = self
            .providers
            .iter()
            .find(|provider| provider.instance_id() == instance)
            .is_none_or(|provider| provider.separate_albums());
        let _syncing = self.syncing.lock().await;
        let (album, changes) = self
            .db
            .call(move |conn| {
                let tx = conn.transaction()?;
                let mut changes = Changes::default();
                let album =
                    reconcile::separate(&tx, id, &instance, separates, now(), &mut changes)?;
                tx.commit()?;
                Ok((album, changes))
            })
            .await?;
        self.publish(&changes, false).await?;
        Ok(album)
    }

    /// Has album `id` looked up on a metadata service again now, as if never asked about.
    pub async fn refresh_metadata(&self, id: i64) -> Result<()> {
        let id = self.resolved(MediaType::Album, id).await?;
        let updated = self
            .db
            .call(move |conn| {
                Ok(conn.execute(
                    "UPDATE albums SET metadata_checked = NULL WHERE id = ?1",
                    [id],
                )?)
            })
            .await?;
        if updated == 0 {
            return Err(Error::new(
                bopsie_core::ErrorCode::MediaNotFound,
                format!("No album {id}"),
            ));
        }
        self.metadata_wanted.notify_one();
        Ok(())
    }

    /// Woken when an album should be looked up now, for the metadata job to wait on.
    pub fn metadata_wanted(&self) -> Arc<tokio::sync::Notify> {
        self.metadata_wanted.clone()
    }

    /// The metadata job says it started or finished a run.
    pub fn metadata_ran(&self, finished: Option<MetadataRunSummary>) {
        let mut run = self.metadata_run.lock().unwrap();
        run.running = finished.is_none();
        if finished.is_some() {
            run.last = finished;
        }
    }

    /// How looking albums up goes: the job, and how far the library is.
    pub async fn metadata_status(&self) -> Result<(MetadataRun, MetadataStatus)> {
        let run = self.metadata_run.lock().unwrap().clone();
        let status = self
            .read(move |conn| {
                conn.query_row(
                    "SELECT count(*),
                         count(metadata_checked),
                         (SELECT count(DISTINCT item_id) FROM external_ids
                          WHERE media_type = 'album' AND provider_instance = ?1),
                         (SELECT count(*) FROM release_suggestions WHERE decision IS NULL)
                     FROM albums",
                    [enrich::MUSICBRAINZ],
                    |row| {
                        Ok(MetadataStatus {
                            albums: row.get(0)?,
                            checked: row.get(1)?,
                            identified: row.get(2)?,
                            suggested: row.get(3)?,
                        })
                    },
                )
            })
            .await?;
        Ok((run, status))
    }

    /// Albums a metadata service was not asked about since `before`, those with files first.
    pub async fn albums_due(&self, before: i64, limit: usize) -> Result<Vec<i64>> {
        self.read(move |conn| enrich::due(conn, before, limit))
            .await
    }

    /// What is known of album `id` to find it on a metadata service by.
    pub async fn album_to_match(&self, id: i64) -> Result<Option<enrich::AlbumToMatch>> {
        self.read(move |conn| enrich::album_to_match(conn, id))
            .await
    }

    /// The release a person decided on for album `id`, and their decision.
    pub async fn release_decision(&self, id: i64) -> Result<Option<(String, String)>> {
        self.read(move |conn| enrich::release_decision(conn, id))
            .await
    }

    /// Records that album `id` was asked about now, and what it looks like but was not certain
    /// enough to take, if anything.
    pub async fn album_checked(
        &self,
        id: i64,
        suggestion: Option<enrich::SuggestedRelease>,
    ) -> Result<()> {
        self.db
            .call(move |conn| {
                let tx = conn.transaction()?;
                if let Some(release) = suggestion {
                    enrich::suggest_release(&tx, id, &release, now())?;
                }
                enrich::checked(&tx, id, now())?;
                tx.commit()?;
                Ok(())
            })
            .await
            .map_err(Error::from)
    }

    /// Keeps what album `id` was identified as, then merges what its new ids tie it to.
    pub async fn identify(&self, id: i64, identified: enrich::Identified) -> Result<()> {
        let _syncing = self.syncing.lock().await;
        let changes = self
            .db
            .call(move |conn| {
                let tx = conn.transaction()?;
                let mut changes = Changes::default();
                enrich::identify(&tx, id, &identified, now(), &mut changes)?;
                reconcile::reconcile(&tx, now(), &mut changes)?;
                edit::apply_all(&tx)?;
                scan::prune(&tx, &mut changes)?;
                tx.commit()?;
                Ok(changes)
            })
            .await?;
        self.publish(&changes, false).await
    }

    /// Tracks kept as more than one file, as one in an album's folder and a copy in a
    /// playlist's: each track with its files, for a person to clear out the ones they do not
    /// want. Files are never removed here.
    pub async fn duplicate_files(&self) -> Result<Vec<(Track, Vec<PathBuf>)>> {
        let held: Vec<(i64, String, String)> = self
            .read(|conn| {
                let mut statement = conn.prepare(
                    "SELECT item_id, provider_instance, provider_item_id FROM provider_mappings
                     WHERE media_type = 'track' AND (item_id, provider_instance) IN (
                         SELECT item_id, provider_instance FROM provider_mappings
                         WHERE media_type = 'track'
                         GROUP BY item_id, provider_instance HAVING count(*) > 1)
                     ORDER BY item_id, provider_item_id",
                )?;
                statement
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
                    .collect()
            })
            .await?;
        let providers = self.providers.clone();
        let files: Vec<(i64, Vec<PathBuf>)> = blocking(move || {
            let mut files: Vec<(i64, Vec<PathBuf>)> = Vec::new();
            for (track, instance, item) in held {
                let Some(path) = providers
                    .iter()
                    .find(|p| p.instance_id() == instance)
                    .and_then(|provider| provider.file(&item))
                else {
                    continue;
                };
                match files.last_mut() {
                    Some((last, paths)) if *last == track => paths.push(path),
                    _ => files.push((track, vec![path])),
                }
            }
            files.retain(|(_, paths)| paths.len() > 1);
            files
        })
        .await?;
        let mut out = Vec::with_capacity(files.len());
        for (id, paths) in files {
            if let Some(MediaItem::Track(track)) = self.item(MediaType::Track, id).await? {
                out.push((*track, paths));
            }
        }
        Ok(out)
    }

    /// The files album `id`'s tracks play from, by track, with each file's stamp.
    pub async fn track_files(&self, id: i64) -> Result<Vec<(i64, PathBuf, String)>> {
        let mappings: Vec<(i64, String, String, Option<String>)> = self
            .read(move |conn| {
                let mut statement = conn.prepare(
                    "SELECT tracks.id, provider_instance, provider_item_id, stamp
                     FROM tracks JOIN provider_mappings
                         ON media_type = 'track' AND item_id = tracks.id
                     WHERE album_id = ?1 ORDER BY tracks.id, priority DESC",
                )?;
                statement
                    .query_map([id], |row| {
                        Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                    })?
                    .collect()
            })
            .await?;
        let providers = self.providers.clone();
        blocking(move || {
            let mut files: Vec<(i64, PathBuf, String)> = Vec::new();
            for (track, instance, item, stamp) in mappings {
                if files.last().is_some_and(|(last, _, _)| *last == track) {
                    continue;
                }
                let file = providers
                    .iter()
                    .find(|p| p.instance_id() == instance)
                    .and_then(|provider| provider.file(&item));
                if let Some(file) = file {
                    files.push((track, file, stamp.unwrap_or_default()));
                }
            }
            files
        })
        .await
    }

    /// Artists whose images were not looked for since `before`, those on albums first.
    pub async fn artists_due(&self, before: i64, limit: usize) -> Result<Vec<i64>> {
        self.read(move |conn| enrich::artists_due(conn, before, limit))
            .await
    }

    /// What is known of artist `id` to find its images by.
    pub async fn artist_to_illustrate(
        &self,
        id: i64,
    ) -> Result<Option<enrich::ArtistToIllustrate>> {
        self.read(move |conn| enrich::artist_to_illustrate(conn, id))
            .await
    }

    /// Adds ids a metadata service links artist `id` to, such as its Spotify id.
    pub async fn link_artist(&self, id: i64, ids: Vec<(ids::IdKind, String)>) -> Result<()> {
        self.db
            .call(move |conn| Ok(enrich::link_artist(conn, id, &ids)?))
            .await
            .map_err(Error::from)
    }

    /// Records that artist `id`'s images were looked for now.
    pub async fn artist_checked(&self, id: i64) -> Result<()> {
        self.db
            .call(move |conn| Ok(enrich::artist_checked(conn, id, now())?))
            .await
            .map_err(Error::from)
    }

    /// Keeps `images`, each a kind (`thumb`, `fanart`, `logo`, `banner`) and the image, as what
    /// `source` has of item `id`, in place of what it had. An image is a URL a provider fetches,
    /// as Spotify's are, or bytes kept with the library's artwork.
    pub async fn set_item_images(
        &self,
        media_type: MediaType,
        id: i64,
        source: String,
        images: Vec<(String, ItemImage)>,
    ) -> Result<()> {
        let artwork_dir = self.artwork_dir.clone();
        let stored: Vec<(String, ImageRef)> = blocking(move || {
            images
                .into_iter()
                .filter_map(|(kind, image)| {
                    let image = match image {
                        ItemImage::Url { provider, url } => Some(ImageRef {
                            provider,
                            path: url,
                        }),
                        ItemImage::Data { bytes, extension } => prepare_image(
                            ARTWORK_CACHE,
                            &artwork_dir,
                            Some(ScannedImage::Embedded {
                                data: bytes,
                                extension: extension.to_owned(),
                            }),
                        ),
                    }?;
                    Some((kind, image))
                })
                .collect()
        })
        .await?;
        let changes = self
            .db
            .call(move |conn| {
                let tx = conn.transaction()?;
                let mut proxied = Vec::with_capacity(stored.len());
                for (kind, image) in &stored {
                    if let Some(proxy_id) = scan::store_image(&tx, Some(image))? {
                        proxied.push((kind.clone(), proxy_id));
                    }
                }
                enrich::set_item_images(&tx, media_type, id, &source, &proxied)?;
                tx.commit()?;
                let mut changes = Changes::default();
                changes.update((media_type, id));
                Ok(changes)
            })
            .await?;
        self.publish(&changes, false).await
    }

    /// Gives album `id` cover art from a metadata service, unless it has art of its own.
    pub async fn set_cover_art(
        &self,
        id: i64,
        data: Vec<u8>,
        extension: &'static str,
    ) -> Result<()> {
        let artwork_dir = self.artwork_dir.clone();
        let Some(mut image) = blocking(move || {
            prepare_image(
                ARTWORK_CACHE,
                &artwork_dir,
                Some(ScannedImage::Embedded {
                    data,
                    extension: extension.to_owned(),
                }),
            )
        })
        .await?
        else {
            return Ok(());
        };
        image.provider = scan::COVER_ART.to_owned();
        let changes = self
            .db
            .call(move |conn| {
                let tx = conn.transaction()?;
                let proxy_id = scan::store_image(&tx, Some(&image))?;
                let set = tx.execute(
                    "UPDATE albums SET image = ?2
                     WHERE id = ?1 AND (image IS NULL
                         OR image IN (SELECT proxy_id FROM images WHERE provider = ?3))",
                    rusqlite::params![id, proxy_id, scan::COVER_ART],
                )?;
                tx.commit()?;
                let mut changes = Changes::default();
                if set > 0 {
                    changes.update((MediaType::Album, id));
                }
                Ok(changes)
            })
            .await?;
        self.publish(&changes, false).await
    }

    /// Releases suggested for albums; with decided ones too when `all`.
    pub async fn release_suggestions(&self, all: bool) -> Result<Vec<enrich::ReleaseSuggestion>> {
        self.read(move |conn| enrich::release_suggestions(conn, all))
            .await
    }

    /// Accepts or rejects the release suggested for album `id`. An accepted one is taken the
    /// next time metadata is looked up.
    pub async fn decide_release(&self, id: i64, decision: String) -> Result<()> {
        if ![enrich::ACCEPT, enrich::REJECT].contains(&decision.as_str()) {
            return Err(Error::invalid_data(format!("No decision {decision}")));
        }
        self.db
            .call(move |conn| Ok(enrich::decide_release(conn, id, &decision)?))
            .await?
            .map(|_| ())
            .ok_or_else(|| {
                Error::new(
                    bopsie_core::ErrorCode::MediaNotFound,
                    format!("No release suggested for album {id}"),
                )
            })
    }

    /// Whether a sync is running now.
    pub fn syncing(&self) -> bool {
        self.syncing.try_lock().is_err()
    }

    /// How the last sync since the server started went, if one finished.
    pub fn last_sync(&self) -> Option<LastSync> {
        self.last_sync.lock().unwrap().clone()
    }

    /// Each provider's instance id.
    pub fn providers(&self) -> Vec<String> {
        self.providers
            .iter()
            .map(|provider| provider.instance_id().to_owned())
            .collect()
    }

    /// Each provider's instance id, domain and name.
    pub fn provider_names(&self) -> Vec<(String, String, String)> {
        self.providers
            .iter()
            .map(|provider| {
                (
                    provider.instance_id().to_owned(),
                    provider.domain().to_owned(),
                    provider.name().to_owned(),
                )
            })
            .collect()
    }

    async fn scan(&self, provider: Arc<dyn MusicProvider>, force: bool) -> Result<Changes> {
        let source = Source {
            domain: provider.domain(),
            instance: provider.instance_id().to_owned(),
            separate_albums: provider.separate_albums(),
            priority: provider.priority(),
        };
        let lister = provider.clone();
        let listing = blocking(move || lister.list()).await?.map_err(|error| {
            Error::new(
                bopsie_core::ErrorCode::ProviderUnavailable,
                error.to_string(),
            )
        })?;
        let mut entries = listing.entries.clone();
        // Items get ids in a reproducible order.
        entries.sort_by(|a, b| a.id.cmp(&b.id));

        let instance = source.instance.clone();
        let known: HashMap<(String, String), Option<String>> = self
            .db
            .call(move |conn| {
                let mut statement = conn.prepare(
                    "SELECT media_type, provider_item_id, stamp FROM provider_mappings
                     WHERE provider_instance = ?1 AND media_type IN ('track', 'playlist')",
                )?;
                let rows = statement
                    .query_map([&instance], |row| {
                        Ok(((row.get(0)?, row.get(1)?), row.get(2)?))
                    })?
                    .collect::<rusqlite::Result<_>>()?;
                Ok(rows)
            })
            .await?;

        let kind_name = |kind: EntryKind| match kind {
            EntryKind::Track => "track",
            EntryKind::Playlist => "playlist",
        };
        let stale = |kind: EntryKind| {
            entries
                .iter()
                .filter(move |entry| entry.kind == kind)
                .filter(|entry| {
                    known.get(&(kind_name(entry.kind).to_owned(), entry.id.clone()))
                        != Some(&Some(entry.stamp.clone()))
                })
                .cloned()
                .collect::<Vec<_>>()
        };
        let stale_tracks = stale(EntryKind::Track);
        let stale_playlists = stale(EntryKind::Playlist);
        let listed: std::collections::HashSet<(String, String)> = entries
            .iter()
            .map(|entry| (kind_name(entry.kind).to_owned(), entry.id.clone()))
            .collect();
        let known_tracks = known.keys().filter(|key| key.0 == "track").count();
        if entries.is_empty() && known_tracks > 0 && !force {
            return Err(Error::new(
                bopsie_core::ErrorCode::ProviderUnavailable,
                format!(
                    "it lists nothing, but {known_tracks} tracks were there; is it mounted? \
                     Its tracks are kept until a forced sync"
                ),
            ));
        }
        let gone = |kind: &str| -> Vec<String> {
            known
                .keys()
                .filter(|key| key.0 == kind && !listed.contains(*key))
                .filter(|key| !listing.hidden(&key.1))
                .map(|key| key.1.clone())
                .collect()
        };
        let gone_tracks = gone("track");
        let gone_playlists = gone("playlist");

        let mut changes = Changes::default();
        for batch in stale_tracks.chunks(BATCH) {
            let batch = batch.to_vec();
            let reader = provider.clone();
            let artwork_dir = self.artwork_dir.clone();
            let instance = source.instance.clone();
            let prepared = blocking(move || {
                batch
                    .into_iter()
                    .filter_map(|entry| match reader.read_track(&entry.id) {
                        Ok(track) => {
                            let image = prepare_image(&instance, &artwork_dir, track.image.clone());
                            Some((entry, PreparedTrack { track, image }))
                        }
                        Err(error) => {
                            tracing::warn!(item = entry.id, %error, "cannot read track");
                            None
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .await?;
            let source = source.clone();
            let batch_changes = self
                .db
                .call(move |conn| {
                    let tx = conn.transaction()?;
                    let mut changes = Changes::default();
                    let now = now();
                    for (entry, track) in &prepared {
                        scan::upsert_track(
                            &tx,
                            &source,
                            &entry.id,
                            &entry.stamp,
                            track,
                            now,
                            &mut changes,
                        )?;
                    }
                    tx.commit()?;
                    Ok(changes)
                })
                .await?;
            changes.merge(batch_changes);
        }

        let reader = provider.clone();
        let (instance, artwork_dir) = (source.instance.clone(), self.artwork_dir.clone());
        let playlists = blocking(move || {
            stale_playlists
                .into_iter()
                .filter_map(|entry| match reader.read_playlist(&entry.id) {
                    Ok(playlist) => {
                        let image = prepare_image(&instance, &artwork_dir, playlist.image.clone());
                        Some((entry, playlist, image))
                    }
                    Err(error) => {
                        tracing::warn!(item = entry.id, %error, "cannot read playlist");
                        None
                    }
                })
                .collect::<Vec<_>>()
        })
        .await?;
        let removed = self
            .db
            .call(move |conn| {
                let tx = conn.transaction()?;
                let mut changes = Changes::default();
                let now = now();
                scan::remove(&tx, &source, MediaType::Track, &gone_tracks, &mut changes)?;
                scan::remove(
                    &tx,
                    &source,
                    MediaType::Playlist,
                    &gone_playlists,
                    &mut changes,
                )?;
                for (entry, playlist, image) in &playlists {
                    scan::upsert_playlist(
                        &tx,
                        &source,
                        entry,
                        playlist,
                        image.as_ref(),
                        now,
                        &mut changes,
                    )?;
                }
                tx.commit()?;
                Ok(changes)
            })
            .await?;
        changes.merge(removed);
        Ok(changes)
    }

    /// Announces `changes`, and that a sync completed when `synced`.
    async fn publish(&self, changes: &Changes, synced: bool) -> Result<()> {
        let mut events = Vec::new();
        // Changed items are read in batches: a large sync changes thousands, which read one at
        // a time would queue in front of everything else that reads the library, and read at
        // once would hold it for seconds.
        let changed: Vec<(EventType, MediaType, i64)> = [
            (EventType::MediaItemAdded, &changes.added),
            (EventType::MediaItemUpdated, &changes.updated),
        ]
        .into_iter()
        .flat_map(|(event, items)| items.iter().map(move |&(t, id)| (event, t, id)))
        .collect();
        let mut items = Vec::with_capacity(changed.len());
        for batch in changed.chunks(PUBLISH_BATCH) {
            let batch = batch.to_vec();
            items.extend(
                self.read(move |conn| {
                    batch
                        .into_iter()
                        .map(|(event, media_type, id)| {
                            let resolved = query::resolve(conn, media_type, id)?;
                            let item = query::item(conn, media_type, resolved)?;
                            Ok((event, media_type, id, item))
                        })
                        .collect::<rusqlite::Result<Vec<_>>>()
                })
                .await?,
            );
        }
        for (event, media_type, id, item) in items {
            if let Some(item) = item {
                events.push(Event {
                    event,
                    object_id: Some(library_uri(media_type, id)),
                    data: serde_json::to_value(item).map_err(|_| Error::unexpected())?,
                });
            }
        }
        // A deleted item cannot be read back, so its event names it by URI only.
        for &(media_type, id) in &changes.deleted {
            events.push(Event {
                event: EventType::MediaItemDeleted,
                object_id: Some(library_uri(media_type, id)),
                data: serde_json::Value::Null,
            });
        }
        for event in events {
            self.events.publish(event);
        }
        if synced {
            self.events.publish(Event {
                event: EventType::MusicSyncCompleted,
                object_id: None,
                data: serde_json::Value::Null,
            });
        }
        Ok(())
    }
}

/// Where the library keeps a scanned image: provider files stay in the provider, embedded art
/// is written to the artwork cache under its content hash.
fn prepare_image(
    instance: &str,
    artwork_dir: &std::path::Path,
    image: Option<ScannedImage>,
) -> Option<ImageRef> {
    match image? {
        ScannedImage::Path(path) => Some(ImageRef {
            provider: instance.to_owned(),
            path,
        }),
        ScannedImage::Embedded { data, extension } => {
            let name = format!("{}.{extension}", hex::encode(Sha256::digest(&data)));
            let path = artwork_dir.join(&name);
            if !path.exists()
                && let Err(error) =
                    std::fs::create_dir_all(artwork_dir).and_then(|()| std::fs::write(&path, &data))
            {
                tracing::warn!(%error, "cannot store embedded art");
                return None;
            }
            Some(ImageRef {
                provider: ARTWORK_CACHE.to_owned(),
                path: name,
            })
        }
    }
}
