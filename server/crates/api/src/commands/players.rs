//! `players/*` commands.

use bopsie_core::{Error, ErrorCode, Result};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::auth::Scope;
use crate::command::{Access, Command, Context};

fn viewer(ctx: &Context) -> Option<&str> {
    ctx.session.as_ref().map(|s| s.token_id.as_str())
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AllArgs {
    /// Include players that are not reachable.
    #[serde(default)]
    pub return_unavailable: Option<bool>,
    /// Include players the config file disables.
    #[serde(default)]
    pub return_disabled: Option<bool>,
    /// Also list, each on its own, the endpoints of players reached over more than one
    /// protocol.
    #[serde(default)]
    pub return_protocol_players: Option<bool>,
}

pub struct All;
impl Command for All {
    const NAME: &'static str = "players/all";
    const ACCESS: Access = Access::Scope(Scope::PlayersRead);
    const SUMMARY: &'static str = "Every player.";
    type Args = AllArgs;
    type Output = Vec<Value>;

    async fn handle(ctx: Context, args: AllArgs) -> Result<Vec<Value>> {
        let unavailable = args.return_unavailable.unwrap_or(true);
        let disabled = args.return_disabled.unwrap_or(false);
        let protocols = args.return_protocol_players.unwrap_or(false);
        Ok(ctx
            .api
            .playback
            .players(viewer(&ctx), protocols)
            .into_iter()
            .filter(|player| unavailable || player["available"] == Value::Bool(true))
            .filter(|player| disabled || player["enabled"] != Value::Bool(false))
            .collect())
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetArgs {
    pub player_id: String,
    /// Fail rather than return null when the player is unknown or unavailable.
    #[serde(default)]
    pub raise_unavailable: Option<bool>,
}

pub struct Get;
impl Command for Get {
    const NAME: &'static str = "players/get";
    const ACCESS: Access = Access::Scope(Scope::PlayersRead);
    const SUMMARY: &'static str = "A player, or null when there is none.";
    type Args = GetArgs;
    type Output = Option<Value>;

    async fn handle(ctx: Context, args: GetArgs) -> Result<Option<Value>> {
        let player = ctx.api.playback.player(&args.player_id, viewer(&ctx));
        let usable = player
            .as_ref()
            .is_some_and(|p| p["available"] == Value::Bool(true));
        if args.raise_unavailable.unwrap_or(false) && !usable {
            return Err(Error::new(
                ErrorCode::PlayerUnavailable,
                format!("Player {} is not available", args.player_id),
            ));
        }
        Ok(player)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VolumeArgs {
    pub player_id: String,
    /// 0–100.
    pub volume_level: u8,
}

pub struct VolumeSet;
impl Command for VolumeSet {
    const NAME: &'static str = "players/cmd/volume_set";
    const ACCESS: Access = Access::Scope(Scope::PlayersControl);
    const SUMMARY: &'static str = "Set a player's volume.";
    type Args = VolumeArgs;
    type Output = ();

    async fn handle(ctx: Context, args: VolumeArgs) -> Result<()> {
        ctx.api
            .playback
            .volume_set(&args.player_id, args.volume_level)
    }
}

pub struct GroupVolume;
impl Command for GroupVolume {
    const NAME: &'static str = "players/cmd/group_volume";
    const ACCESS: Access = Access::Scope(Scope::PlayersControl);
    const SUMMARY: &'static str = "Set a group's volume: every member moves by the same proportion, the loudest to the level given.";
    type Args = VolumeArgs;
    type Output = ();

    async fn handle(ctx: Context, args: VolumeArgs) -> Result<()> {
        ctx.api
            .playback
            .group_volume(&args.player_id, args.volume_level)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MuteArgs {
    pub player_id: String,
    pub muted: bool,
}

pub struct VolumeMute;
impl Command for VolumeMute {
    const NAME: &'static str = "players/cmd/volume_mute";
    const ACCESS: Access = Access::Scope(Scope::PlayersControl);
    const SUMMARY: &'static str = "Mute or unmute a player.";
    type Args = MuteArgs;
    type Output = ();

    async fn handle(ctx: Context, args: MuteArgs) -> Result<()> {
        ctx.api.playback.volume_mute(&args.player_id, args.muted)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GroupArgs {
    /// The player to add.
    pub player_id: String,
    /// A player in the group to join.
    pub target_player: String,
}

pub struct Group;
impl Command for Group {
    const NAME: &'static str = "players/cmd/group";
    const ACCESS: Access = Access::Scope(Scope::PlayersControl);
    const SUMMARY: &'static str =
        "Add a player to another player's group. It stops what it was playing.";
    type Args = GroupArgs;
    type Output = ();

    async fn handle(ctx: Context, args: GroupArgs) -> Result<()> {
        ctx.api.playback.group(&args.player_id, &args.target_player)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PlayerArgs {
    pub player_id: String,
}

pub struct Ungroup;
impl Command for Ungroup {
    const NAME: &'static str = "players/cmd/ungroup";
    const ACCESS: Access = Access::Scope(Scope::PlayersControl);
    const SUMMARY: &'static str = "Take a player out of its group. A leader hands its queue and followers to the next member.";
    type Args = PlayerArgs;
    type Output = ();

    async fn handle(ctx: Context, args: PlayerArgs) -> Result<()> {
        ctx.api.playback.ungroup(&args.player_id)
    }
}
