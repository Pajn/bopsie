//! Changing a player's settings from the web UI. What is saved here is kept in the database and
//! wins over the config file; a field set to `null` clears what was saved, so the config file's
//! value, or the default, applies again. A field left out is left as it is.

use bopsie_core::{Error, ErrorCode, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer};
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
}

pub struct PlayerUpdate;
impl Command for PlayerUpdate {
    const NAME: &'static str = "admin/player/update";
    const ACCESS: Access = Access::Scope(Scope::Admin);
    const SUMMARY: &'static str = "Save a player's name, whether it is enabled, and its maximum volume, over the config file.";
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
