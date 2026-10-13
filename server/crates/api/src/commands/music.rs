//! `music/*` commands: browsing, searching and syncing the library.

use bopsie_core::media::{Album, LIBRARY, MediaItem, MediaType, Track, parse_library_uri};
use bopsie_core::{Error, ErrorCode, Result};
use bopsie_library::ListQuery;
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

fn to_value(item: impl Serialize) -> Result<Value> {
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
    /// Where the id comes from. Only `library` holds items on this server.
    pub provider_instance_id_or_domain: String,
}

impl ItemArgs {
    /// The library id the arguments name.
    fn id(&self, media_type: MediaType) -> Result<i64> {
        if self.provider_instance_id_or_domain != LIBRARY {
            return Err(not_found(media_type, &self.item_id));
        }
        self.item_id
            .parse()
            .map_err(|_| not_found(media_type, &self.item_id))
    }
}

async fn get(ctx: &Context, media_type: MediaType, args: &ItemArgs) -> Result<Value> {
    let item = ctx
        .api
        .library
        .item(media_type, args.id(media_type)?)
        .await?;
    to_value(item.ok_or_else(|| not_found(media_type, &args.item_id))?)
}

/// Fails unless the item exists, so listing an unknown item's children is an error, not empty.
async fn require(ctx: &Context, media_type: MediaType, args: &ItemArgs) -> Result<i64> {
    let id = args.id(media_type)?;
    match ctx.api.library.item(media_type, id).await? {
        Some(_) => Ok(id),
        None => Err(not_found(media_type, &args.item_id)),
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
        let id = require(&ctx, MediaType::Album, &args).await?;
        values::<Track>(ctx.api.library.album_tracks(id).await?)
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
        let id = require(&ctx, MediaType::Artist, &args).await?;
        values::<Album>(ctx.api.library.artist_albums(id).await?)
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
        let id = require(&ctx, MediaType::Artist, &args).await?;
        values::<Track>(ctx.api.library.artist_tracks(id).await?)
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
        let id = require(&ctx, MediaType::Playlist, &args).await?;
        values::<Track>(ctx.api.library.playlist_tracks(id).await?)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ItemByUriArgs {
    /// A `library://<type>/<id>` URI.
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
            return Err(Error::new(
                ErrorCode::InvalidProviderUri,
                format!("Not a valid library URI: {}", args.uri),
            ));
        };
        let item = ctx.api.library.item(media_type, id).await?;
        to_value(item.ok_or_else(|| not_found(media_type, &id.to_string()))?)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchArgs {
    pub search_query: String,
    /// Media types to search: `artist`, `album`, `track`, `playlist`. Defaults to all.
    #[serde(default)]
    pub media_types: Option<Vec<String>>,
    /// Results per media type. Defaults to 50.
    #[serde(default)]
    pub limit: Option<u32>,
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
    const SUMMARY: &'static str = "Search the library by name.";
    type Args = SearchArgs;
    type Output = SearchResults;

    async fn handle(ctx: Context, args: SearchArgs) -> Result<SearchResults> {
        let limit = args.limit.unwrap_or(50);
        let wanted = |media_type: MediaType| {
            args.media_types
                .as_ref()
                .is_none_or(|types| types.iter().any(|t| t == media_type.as_str()))
        };
        let mut results = SearchResults::default();
        for (media_type, slot) in [
            (MediaType::Artist, &mut results.artists),
            (MediaType::Album, &mut results.albums),
            (MediaType::Track, &mut results.tracks),
            (MediaType::Playlist, &mut results.playlists),
        ] {
            if wanted(media_type) {
                let items: Vec<MediaItem> = ctx
                    .api
                    .library
                    .search(media_type, args.search_query.clone(), limit)
                    .await?;
                *slot = values(items)?;
            }
        }
        Ok(results)
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct SyncArgs {}

pub struct Sync;
impl Command for Sync {
    const NAME: &'static str = "music/sync";
    const ACCESS: Access = Access::Scope(Scope::LibraryRead);
    const SUMMARY: &'static str =
        "Rescan the library in the background. music_sync_completed follows when it is done.";
    type Args = SyncArgs;
    type Output = ();

    async fn handle(ctx: Context, _: SyncArgs) -> Result<()> {
        let library = ctx.api.library.clone();
        tokio::spawn(async move {
            if let Err(error) = library.sync().await {
                tracing::error!(%error, "library sync failed");
            }
        });
        Ok(())
    }
}
