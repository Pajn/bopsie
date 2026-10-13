//! Commands a client sends as soon as it signs in, answered as a server with no players
//! truthfully would. Each gives way to the real command when the players and queues arrive.

use bopsie_core::Result;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::auth::Scope;
use crate::command::{Access, Command, Context};

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
