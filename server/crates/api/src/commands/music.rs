//! `music/*` commands: browsing, searching and syncing the library.

use bopsie_core::media::{Album, LIBRARY, MediaItem, MediaType, Track, parse_library_uri};
use bopsie_core::{Error, ErrorCode, Result};
use std::sync::Arc;

use bopsie_library::ListQuery;
use bopsie_library::catalog::{self, Catalog};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::auth::Scope;
use crate::command::{Access, Command, Context};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LibraryItemsArgs {
    /// Only favorites.
    #[serde(default)]
    pub favorite: Option<bool>,
    /// Only items whose name contains this.
    #[serde(default)]
    pub search: Option<String>,
    /// Page size. Defaults to 500.
    #[serde(default)]
    pub limit: Option<u32>,
    /// Items to skip.
    #[serde(default)]
    pub offset: Option<u32>,
    /// Sort order: `name`, `sort_name`, `timestamp_added`, `year` for albums, `duration` for
    /// tracks, each optionally with `_desc`, or `random`. Defaults to `sort_name`.
    #[serde(default)]
    pub order_by: Option<String>,
    /// Artists only: only those credited on an album.
    #[serde(default)]
    pub album_artists_only: Option<bool>,
}

impl From<LibraryItemsArgs> for ListQuery {
    fn from(args: LibraryItemsArgs) -> Self {
        ListQuery {
            favorite: args.favorite,
            search: args.search,
            album_artists_only: args.album_artists_only.unwrap_or(false),
            limit: args.limit,
            offset: args.offset,
            order_by: args.order_by,
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CountArgs {
    /// Only favorites.
    #[serde(default)]
    pub favorite: Option<bool>,
    /// Artists only: only those credited on an album.
    #[serde(default)]
    pub album_artists_only: Option<bool>,
}

macro_rules! library_commands {
    ($($items:ident, $count:ident, $kind:literal, $media_type:expr;)*) => {$(
        pub struct $items;
        impl Command for $items {
            const NAME: &'static str = concat!("music/", $kind, "/library_items");
            const ACCESS: Access = Access::Scope(Scope::LibraryRead);
            const SUMMARY: &'static str = concat!("A page of the library's ", $kind, ".");
            type Args = LibraryItemsArgs;
            type Output = Vec<Value>;

            async fn handle(ctx: Context, args: LibraryItemsArgs) -> Result<Vec<Value>> {
                let items = ctx.api.library.list($media_type, args.into()).await?;
                items.iter().map(to_value).collect()
            }
        }

        pub struct $count;
        impl Command for $count {
            const NAME: &'static str = concat!("music/", $kind, "/count");
            const ACCESS: Access = Access::Scope(Scope::LibraryRead);
            const SUMMARY: &'static str = concat!("How many ", $kind, " the library holds.");
            type Args = CountArgs;
            type Output = u32;

            async fn handle(ctx: Context, args: CountArgs) -> Result<u32> {
                let query = ListQuery {
                    favorite: args.favorite,
                    album_artists_only: args.album_artists_only.unwrap_or(false),
                    ..ListQuery::default()
                };
                ctx.api.library.count($media_type, query).await
            }
        }
    )*};
}

library_commands! {
    ArtistItems, ArtistCount, "artists", MediaType::Artist;
    AlbumItems, AlbumCount, "albums", MediaType::Album;
    TrackItems, TrackCount, "tracks", MediaType::Track;
    PlaylistItems, PlaylistCount, "playlists", MediaType::Playlist;
}

/// The catalogs that can answer now.
fn available_catalogs(ctx: &Context) -> Vec<Arc<dyn Catalog>> {
    ctx.api
        .library
        .catalogs()
        .all()
        .into_iter()
        .filter(|catalog| catalog.available())
        .collect()
}

pub struct AudiobookItems;
impl Command for AudiobookItems {
    const NAME: &'static str = "music/audiobooks/library_items";
    const ACCESS: Access = Access::Scope(Scope::LibraryRead);
    const SUMMARY: &'static str = "A page of the audiobooks the sources serve, such as Audiobookshelf's, as they have them: the library stores none.";
    type Args = LibraryItemsArgs;
    type Output = Vec<Value>;

    async fn handle(ctx: Context, args: LibraryItemsArgs) -> Result<Vec<Value>> {
        let mut offset = args.offset.unwrap_or(0) as usize;
        let mut left = args.limit.unwrap_or(bopsie_library::query::DEFAULT_LIMIT) as usize;
        let mut books = Vec::new();
        // One source's books after another's, as one list.
        for catalog in available_catalogs(&ctx) {
            if left == 0 {
                break;
            }
            let count = catalog.audiobook_count().await?;
            if offset >= count {
                offset -= count;
                continue;
            }
            let page = catalog.audiobooks(offset, left).await?;
            offset = 0;
            left = left.saturating_sub(page.len());
            books.extend(page);
        }
        values(books)
    }
}

pub struct AudiobookCount;
impl Command for AudiobookCount {
    const NAME: &'static str = "music/audiobooks/count";
    const ACCESS: Access = Access::Scope(Scope::LibraryRead);
    const SUMMARY: &'static str = "How many audiobooks the sources serve.";
    type Args = CountArgs;
    type Output = u32;

    async fn handle(ctx: Context, _args: CountArgs) -> Result<u32> {
        let mut count = 0;
        for catalog in available_catalogs(&ctx) {
            count += catalog.audiobook_count().await?;
        }
        Ok(count as u32)
    }
}

pub(crate) fn to_value(item: impl Serialize) -> Result<Value> {
    serde_json::to_value(item).map_err(|error| {
        tracing::error!(%error, "serializing a media item");
        Error::unexpected()
    })
}

fn not_found(media_type: MediaType, item_id: &str) -> Error {
    Error::new(
        ErrorCode::MediaNotFound,
        format!("{} {item_id} not found", media_type.as_str()),
    )
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ItemArgs {
    pub item_id: String,
    /// Where the id comes from: `library`, or a catalog such as `spotify`.
    pub provider_instance_id_or_domain: String,
}

/// Where an item is: the library, by its id, or a catalog, by the catalog's id for it.
enum Target<'a> {
    Library(i64),
    Catalog(Arc<dyn Catalog>, &'a str),
}

impl ItemArgs {
    fn target(&self, ctx: &Context, media_type: MediaType) -> Result<Target<'_>> {
        if self.provider_instance_id_or_domain == LIBRARY {
            return self
                .item_id
                .parse()
                .map(Target::Library)
                .map_err(|_| not_found(media_type, &self.item_id));
        }
        ctx.api
            .library
            .catalogs()
            .get(&self.provider_instance_id_or_domain)
            .map(|catalog| Target::Catalog(catalog, &self.item_id))
            .ok_or_else(|| not_found(media_type, &self.item_id))
    }
}

async fn get(ctx: &Context, media_type: MediaType, args: &ItemArgs) -> Result<Value> {
    match args.target(ctx, media_type)? {
        Target::Library(id) => {
            let item = ctx.api.library.item(media_type, id).await?;
            to_value(item.ok_or_else(|| not_found(media_type, &args.item_id))?)
        }
        Target::Catalog(catalog, id) => to_value(catalog.item(media_type, id).await?),
    }
}

/// Fails unless the library item exists, so listing an unknown item's children is an error,
/// not empty.
async fn require(ctx: &Context, media_type: MediaType, id: i64) -> Result<i64> {
    match ctx.api.library.item(media_type, id).await? {
        Some(_) => Ok(id),
        None => Err(not_found(media_type, &id.to_string())),
    }
}

macro_rules! get_commands {
    ($($name:ident, $command:literal, $media_type:expr, $summary:literal;)*) => {$(
        pub struct $name;
        impl Command for $name {
            const NAME: &'static str = $command;
            const ACCESS: Access = Access::Scope(Scope::LibraryRead);
            const SUMMARY: &'static str = $summary;
            type Args = ItemArgs;
            type Output = Value;

            async fn handle(ctx: Context, args: ItemArgs) -> Result<Value> {
                get(&ctx, $media_type, &args).await
            }
        }
    )*};
}

get_commands! {
    GetArtist, "music/artists/get", MediaType::Artist, "An artist.";
    GetAlbum, "music/albums/get", MediaType::Album, "An album.";
    GetTrack, "music/tracks/get", MediaType::Track, "A track.";
    GetPlaylist, "music/playlists/get", MediaType::Playlist, "A playlist.";
    GetAudiobook, "music/audiobooks/get", MediaType::Audiobook, "An audiobook, from the source that serves it.";
}

fn values<T: Serialize>(items: Vec<T>) -> Result<Vec<Value>> {
    items.into_iter().map(to_value).collect()
}

pub struct AlbumTracks;
impl Command for AlbumTracks {
    const NAME: &'static str = "music/albums/album_tracks";
    const ACCESS: Access = Access::Scope(Scope::LibraryRead);
    const SUMMARY: &'static str = "An album's tracks, by disc and track number.";
    type Args = ItemArgs;
    type Output = Vec<Value>;

    async fn handle(ctx: Context, args: ItemArgs) -> Result<Vec<Value>> {
        match args.target(&ctx, MediaType::Album)? {
            Target::Library(id) => {
                let id = require(&ctx, MediaType::Album, id).await?;
                values::<Track>(ctx.api.library.album_tracks(id).await?)
            }
            Target::Catalog(catalog, id) => values(catalog.album_tracks(id).await?),
        }
    }
}

pub struct ArtistAlbums;
impl Command for ArtistAlbums {
    const NAME: &'static str = "music/artists/artist_albums";
    const ACCESS: Access = Access::Scope(Scope::LibraryRead);
    const SUMMARY: &'static str = "Albums the artist is credited on or has tracks on.";
    type Args = ItemArgs;
    type Output = Vec<Value>;

    async fn handle(ctx: Context, args: ItemArgs) -> Result<Vec<Value>> {
        match args.target(&ctx, MediaType::Artist)? {
            Target::Library(id) => {
                let id = require(&ctx, MediaType::Artist, id).await?;
                values::<Album>(ctx.api.library.artist_albums(id).await?)
            }
            Target::Catalog(catalog, id) => values(catalog.artist_albums(id).await?),
        }
    }
}

pub struct ArtistTracks;
impl Command for ArtistTracks {
    const NAME: &'static str = "music/artists/artist_tracks";
    const ACCESS: Access = Access::Scope(Scope::LibraryRead);
    const SUMMARY: &'static str = "The artist's tracks.";
    type Args = ItemArgs;
    type Output = Vec<Value>;

    async fn handle(ctx: Context, args: ItemArgs) -> Result<Vec<Value>> {
        match args.target(&ctx, MediaType::Artist)? {
            Target::Library(id) => {
                let id = require(&ctx, MediaType::Artist, id).await?;
                values::<Track>(ctx.api.library.artist_tracks(id).await?)
            }
            Target::Catalog(catalog, id) => values(catalog.artist_tracks(id).await?),
        }
    }
}

pub struct ArtistAudiobooks;
impl Command for ArtistAudiobooks {
    const NAME: &'static str = "music/artists/artist_audiobooks";
    const ACCESS: Access = Access::Scope(Scope::LibraryRead);
    const SUMMARY: &'static str = "The audiobooks an author wrote, from the source that serves them. This server's own addition.";
    type Args = ItemArgs;
    type Output = Vec<Value>;

    async fn handle(ctx: Context, args: ItemArgs) -> Result<Vec<Value>> {
        match args.target(&ctx, MediaType::Artist)? {
            // The library stores no audiobooks.
            Target::Library(id) => {
                require(&ctx, MediaType::Artist, id).await?;
                Ok(Vec::new())
            }
            Target::Catalog(catalog, id) => values(catalog.artist_audiobooks(id).await?),
        }
    }
}

pub struct PlaylistTracks;
impl Command for PlaylistTracks {
    const NAME: &'static str = "music/playlists/playlist_tracks";
    const ACCESS: Access = Access::Scope(Scope::LibraryRead);
    const SUMMARY: &'static str = "The playlist's tracks in order. Streamed in batches.";
    const PARTIAL: bool = true;
    type Args = ItemArgs;
    type Output = Vec<Value>;

    async fn handle(ctx: Context, args: ItemArgs) -> Result<Vec<Value>> {
        match args.target(&ctx, MediaType::Playlist)? {
            Target::Library(id) => {
                let id = require(&ctx, MediaType::Playlist, id).await?;
                values::<Track>(ctx.api.library.playlist_tracks(id).await?)
            }
            Target::Catalog(catalog, id) => values(catalog.playlist_tracks(id).await?),
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ItemByUriArgs {
    /// A `library://<type>/<id>` URI, or a catalog's, such as `spotify://album/<id>`.
    pub uri: String,
}

pub struct ItemByUri;
impl Command for ItemByUri {
    const NAME: &'static str = "music/item_by_uri";
    const ACCESS: Access = Access::Scope(Scope::LibraryRead);
    const SUMMARY: &'static str = "The item a URI names.";
    type Args = ItemByUriArgs;
    type Output = Value;

    async fn handle(ctx: Context, args: ItemByUriArgs) -> Result<Value> {
        let Some((media_type, id)) = parse_library_uri(&args.uri) else {
            let invalid = || {
                Error::new(
                    ErrorCode::InvalidProviderUri,
                    format!("Not a valid URI: {}", args.uri),
                )
            };
            let (scheme, media_type, id) = catalog::parse_uri(&args.uri).ok_or_else(invalid)?;
            let catalog = ctx.api.library.catalogs().get(scheme).ok_or_else(invalid)?;
            return to_value(catalog.item(media_type, id).await?);
        };
        let item = ctx.api.library.item(media_type, id).await?;
        to_value(item.ok_or_else(|| not_found(media_type, &id.to_string()))?)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchArgs {
    pub search_query: String,
    /// Media types to search: `artist`, `album`, `track`, `playlist`, `audiobook`. Defaults to
    /// all.
    #[serde(default)]
    pub media_types: Option<Vec<String>>,
    /// Results per media type, from each source. Defaults to 50.
    #[serde(default)]
    pub limit: Option<u32>,
    /// Search the library alone, not catalogs such as Spotify.
    #[serde(default)]
    pub library_only: Option<bool>,
    /// The sources to search, by provider id: `library`, or a catalog's such as `spotify`.
    /// Defaults to all. This server's own addition.
    #[serde(default)]
    pub providers: Option<Vec<String>>,
}

/// Music Assistant's `SearchResults`. Kinds of media this server does not have stay empty.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct SearchResults {
    pub artists: Vec<Value>,
    pub albums: Vec<Value>,
    pub genres: Vec<Value>,
    pub tracks: Vec<Value>,
    pub playlists: Vec<Value>,
    pub radio: Vec<Value>,
    pub audiobooks: Vec<Value>,
    pub podcasts: Vec<Value>,
    pub sound_effects: Vec<Value>,
}

pub struct Search;
impl Command for Search {
    const NAME: &'static str = "music/search";
    const ACCESS: Access = Access::Scope(Scope::LibraryRead);
    const SUMMARY: &'static str = "Search the library and catalogs such as Spotify by name: the library's results first, and a catalog's that the library holds as the library's.";
    type Args = SearchArgs;
    type Output = SearchResults;

    async fn handle(ctx: Context, args: SearchArgs) -> Result<SearchResults> {
        let limit = args.limit.unwrap_or(50);
        let wanted = |media_type: MediaType| {
            args.media_types
                .as_ref()
                .is_none_or(|types| types.iter().any(|t| t == media_type.as_str()))
        };
        let from = |provider: &str| {
            args.providers
                .as_ref()
                .is_none_or(|providers| providers.iter().any(|p| p == provider))
        };
        let catalogs: Vec<_> = if args.library_only == Some(true) {
            Vec::new()
        } else {
            ctx.api
                .library
                .catalogs()
                .all()
                .into_iter()
                .filter(|c| c.available() && (from(c.instance_id()) || from(c.domain())))
                .collect()
        };
        let mut results = SearchResults::default();
        for (media_type, slot) in [
            (MediaType::Artist, &mut results.artists),
            (MediaType::Album, &mut results.albums),
            (MediaType::Track, &mut results.tracks),
            (MediaType::Playlist, &mut results.playlists),
            (MediaType::Audiobook, &mut results.audiobooks),
        ] {
            if !wanted(media_type) {
                continue;
            }
            let mut items: Vec<MediaItem> = if from(LIBRARY) {
                ctx.api
                    .library
                    .search(media_type, args.search_query.clone(), limit)
                    .await?
            } else {
                Vec::new()
            };
            for catalog in &catalogs {
                let found = match catalog
                    .search(&args.search_query, media_type, limit as usize)
                    .await
                {
                    Ok(found) => found,
                    // One source failing leaves the others' results.
                    Err(error) => {
                        tracing::warn!(catalog = catalog.instance_id(), %error, "search failed");
                        continue;
                    }
                };
                for item in found {
                    let held = ctx
                        .api
                        .library
                        .by_mapping(catalog.instance_id(), media_type, &item.fields().item_id)
                        .await?;
                    let item = held.unwrap_or(item);
                    if !items.iter().any(|i| i.fields().uri == item.fields().uri) {
                        items.push(item);
                    }
                }
            }
            *slot = values(items)?;
        }
        Ok(results)
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct SyncArgs {
    /// Remove the tracks of a source that lists none, which a sync otherwise keeps in case
    /// its disk is not mounted. This server's own addition.
    #[serde(default)]
    pub force: Option<bool>,
}

pub struct Sync;
impl Command for Sync {
    const NAME: &'static str = "music/sync";
    const ACCESS: Access = Access::Scope(Scope::LibraryRead);
    const SUMMARY: &'static str =
        "Rescan the library in the background. music_sync_completed follows when it is done.";
    type Args = SyncArgs;
    type Output = ();

    async fn handle(ctx: Context, args: SyncArgs) -> Result<()> {
        let library = ctx.api.library.clone();
        let force = args.force.unwrap_or(false);
        tokio::spawn(async move {
            if let Err(error) = library.sync_with(force).await {
                tracing::error!(%error, "library sync failed");
            }
        });
        Ok(())
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProvidersArgs {}

pub struct Providers;
impl Command for Providers {
    const NAME: &'static str = "providers";
    const ACCESS: Access = Access::Scope(Scope::LibraryRead);
    const SUMMARY: &'static str = "The music providers: the sources the library scans, and catalogs such as Spotify, searched and browsed as they are.";
    type Args = ProvidersArgs;
    type Output = Vec<Value>;

    async fn handle(ctx: Context, _: ProvidersArgs) -> Result<Vec<Value>> {
        let library = &ctx.api.library;
        let catalogs: Vec<(String, String, String)> = library
            .catalogs()
            .all()
            .iter()
            .filter(|c| c.available())
            .map(|c| {
                (
                    c.instance_id().to_owned(),
                    c.domain().to_owned(),
                    c.name().to_owned(),
                )
            })
            .collect();
        Ok(providers_list(&library.provider_names(), &catalogs))
    }
}

/// One entry per provider instance, by `(instance, domain, name)`: the library's sources, then
/// the catalogs. A source that is also a catalog, as Spotify's saved music is, is one provider
/// that both fills the library and streams, named as the catalog names itself.
fn providers_list(
    sources: &[(String, String, String)],
    catalogs: &[(String, String, String)],
) -> Vec<Value> {
    const LIBRARY: [&str; 4] = [
        "library_artists",
        "library_albums",
        "library_tracks",
        "library_playlists",
    ];
    const CATALOG: [&str; 2] = ["search", "browse"];
    let entry = |(instance, domain, name): &(String, String, String),
                 streaming: bool,
                 features: Vec<&str>| {
        serde_json::json!({
            "type": "music",
            "domain": domain,
            "name": name,
            "instance_id": instance,
            "lookup_key": instance,
            "available": true,
            "is_streaming_provider": streaming,
            "supported_features": features,
        })
    };
    let mut list: Vec<Value> = sources
        .iter()
        .map(|source| match catalogs.iter().find(|c| c.0 == source.0) {
            Some(catalog) => entry(
                catalog,
                true,
                LIBRARY.iter().chain(&CATALOG).copied().collect(),
            ),
            None => entry(source, false, LIBRARY.to_vec()),
        })
        .collect();
    list.extend(
        catalogs
            .iter()
            .filter(|catalog| !sources.iter().any(|s| s.0 == catalog.0))
            .map(|catalog| entry(catalog, true, CATALOG.to_vec())),
    );
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(instance: &str, domain: &str, name: &str) -> (String, String, String) {
        (instance.to_owned(), domain.to_owned(), name.to_owned())
    }

    #[test]
    fn a_source_that_is_also_a_catalog_is_one_provider_named_as_itself() {
        let list = providers_list(
            &[
                named("filesystem_local--a", "filesystem_local", "Local files"),
                named("spotify", "spotify", "Spotify"),
            ],
            &[named("spotify", "spotify", "Spotify")],
        );
        let names: Vec<(&str, &str)> = list
            .iter()
            .map(|p| {
                (
                    p["instance_id"].as_str().unwrap(),
                    p["name"].as_str().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            names,
            [
                ("filesystem_local--a", "Local files"),
                ("spotify", "Spotify")
            ]
        );
        assert_eq!(list[1]["is_streaming_provider"], true);
        assert_eq!(list[1]["supported_features"].as_array().unwrap().len(), 6);
        assert_eq!(list[0]["is_streaming_provider"], false);
    }
}
