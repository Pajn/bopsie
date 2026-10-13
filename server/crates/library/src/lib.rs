//! The SQLite library: schema and migrations, the scanner, FTS5 search, the artwork cache, and
//! the `MusicProvider` trait with its registry.

pub mod provider;
pub mod query;
pub mod scan;
mod schema;
pub mod search;

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
    /// Embedded art, stored by content hash.
    artwork_dir: PathBuf,
    /// One sync at a time.
    syncing: Arc<tokio::sync::Mutex<()>>,
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
            artwork_dir,
            syncing: Arc::default(),
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

    pub async fn playlist_tracks(
        &self,
        playlist_id: i64,
    ) -> Result<Vec<bopsie_core::media::Track>> {
        self.read(move |conn| query::playlist_tracks(conn, playlist_id))
            .await
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

    /// The file the track plays from: the first of its providers that has one.
    pub async fn track_file(&self, track_id: i64) -> Result<Option<PathBuf>> {
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
        blocking(move || {
            mappings.iter().find_map(|(instance, item)| {
                providers
                    .iter()
                    .find(|p| p.instance_id() == instance)?
                    .file(item)
            })
        })
        .await
    }

    /// The tracks a `library://` URI stands for, with the files they play from: a track
    /// itself, an album's or artist's tracks, or a playlist's, in order. Tracks without a file
    /// are left out.
    pub async fn playable(&self, uri: &str) -> Result<Vec<(Track, PathBuf)>> {
        let not_found = || {
            Error::new(
                bopsie_core::ErrorCode::MediaNotFound,
                format!("{uri} not found"),
            )
        };
        let (media_type, id) = parse_library_uri(uri).ok_or_else(not_found)?;
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
            let Ok(track_id) = track.fields.item_id.parse() else {
                continue;
            };
            if let Some(file) = self.track_file(track_id).await? {
                playable.push((track, file));
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
        let _syncing = self.syncing.lock().await;
        let mut changes = Changes::default();
        for provider in self.providers.iter() {
            match self.scan(provider.clone()).await {
                Ok(provider_changes) => changes.merge(provider_changes),
                Err(error) => tracing::warn!(
                    provider = provider.instance_id(),
                    %error,
                    "scan failed"
                ),
            }
        }
        let pruned = self
            .db
            .call(|conn| {
                let tx = conn.transaction()?;
                let mut pruned = Changes::default();
                scan::prune(&tx, &mut pruned)?;
                tx.commit()?;
                Ok(pruned)
            })
            .await?;
        changes.merge(pruned);
        self.publish(&changes).await?;
        tracing::info!(
            added = changes.added.len(),
            updated = changes.updated.len(),
            deleted = changes.deleted.len(),
            "library synced"
        );
        Ok(changes)
    }

    async fn scan(&self, provider: Arc<dyn MusicProvider>) -> Result<Changes> {
        let source = Source {
            domain: provider.domain(),
            instance: provider.instance_id().to_owned(),
        };
        let lister = provider.clone();
        let mut entries = blocking(move || lister.list()).await?.map_err(|error| {
            Error::new(
                bopsie_core::ErrorCode::ProviderUnavailable,
                error.to_string(),
            )
        })?;
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
        let gone = |kind: &str| -> Vec<String> {
            known
                .keys()
                .filter(|key| key.0 == kind && !listed.contains(*key))
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
        let playlists = blocking(move || {
            stale_playlists
                .into_iter()
                .filter_map(|entry| match reader.read_playlist(&entry.id) {
                    Ok(playlist) => Some((entry, playlist)),
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
                for (entry, playlist) in &playlists {
                    scan::upsert_playlist(
                        &tx,
                        &source,
                        &entry.id,
                        &entry.stamp,
                        playlist,
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

    async fn publish(&self, changes: &Changes) -> Result<()> {
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
        self.events.publish(Event {
            event: EventType::MusicSyncCompleted,
            object_id: None,
            data: serde_json::Value::Null,
        });
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
