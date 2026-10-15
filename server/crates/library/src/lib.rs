//! The SQLite library: schema and migrations, the scanner, FTS5 search, the artwork cache, and
//! the `MusicProvider` trait with its registry.

pub mod catalog;
pub mod edit;
pub mod ids;
pub mod matching;
pub mod provider;
pub mod query;
pub mod scan;
mod schema;
pub mod search;

use bopsie_audio::engine::Input;
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
        }
    }

    async fn read<T: Send + 'static>(
        &self,
        f: impl FnOnce(&Connection) -> rusqlite::Result<T> + Send + 'static,
    ) -> Result<T> {
        Ok(self.db.call(move |conn| Ok(f(conn)?)).await?)
    }

    pub async fn list(&self, media_type: MediaType, query: ListQuery) -> Result<Vec<MediaItem>> {
        self.read(move |conn| query::list(conn, media_type, &query))
            .await
    }

    pub async fn count(&self, media_type: MediaType, query: ListQuery) -> Result<u32> {
        self.read(move |conn| query::count(conn, media_type, &query))
            .await
    }

    pub async fn item(&self, media_type: MediaType, id: i64) -> Result<Option<MediaItem>> {
        self.read(move |conn| query::item(conn, media_type, id))
            .await
    }

    pub async fn album_tracks(&self, album_id: i64) -> Result<Vec<bopsie_core::media::Track>> {
        self.read(move |conn| query::album_tracks(conn, album_id))
            .await
    }

    pub async fn artist_albums(&self, artist_id: i64) -> Result<Vec<bopsie_core::media::Album>> {
        self.read(move |conn| query::artist_albums(conn, artist_id))
            .await
    }

    pub async fn artist_tracks(&self, artist_id: i64) -> Result<Vec<bopsie_core::media::Track>> {
        self.read(move |conn| query::artist_tracks(conn, artist_id))
            .await
    }

    /// A playlist's tracks in order. A playlist a catalog keeps, such as a Spotify one, is
    /// asked for them, so that its tracks need not be in the library.
    pub async fn playlist_tracks(
        &self,
        playlist_id: i64,
    ) -> Result<Vec<bopsie_core::media::Track>> {
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
                     WHERE media_type = ?1 AND item_id = ?2",
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
    pub async fn track_media(&self, track_id: i64) -> Result<Option<Input>> {
        let mappings: Vec<(String, String)> = self
            .read(move |conn| {
                let mut statement = conn.prepare(
                    "SELECT provider_instance, provider_item_id FROM provider_mappings
                     WHERE media_type = 'track' AND item_id = ?1",
                )?;
                statement
                    .query_map([track_id], |row| Ok((row.get(0)?, row.get(1)?)))?
                    .collect()
            })
            .await?;
        let providers = self.providers.clone();
        let catalogs = self.catalogs.clone();
        blocking(move || {
            mappings.iter().find_map(|(instance, item)| {
                match providers.iter().find(|p| p.instance_id() == instance) {
                    Some(provider) => provider.media(item),
                    None => catalogs.get(instance)?.media(item),
                }
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
    pub async fn playable(&self, uri: &str) -> Result<Vec<(Track, Input)>> {
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
                    Some((track, media))
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
        let mut playable = Vec::with_capacity(tracks.len());
        for track in tracks {
            // A catalog's playlist holds the catalog's own tracks.
            let media = match track.fields.item_id.parse() {
                Ok(track_id) if track.fields.provider == bopsie_core::media::LIBRARY => {
                    self.track_media(track_id).await?
                }
                _ => self
                    .catalogs
                    .get(&track.fields.provider)
                    .and_then(|catalog| catalog.media(&track.fields.item_id)),
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
        if image.provider == ARTWORK_CACHE {
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
        Ok(changes)
    }

    /// Edits item `id`: each field of `changes` is set, or with `null` reset to what was
    /// scanned. See [`edit`] for the fields of each kind of item. Announces what changed.
    pub async fn edit(
        &self,
        media_type: MediaType,
        id: i64,
        changes: serde_json::Map<String, serde_json::Value>,
    ) -> Result<()> {
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
        self.db
            .call(move |conn| Ok(edit::stored(conn, media_type, id)?.edits))
            .await
            .map_err(Error::from)
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

    async fn scan(&self, provider: Arc<dyn MusicProvider>, force: bool) -> Result<Changes> {
        let source = Source {
            domain: provider.domain(),
            instance: provider.instance_id().to_owned(),
            separate_albums: provider.separate_albums(),
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
        for (event, items) in [
            (EventType::MediaItemAdded, &changes.added),
            (EventType::MediaItemUpdated, &changes.updated),
        ] {
            for &(media_type, id) in items {
                if let Some(item) = self.item(media_type, id).await? {
                    events.push(Event {
                        event,
                        object_id: Some(library_uri(media_type, id)),
                        data: serde_json::to_value(item).map_err(|_| Error::unexpected())?,
                    });
                }
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
