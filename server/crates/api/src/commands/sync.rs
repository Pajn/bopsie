//! How speakers are kept in step: each endpoint's `sync_adjust_ms`, the offset AirPlay speakers
//! share beside other protocols, and what the last calibration set.

use bopsie_core::{Error, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::auth::Scope;
use crate::command::{Access, Command, Context};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NoArgs {}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SyncEndpoint {
    pub endpoint_id: String,
    pub device_id: String,
    /// The device's name.
    pub name: String,
    pub provider: String,
    pub available: bool,
    /// How much later than the rest of a group it plays, in ms; 0 unless set.
    pub sync_adjust_ms: i32,
    /// Where that comes from, as `admin/devices` says: `null` for the default.
    pub source: Value,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SyncView {
    /// How much later every AirPlay speaker plays in a group that mixes protocols, in ms.
    pub mixed_airplay_ms: i32,
    /// What the last calibration set: `at` in Unix seconds, and each speaker it measured with
    /// `heard_ms`, `previous_ms`, `sync_adjust_ms` and `shared_airplay`, true when what it set
    /// is the AirPlay offset. `null` before the first.
    pub last_calibration: Option<Value>,
    pub endpoints: Vec<SyncEndpoint>,
}

pub struct Sync;
impl Command for Sync {
    const NAME: &'static str = "admin/sync";
    const ACCESS: Access = Access::Scope(Scope::Admin);
    const SUMMARY: &'static str = "How speakers are kept in step: each endpoint's sync adjustment, the offset AirPlay speakers share beside other protocols, and what the last calibration set.";
    type Args = NoArgs;
    type Output = SyncView;

    async fn handle(ctx: Context, _args: NoArgs) -> Result<SyncView> {
        let playback = &ctx.api.playback;
        let mut endpoints: Vec<SyncEndpoint> = playback
            .devices()
            .all()
            .into_iter()
            .flat_map(|device| {
                device.members.into_iter().map(move |member| SyncEndpoint {
                    endpoint_id: member.id.clone(),
                    device_id: device.id.clone(),
                    name: device.name.clone(),
                    provider: member.state.provider.to_owned(),
                    available: member.state.available,
                    sync_adjust_ms: member.state.quirks.quirks.sync_adjust_ms.unwrap_or(0),
                    source: json!(member.state.quirks.source("sync_adjust_ms")),
                })
            })
            .collect();
        endpoints.sort_by(|a, b| a.name.cmp(&b.name).then(a.provider.cmp(&b.provider)));
        Ok(SyncView {
            mixed_airplay_ms: playback.mixed_airplay_ms(),
            last_calibration: playback.last_calibration(),
            endpoints,
        })
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SyncUpdateArgs {
    /// How much later every AirPlay speaker plays in a group that mixes protocols, in ms.
    pub mixed_airplay_ms: i32,
}

pub struct SyncUpdate;
impl Command for SyncUpdate {
    const NAME: &'static str = "admin/sync/update";
    const ACCESS: Access = Access::Scope(Scope::Admin);
    const SUMMARY: &'static str = "Set how much later AirPlay speakers play in a group that mixes protocols. Each endpoint's own adjustment is set with admin/endpoint/update.";
    type Args = SyncUpdateArgs;
    type Output = ();

    async fn handle(ctx: Context, args: SyncUpdateArgs) -> Result<()> {
        if !(-5_000..=5_000).contains(&args.mixed_airplay_ms) {
            return Err(Error::invalid_data(
                "The offset is at most 5 seconds either way",
            ));
        }
        ctx.api
            .playback
            .set_mixed_airplay_ms(args.mixed_airplay_ms)
            .await
    }
}
