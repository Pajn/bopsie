//! Changing a player's settings from the web UI. What is saved here is kept in the database and
//! wins over the config file; a field set to `null` clears what was saved, so the config file's
//! value, or the default, applies again. A field left out is left as it is.

use bopsie_core::{Error, ErrorCode, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::auth::Scope;
use crate::command::{Access, Command, Context};

/// Reads a field that may be absent (`None`), `null` (`Some(None)`) or a value.
fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> std::result::Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(deserializer).map(Some)
}

fn invalid(details: impl Into<String>) -> Error {
    Error::invalid_data(details)
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PlayerUpdateArgs {
    pub player_id: String,
    /// The name shown, instead of the one the player gives itself. Empty clears it.
    #[serde(default, deserialize_with = "present")]
    pub name: Option<Option<String>>,
    /// A disabled player is hidden and never played to.
    #[serde(default, deserialize_with = "present")]
    pub enabled: Option<Option<bool>>,
    /// The loudest the player may be set, 0 to 100. The server holds it there.
    #[serde(default, deserialize_with = "present")]
    pub max_volume: Option<Option<u8>>,
    /// Whether the player shows in the Spotify app as a Spotify Connect device.
    #[serde(default, deserialize_with = "present")]
    pub spotify_connect: Option<Option<bool>>,
    /// What the player is called in the Spotify app, instead of `[spotify] name`'s pattern.
    /// Empty clears it.
    #[serde(default, deserialize_with = "present")]
    pub spotify_name: Option<Option<String>>,
}

pub struct PlayerUpdate;
impl Command for PlayerUpdate {
    const NAME: &'static str = "admin/player/update";
    const ACCESS: Access = Access::Scope(Scope::Admin);
    const SUMMARY: &'static str = "Save a player's name, whether it is enabled, its maximum volume and whether it is a Spotify Connect device, over the config file.";
    type Args = PlayerUpdateArgs;
    type Output = ();

    async fn handle(ctx: Context, args: PlayerUpdateArgs) -> Result<()> {
        let devices = ctx.api.playback.devices();
        let device = devices.get(&args.player_id).ok_or_else(|| {
            Error::new(
                ErrorCode::PlayerUnavailable,
                format!("Player {} not found", args.player_id),
            )
        })?;
        let players = devices.players();
        let mut saved = players.saved_device(&device.id);
        if let Some(name) = args.name {
            saved.name = name
                .map(|name| name.trim().to_owned())
                .filter(|name| !name.is_empty());
        }
        if let Some(enabled) = args.enabled {
            saved.enabled = enabled;
        }
        if let Some(max_volume) = args.max_volume {
            if max_volume.is_some_and(|v| v > 100) {
                return Err(invalid("max_volume is 0 to 100"));
            }
            saved.max_volume = max_volume;
        }
        if let Some(spotify_connect) = args.spotify_connect {
            saved.spotify_connect = spotify_connect;
        }
        if let Some(spotify_name) = args.spotify_name {
            saved.spotify_name = spotify_name
                .map(|name| name.trim().to_owned())
                .filter(|name| !name.is_empty());
        }
        players.save_device(&device.id, saved).await?;
        // Clients hear of it as of any change to the device's endpoints.
        for member in &device.members {
            players.changed(&member.id);
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EndpointUpdateArgs {
    pub endpoint_id: String,
    /// Whether the player may play over this endpoint's protocol.
    #[serde(default, deserialize_with = "present")]
    pub enabled: Option<Option<bool>>,
    /// For speakers that ask for one. Applies at once: a speaker that refused the old one is
    /// tried again.
    #[serde(default, deserialize_with = "present")]
    pub password: Option<Option<String>>,
    /// Quirks by name, as in the config file's `[players.<id>.quirks]`; `null` clears one.
    /// They apply from the next play.
    #[serde(default)]
    pub quirks: Option<serde_json::Map<String, Value>>,
}

pub struct EndpointUpdate;
impl Command for EndpointUpdate {
    const NAME: &'static str = "admin/endpoint/update";
    const ACCESS: Access = Access::Scope(Scope::Admin);
    const SUMMARY: &'static str = "Save an endpoint's settings over the config file: whether it plays, its password, its quirks.";
    type Args = EndpointUpdateArgs;
    type Output = ();

    async fn handle(ctx: Context, args: EndpointUpdateArgs) -> Result<()> {
        let devices = ctx.api.playback.devices();
        let players = devices.players();
        if players.get(&args.endpoint_id).is_none() {
            return Err(Error::new(
                ErrorCode::PlayerUnavailable,
                format!("Endpoint {} not found", args.endpoint_id),
            ));
        }
        let mut saved = players.saved_endpoint(&args.endpoint_id);
        if let Some(enabled) = args.enabled {
            saved.enabled = enabled;
        }
        let password_changed = args.password.is_some();
        if let Some(password) = args.password {
            saved.password = password.filter(|p| !p.is_empty());
        }
        if let Some(changes) = args.quirks {
            let Value::Object(mut quirks) =
                serde_json::to_value(&saved.quirks).map_err(|_| Error::unexpected())?
            else {
                return Err(Error::unexpected());
            };
            for (name, value) in changes {
                quirks.insert(name, value);
            }
            quirks.retain(|_, value| !value.is_null());
            saved.quirks = serde_json::from_value(Value::Object(quirks))
                .map_err(|error| invalid(format!("Invalid quirks: {error}")))?;
        }
        players.save_endpoint(&args.endpoint_id, saved).await?;
        if password_changed
            && let Some(airplay) = &ctx.api.airplay
            && airplay.knows(&args.endpoint_id)
        {
            airplay.settings_changed(&args.endpoint_id)?;
        }
        Ok(())
    }
}

fn media_type(name: &str) -> Result<bopsie_core::media::MediaType> {
    bopsie_core::media::MediaType::parse(name).ok_or_else(|| {
        invalid(format!(
            "No media type {name}: artist, album, track or playlist"
        ))
    })
}

fn item_id(id: &str) -> Result<i64> {
    id.parse()
        .map_err(|_| invalid(format!("No library item {id}")))
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LibraryUpdateArgs {
    /// `artist`, `album`, `track` or `playlist`.
    pub media_type: String,
    /// The item's library id.
    pub item_id: String,
    /// Fields to set, or to reset to what the scan found with `null`. Tracks: `name`,
    /// `sort_name`, `track_number`, `disc_number`, `album` (`{"id"}` or `{"name", "artists"?}`),
    /// `artists` (a list of `{"id"}` or `{"name"}`), `image` (a proxy id from `POST
    /// /artwork`). Albums: `name`, `sort_name`, `year`, `artists`, `image`. Artists: `name`,
    /// `sort_name`. Playlists: `name`.
    pub changes: serde_json::Map<String, Value>,
}

pub struct LibraryUpdate;
impl Command for LibraryUpdate {
    const NAME: &'static str = "admin/library/update";
    const ACCESS: Access = Access::Scope(Scope::Admin);
    const SUMMARY: &'static str =
        "Edit a library item's metadata. Edits outlast a rescan of its file; null resets a field.";
    type Args = LibraryUpdateArgs;
    type Output = ();

    async fn handle(ctx: Context, args: LibraryUpdateArgs) -> Result<()> {
        ctx.api
            .library
            .edit(
                media_type(&args.media_type)?,
                item_id(&args.item_id)?,
                args.changes,
            )
            .await
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LibraryEditsArgs {
    pub media_type: String,
    pub item_id: String,
}

pub struct LibraryEdits;
impl Command for LibraryEdits {
    const NAME: &'static str = "admin/library/edits";
    const ACCESS: Access = Access::Scope(Scope::Admin);
    const SUMMARY: &'static str = "The fields of a library item edited by hand, with their values.";
    type Args = LibraryEditsArgs;
    type Output = serde_json::Map<String, Value>;

    async fn handle(
        ctx: Context,
        args: LibraryEditsArgs,
    ) -> Result<serde_json::Map<String, Value>> {
        ctx.api
            .library
            .edits(media_type(&args.media_type)?, item_id(&args.item_id)?)
            .await
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LibrarySuggestionsArgs {
    /// Also those already decided.
    #[serde(default)]
    pub all: bool,
}

/// Two items that may be the same, and what a person decided of them.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LibrarySuggestion {
    pub id: i64,
    /// `album`, for now.
    pub media_type: String,
    /// The two items, as `music/<kind>s/get` serves them.
    pub a: Value,
    pub b: Value,
    /// Why they may be the same, as a person reads it.
    pub reason: String,
    /// `merge`, `apart`, or `null` while undecided.
    pub decision: Option<String>,
}

pub struct LibrarySuggestions;
impl Command for LibrarySuggestions {
    const NAME: &'static str = "admin/library/suggestions";
    const ACCESS: Access = Access::Scope(Scope::Admin);
    const SUMMARY: &'static str =
        "Items from different providers that may be the same, for a person to merge or keep apart.";
    type Args = LibrarySuggestionsArgs;
    type Output = Vec<LibrarySuggestion>;

    async fn handle(ctx: Context, args: LibrarySuggestionsArgs) -> Result<Vec<LibrarySuggestion>> {
        let library = &ctx.api.library;
        let mut out = Vec::new();
        for suggestion in library.suggestions(args.all).await? {
            let (Some(a), Some(b)) = (
                library.item(suggestion.media_type, suggestion.a).await?,
                library.item(suggestion.media_type, suggestion.b).await?,
            ) else {
                continue;
            };
            out.push(LibrarySuggestion {
                id: suggestion.id,
                media_type: suggestion.media_type.as_str().to_owned(),
                a: super::music::to_value(a)?,
                b: super::music::to_value(b)?,
                reason: suggestion.reason,
                decision: suggestion.decision,
            });
        }
        Ok(out)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LibraryDecideArgs {
    /// The suggestion's id.
    pub id: i64,
    /// `merge` to make the two one now, `apart` to keep them apart. Either holds at every
    /// later sync.
    pub decision: String,
}

pub struct LibraryDecide;
impl Command for LibraryDecide {
    const NAME: &'static str = "admin/library/decide";
    const ACCESS: Access = Access::Scope(Scope::Admin);
    const SUMMARY: &'static str = "Merge the two items of a suggestion, or keep them apart.";
    type Args = LibraryDecideArgs;
    type Output = ();

    async fn handle(ctx: Context, args: LibraryDecideArgs) -> Result<()> {
        ctx.api.library.decide(args.id, args.decision).await
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LibrarySeparateArgs {
    /// The album's library id.
    pub item_id: String,
    /// The provider instance to take back out of it, as its `provider_mappings` name it.
    pub provider_instance: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Separated {
    /// The album the provider now has, or `null` when it did not share the album.
    pub item_id: Option<String>,
}

pub struct LibrarySeparate;
impl Command for LibrarySeparate {
    const NAME: &'static str = "admin/library/separate";
    const ACCESS: Access = Access::Scope(Scope::Admin);
    const SUMMARY: &'static str =
        "Take one provider back out of a merged album, into an album of its own kept apart.";
    type Args = LibrarySeparateArgs;
    type Output = Separated;

    async fn handle(ctx: Context, args: LibrarySeparateArgs) -> Result<Separated> {
        let album = ctx
            .api
            .library
            .separate(item_id(&args.item_id)?, args.provider_instance)
            .await?;
        Ok(Separated {
            item_id: album.map(|id| id.to_string()),
        })
    }
}

/// A MusicBrainz release an album looks like, for a person to accept or reject.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ReleaseSuggestion {
    /// The album, as `music/albums/get` serves it.
    pub album: Value,
    pub release_id: String,
    /// The release as a person reads it: "Artist – Title (year, country, format)".
    pub title: String,
    /// Its page on MusicBrainz.
    pub url: String,
    /// How far it is from the album: 0 is the same, 1 nothing alike.
    pub distance: f64,
    /// `accept`, `reject`, or `null` while undecided.
    pub decision: Option<String>,
}

pub struct ReleaseSuggestions;
impl Command for ReleaseSuggestions {
    const NAME: &'static str = "admin/library/release_suggestions";
    const ACCESS: Access = Access::Scope(Scope::Admin);
    const SUMMARY: &'static str =
        "MusicBrainz releases albums look like but were not certain enough to take.";
    type Args = LibrarySuggestionsArgs;
    type Output = Vec<ReleaseSuggestion>;

    async fn handle(ctx: Context, args: LibrarySuggestionsArgs) -> Result<Vec<ReleaseSuggestion>> {
        let library = &ctx.api.library;
        let mut out = Vec::new();
        for suggestion in library.release_suggestions(args.all).await? {
            let Some(album) = library
                .item(bopsie_core::media::MediaType::Album, suggestion.album_id)
                .await?
            else {
                continue;
            };
            out.push(ReleaseSuggestion {
                album: super::music::to_value(album)?,
                url: format!("https://musicbrainz.org/release/{}", suggestion.release_id),
                release_id: suggestion.release_id,
                title: suggestion.title,
                distance: suggestion.distance,
                decision: suggestion.decision,
            });
        }
        Ok(out)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DecideReleaseArgs {
    /// The album's library id.
    pub item_id: String,
    /// `accept` to take the suggested release at the next lookup, `reject` never to suggest it
    /// again.
    pub decision: String,
}

pub struct DecideRelease;
impl Command for DecideRelease {
    const NAME: &'static str = "admin/library/decide_release";
    const ACCESS: Access = Access::Scope(Scope::Admin);
    const SUMMARY: &'static str =
        "Accept or reject the MusicBrainz release suggested for an album.";
    type Args = DecideReleaseArgs;
    type Output = ();

    async fn handle(ctx: Context, args: DecideReleaseArgs) -> Result<()> {
        ctx.api
            .library
            .decide_release(item_id(&args.item_id)?, args.decision)
            .await
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RefreshMetadataArgs {
    /// The album's library id.
    pub item_id: String,
}

pub struct RefreshMetadata;
impl Command for RefreshMetadata {
    const NAME: &'static str = "admin/library/refresh_metadata";
    const ACCESS: Access = Access::Scope(Scope::Admin);
    const SUMMARY: &'static str =
        "Look an album up on MusicBrainz again now, as if it had never been.";
    type Args = RefreshMetadataArgs;
    type Output = ();

    async fn handle(ctx: Context, args: RefreshMetadataArgs) -> Result<()> {
        ctx.api
            .library
            .refresh_metadata(item_id(&args.item_id)?)
            .await
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReconsiderArgs {
    /// The decided suggestion's id, as `admin/library/suggestions` with `all` lists it.
    pub id: i64,
}

pub struct Reconsider;
impl Command for Reconsider {
    const NAME: &'static str = "admin/library/reconsider";
    const ACCESS: Access = Access::Scope(Scope::Admin);
    const SUMMARY: &'static str = "Forget a decision to merge or keep two items apart, Separate's too, and compare them again now.";
    type Args = ReconsiderArgs;
    type Output = ();

    async fn handle(ctx: Context, args: ReconsiderArgs) -> Result<()> {
        ctx.api.library.reconsider(args.id).await
    }
}

pub struct ReconsiderRelease;
impl Command for ReconsiderRelease {
    const NAME: &'static str = "admin/library/reconsider_release";
    const ACCESS: Access = Access::Scope(Scope::Admin);
    const SUMMARY: &'static str = "Forget a decision on the MusicBrainz release suggested for an album, and look it up again now.";
    type Args = RefreshMetadataArgs;
    type Output = ();

    async fn handle(ctx: Context, args: RefreshMetadataArgs) -> Result<()> {
        ctx.api
            .library
            .reconsider_release(item_id(&args.item_id)?)
            .await
    }
}
