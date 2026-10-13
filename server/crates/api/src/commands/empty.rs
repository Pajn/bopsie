//! Commands a client sends as soon as it signs in, answered as a server with an empty library
//! and no players truthfully would. Each gives way to the real command when the library, the
//! players or the queues arrive.

use bopsie_core::Result;
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
    /// Only items whose name matches.
    #[serde(default)]
    pub search: Option<String>,
    /// Page size. Defaults to 500.
    #[serde(default)]
    pub limit: Option<u32>,
    /// Items to skip.
    #[serde(default)]
    pub offset: Option<u32>,
    /// Sort order, such as `sort_name`.
    #[serde(default)]
    pub order_by: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CountArgs {
    /// Only favorites.
    #[serde(default)]
    pub favorite: Option<bool>,
}

macro_rules! library_commands {
    ($($items:ident, $count:ident, $kind:literal;)*) => {$(
        pub struct $items;
        impl Command for $items {
            const NAME: &'static str = concat!("music/", $kind, "/library_items");
            const ACCESS: Access = Access::Scope(Scope::LibraryRead);
            const SUMMARY: &'static str = concat!("A page of the library's ", $kind, ".");
            type Args = LibraryItemsArgs;
            type Output = Vec<Value>;

            async fn handle(_: Context, _: LibraryItemsArgs) -> Result<Vec<Value>> {
                Ok(Vec::new())
            }
        }

        pub struct $count;
        impl Command for $count {
            const NAME: &'static str = concat!("music/", $kind, "/count");
            const ACCESS: Access = Access::Scope(Scope::LibraryRead);
            const SUMMARY: &'static str = concat!("How many ", $kind, " the library holds.");
            type Args = CountArgs;
            type Output = u32;

            async fn handle(_: Context, _: CountArgs) -> Result<u32> {
                Ok(0)
            }
        }
    )*};
}

library_commands! {
    ArtistItems, ArtistCount, "artists";
    AlbumItems, AlbumCount, "albums";
    TrackItems, TrackCount, "tracks";
    PlaylistItems, PlaylistCount, "playlists";
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchArgs {
    pub search_query: String,
    /// Which media types to search.
    #[serde(default)]
    pub media_types: Option<Vec<String>>,
    /// Results per media type.
    #[serde(default)]
    pub limit: Option<u32>,
}

/// Music Assistant's `SearchResults`.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct SearchResults {
    pub artists: Vec<Value>,
    pub albums: Vec<Value>,
    pub tracks: Vec<Value>,
    pub playlists: Vec<Value>,
    pub radio: Vec<Value>,
    pub audiobooks: Vec<Value>,
    pub podcasts: Vec<Value>,
}

pub struct Search;
impl Command for Search {
    const NAME: &'static str = "music/search";
    const ACCESS: Access = Access::Scope(Scope::LibraryRead);
    const SUMMARY: &'static str = "Search the library.";
    type Args = SearchArgs;
    type Output = SearchResults;

    async fn handle(_: Context, _: SearchArgs) -> Result<SearchResults> {
        Ok(SearchResults::default())
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AllPlayersArgs {
    /// Include players that are not reachable.
    #[serde(default)]
    pub return_unavailable: Option<bool>,
    /// Include players the user disabled.
    #[serde(default)]
    pub return_disabled: Option<bool>,
}

pub struct AllPlayers;
impl Command for AllPlayers {
    const NAME: &'static str = "players/all";
    const ACCESS: Access = Access::Scope(Scope::PlayersRead);
    const SUMMARY: &'static str = "Every player.";
    type Args = AllPlayersArgs;
    type Output = Vec<Value>;

    async fn handle(_: Context, _: AllPlayersArgs) -> Result<Vec<Value>> {
        Ok(Vec::new())
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetQueueArgs {
    pub queue_id: String,
}

pub struct GetQueue;
impl Command for GetQueue {
    const NAME: &'static str = "player_queues/get";
    const ACCESS: Access = Access::Scope(Scope::QueuesRead);
    const SUMMARY: &'static str = "A player's queue, or null when there is none.";
    type Args = GetQueueArgs;
    type Output = Option<Value>;

    async fn handle(_: Context, _: GetQueueArgs) -> Result<Option<Value>> {
        Ok(None)
    }
}
