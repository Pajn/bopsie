//! Calibrating a group's sync with a phone's microphone: the server plays the chirps, the
//! phone hears them, and the server sets each speaker's `sync_adjust_ms` from what it heard.

use std::collections::BTreeMap;

use bopsie_core::{Error, ErrorCode, Result};
use bopsie_players::settings::SavedEndpoint;
use bopsie_queues::calibration;
use bopsie_queues::playback::AIRPLAY;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

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
    /// The server's clock as it answered, in microseconds: what a phone whose own idea of the
    /// server's clock is off can take its times from.
    pub now_us: i64,
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
            now_us: bopsie_core::clock::now(),
        })
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CalibrationApplyArgs {
    /// By endpoint id: how long after it was due its chirp was heard, in milliseconds. Only
    /// the differences between endpoints count, so a constant the phone adds does no harm.
    pub delays_ms: BTreeMap<String, f64>,
    /// Answers with what it would set, and saves nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// What a calibration sets for an endpoint. In a group that mixes protocols, every AirPlay
/// speaker reports the offset AirPlay speakers share beside other protocols, rather than its
/// own `sync_adjust_ms`, which it leaves as it was.
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
    const SUMMARY: &'static str = "Set each endpoint's sync_adjust_ms from when a calibration's chirps were heard, so that they play in step; beside other protocols, AirPlay speakers share one offset instead.";
    type Args = CalibrationApplyArgs;
    type Output = Vec<Adjustment>;

    async fn handle(ctx: Context, args: CalibrationApplyArgs) -> Result<Vec<Adjustment>> {
        if args.delays_ms.len() < 2 {
            return Err(Error::invalid_data(
                "A calibration needs what was heard of at least two speakers",
            ));
        }
        let playback = &ctx.api.playback;
        let devices = playback.devices();
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
            let state = endpoint.state();
            let own = state.quirks.quirks.sync_adjust_ms.unwrap_or(0);
            measured.push((id.clone(), *delay, own, state.provider == AIRPLAY));
        }
        // AirPlay speakers keep in step with each other on PTP; beside other protocols they
        // move together, by the offset they played with on top of their own adjustments.
        let mixed = measured.iter().any(|m| m.3) && measured.iter().any(|m| !m.3);
        let offset = playback.mixed_airplay_ms();
        let played: Vec<(String, f64, i32)> = measured
            .iter()
            .map(|(id, delay, own, airplay)| {
                let shared = if mixed && *airplay { offset } else { 0 };
                (id.clone(), *delay, own + shared)
            })
            .collect();
        let adjusted = calibration::adjustments(&played);
        let new_offset = mixed.then(|| {
            let mut wanted: Vec<i32> = adjusted
                .iter()
                .zip(&measured)
                .filter(|(_, m)| m.3)
                .map(|((_, adjust), (_, _, own, _))| adjust - own)
                .collect();
            wanted.sort_unstable();
            wanted[wanted.len() / 2]
        });
        let mut out = Vec::new();
        for ((id, adjust), (_, _, own, airplay)) in adjusted.into_iter().zip(&measured) {
            let (previous, set) = match new_offset {
                Some(new_offset) if *airplay => (offset, new_offset),
                _ => {
                    if !args.dry_run {
                        let mut saved: SavedEndpoint = players.saved_endpoint(&id);
                        saved.quirks.sync_adjust_ms = Some(adjust);
                        players.save_endpoint(&id, saved).await?;
                    }
                    (*own, adjust)
                }
            };
            out.push(Adjustment {
                endpoint_id: id,
                previous_ms: previous,
                sync_adjust_ms: set,
            });
        }
        if let (Some(new_offset), false) = (new_offset, args.dry_run) {
            playback.set_mixed_airplay_ms(new_offset).await?;
        }
        if !args.dry_run {
            let speakers: Vec<Value> = out
                .iter()
                .zip(&measured)
                .map(|(adjustment, (_, delay, _, airplay))| {
                    let device = devices.device_of(&adjustment.endpoint_id);
                    json!({
                        "endpoint_id": adjustment.endpoint_id,
                        "device_id": device.as_ref().map(|d| d.id.clone()),
                        "name": device.as_ref().map(|d| d.name.clone()),
                        "heard_ms": delay,
                        "previous_ms": adjustment.previous_ms,
                        "sync_adjust_ms": adjustment.sync_adjust_ms,
                        "shared_airplay": new_offset.is_some() && *airplay,
                    })
                })
                .collect();
            playback
                .record_calibration(json!({
                    "at": std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |d| d.as_secs()),
                    "speakers": speakers,
                }))
                .await?;
        }
        if !args.dry_run {
            tracing::info!(?out, "calibrated");
        }
        Ok(out)
    }
}
