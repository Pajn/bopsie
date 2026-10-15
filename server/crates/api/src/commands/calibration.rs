//! Calibrating a group's sync with a phone's microphone: the server plays the chirps, the
//! phone hears them, and the server sets each speaker's `sync_adjust_ms` from what it heard.

use std::collections::BTreeMap;

use bopsie_core::{Error, ErrorCode, Result};
use bopsie_players::settings::SavedEndpoint;
use bopsie_queues::calibration;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::auth::Scope;
use crate::command::{Access, Command, Context};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CalibrationStartArgs {
    /// Any player of the group to calibrate.
    pub player_id: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ChirpView {
    pub from_hz: f64,
    pub to_hz: f64,
    pub duration_ms: u32,
    /// The raised-cosine fade at each end.
    pub fade_ms: u32,
    /// Its peak, as a fraction of full scale.
    pub amplitude: f64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ScheduledChirp {
    pub player_id: String,
    pub name: String,
    /// The endpoint that plays it: the key for its result in `admin/calibration/apply`.
    pub endpoint_id: String,
    /// When it is due, in microseconds on the server's clock, the one Sendspin clients sync to.
    pub at_us: i64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CalibrationView {
    /// The sweep each speaker plays: an exponential sine sweep from `from_hz` to `to_hz`.
    pub chirp: ChirpView,
    /// Every chirp, in the order they play; each speaker chirps once a round.
    pub chirps: Vec<ScheduledChirp>,
    /// When the last chirp has died away, in microseconds on the server's clock.
    pub ends_at_us: i64,
    /// Players the group carries in their own multiroom, which are not measured.
    pub skipped: Vec<String>,
}

pub struct CalibrationStart;
impl Command for CalibrationStart {
    const NAME: &'static str = "admin/calibration/start";
    const ACCESS: Access = Access::Scope(Scope::Admin);
    const SUMMARY: &'static str = "Pause a group and play each of its speakers a chirp in turn, for a phone to hear; returns when each is due.";
    type Args = CalibrationStartArgs;
    type Output = CalibrationView;

    async fn handle(ctx: Context, args: CalibrationStartArgs) -> Result<CalibrationView> {
        let calibration = ctx.api.playback.calibrate(&args.player_id).await?;
        let chirp = calibration.chirp;
        Ok(CalibrationView {
            chirp: ChirpView {
                from_hz: chirp.from_hz,
                to_hz: chirp.to_hz,
                duration_ms: chirp.duration_ms,
                fade_ms: chirp.fade_ms,
                amplitude: chirp.amplitude,
            },
            chirps: calibration
                .chirps
                .into_iter()
                .map(|scheduled| ScheduledChirp {
                    player_id: scheduled.device,
                    name: scheduled.name,
                    endpoint_id: scheduled.endpoint,
                    at_us: scheduled.at,
                })
                .collect(),
            ends_at_us: calibration.ends_at,
            skipped: calibration.skipped,
        })
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CalibrationApplyArgs {
    /// By endpoint id: how long after it was due its chirp was heard, in milliseconds. Only
    /// the differences between endpoints count, so a constant the phone adds does no harm.
    pub delays_ms: BTreeMap<String, f64>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Adjustment {
    pub endpoint_id: String,
    /// What it played with.
    pub previous_ms: i32,
    /// What it plays with from the next play.
    pub sync_adjust_ms: i32,
}

pub struct CalibrationApply;
impl Command for CalibrationApply {
    const NAME: &'static str = "admin/calibration/apply";
    const ACCESS: Access = Access::Scope(Scope::Admin);
    const SUMMARY: &'static str = "Set each endpoint's sync_adjust_ms from when a calibration's chirps were heard, so that they play in step.";
    type Args = CalibrationApplyArgs;
    type Output = Vec<Adjustment>;

    async fn handle(ctx: Context, args: CalibrationApplyArgs) -> Result<Vec<Adjustment>> {
        if args.delays_ms.len() < 2 {
            return Err(Error::invalid_data(
                "A calibration needs what was heard of at least two speakers",
            ));
        }
        let devices = ctx.api.playback.devices();
        let players = devices.players();
        let mut measured = Vec::new();
        for (id, delay) in &args.delays_ms {
            let Some(endpoint) = players.get(id) else {
                return Err(Error::new(
                    ErrorCode::PlayerUnavailable,
                    format!("Endpoint {id} not found"),
                ));
            };
            if !delay.is_finite() {
                return Err(Error::invalid_data(format!("No delay for {id}")));
            }
            let current = endpoint.state().quirks.quirks.sync_adjust_ms.unwrap_or(0);
            measured.push((id.clone(), *delay, current));
        }
        let mut out = Vec::new();
        for ((id, adjust), (_, _, previous)) in calibration::adjustments(&measured)
            .into_iter()
            .zip(&measured)
        {
            let mut saved: SavedEndpoint = players.saved_endpoint(&id);
            saved.quirks.sync_adjust_ms = Some(adjust);
            players.save_endpoint(&id, saved).await?;
            out.push(Adjustment {
                endpoint_id: id,
                previous_ms: *previous,
                sync_adjust_ms: adjust,
            });
        }
        tracing::info!(?out, "calibrated");
        Ok(out)
    }
}
