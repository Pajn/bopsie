//! Pairing AirPlay speakers that ask for a PIN, such as Apple TVs and HomePods.

use bopsie_core::{Error, ErrorCode, Result};
use bopsie_players_airplay::AirplayProvider;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::auth::Scope;
use crate::command::{Access, Command, Context};

fn provider(ctx: &Context) -> Result<AirplayProvider> {
    ctx.api
        .airplay
        .clone()
        .ok_or_else(|| Error::new(ErrorCode::ProviderUnavailable, "AirPlay is not set up"))
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PairStartArgs {
    pub player_id: String,
}

pub struct PairStart;
impl Command for PairStart {
    const NAME: &'static str = "airplay/pair_start";
    const ACCESS: Access = Access::Scope(Scope::PlayersControl);
    const SUMMARY: &'static str = "Ask an AirPlay speaker to show a pairing PIN.";
    type Args = PairStartArgs;
    type Output = ();

    async fn handle(ctx: Context, args: PairStartArgs) -> Result<()> {
        provider(&ctx)?.pair_start(&args.player_id)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PairFinishArgs {
    pub player_id: String,
    /// The PIN the speaker shows.
    pub pin: String,
}

pub struct PairFinish;
impl Command for PairFinish {
    const NAME: &'static str = "airplay/pair_finish";
    const ACCESS: Access = Access::Scope(Scope::PlayersControl);
    const SUMMARY: &'static str =
        "Pair with the PIN the speaker shows. The speaker can play once paired.";
    type Args = PairFinishArgs;
    type Output = ();

    async fn handle(ctx: Context, args: PairFinishArgs) -> Result<()> {
        provider(&ctx)?
            .pair_finish(&args.player_id, &args.pin)
            .await
    }
}
