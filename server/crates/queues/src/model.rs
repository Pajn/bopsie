//! Music Assistant's `Player`, `PlayerQueue` and `QueueItem`, field for field as 2.10.6 serves
//! them for a Sendspin player. Fields this server has no value for are present and null or
//! empty.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bopsie_core::media::Track;
use bopsie_players::EndpointState;
use bopsie_players::health::Health;
use bopsie_sources::SourceState;
use serde_json::{Value, json};

use crate::state::{PlaybackState, Queue, QueueItem, RepeatMode};

/// `aabbcc001122` as `AA:BB:CC:00:11:22`.
fn mac_address(hex: &str) -> String {
    hex.as_bytes()
        .chunks(2)
        .map(|pair| String::from_utf8_lossy(pair).to_uppercase())
        .collect::<Vec<_>>()
        .join(":")
}

/// Unix seconds now, as Music Assistant stamps elapsed times.
pub fn unix_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

fn state_name(state: PlaybackState) -> &'static str {
    match state {
        PlaybackState::Idle => "idle",
        PlaybackState::Paused => "paused",
        PlaybackState::Playing => "playing",
    }
}

fn repeat_name(mode: RepeatMode) -> &'static str {
    match mode {
        RepeatMode::Off => "off",
        RepeatMode::One => "one",
        RepeatMode::All => "all",
    }
}

fn first_artist(track: &Track) -> Option<&str> {
    track.artists.first().map(|a| a.name.as_str())
}

/// The image a track shows: its own, else its album's.
fn image(track: &Track) -> Value {
    track
        .fields
        .metadata
        .images
        .as_ref()
        .and_then(|images| images.first())
        .or_else(|| track.album.as_ref().and_then(|a| a.image.as_ref()))
        .map_or(Value::Null, |image| json!(image))
}

/// An absolute artwork URL for clients that cannot build one, when the server knows its own.
pub fn artwork_url(track: &Track, base_url: Option<&str>) -> Option<String> {
    let base = base_url?;
    let proxy_id = image(track).get("proxy_id")?.as_str()?.to_owned();
    Some(format!("{base}/imageproxy/{proxy_id}?size=512&fmt=jpeg"))
}

pub fn queue_item(queue_id: &str, item: &QueueItem, index: usize) -> Value {
    let track = &item.track;
    let name = match first_artist(track) {
        Some(artist) => format!("{artist} - {}", track.fields.name),
        None => track.fields.name.clone(),
    };
    json!({
        "queue_id": queue_id,
        "queue_item_id": item.queue_item_id,
        "name": name,
        "duration": track.duration,
        "sort_index": item.sort_index,
        "streamdetails": null,
        "media_item": track,
        "image": image(track),
        "index": index,
        "available": true,
        "extra_attributes": {},
    })
}

/// The queue of the player `queue_id`, at `elapsed` into the current item.
pub fn queue(
    queue_id: &str,
    name: &str,
    available: bool,
    queue: &Queue,
    elapsed: Duration,
) -> Value {
    let item = |index: Option<usize>| {
        index
            .and_then(|i| queue.items.get(i).map(|item| queue_item(queue_id, item, i)))
            .unwrap_or(Value::Null)
    };
    let next_index = queue.current_index.and_then(|i| queue.next_index(i, false));
    json!({
        "queue_id": queue_id,
        "active": true,
        "display_name": name,
        "available": available,
        "items": queue.items.len(),
        "shuffle_enabled": queue.shuffle_enabled,
        "repeat_mode": repeat_name(queue.repeat_mode),
        "crossfade_enabled": false,
        "autoplay_enabled": false,
        "overlay_enabled": false,
        "overlay_source": null,
        "overlay_volume": 100,
        "smart_fades_active": false,
        "smart_shuffle_active": false,
        "current_index": queue.current_index,
        "index_in_buffer": queue.index_in_buffer,
        "ended": queue.ended,
        "elapsed_time": elapsed.as_secs_f64(),
        "elapsed_time_last_updated": unix_now(),
        "playback_speed": 1,
        "state": state_name(queue.state),
        "current_item": item(queue.current_index),
        "next_item": item(next_index),
        "sources": [],
        "flow_mode": false,
        "resume_pos": queue.resume_position.as_secs(),
        "is_dynamic": false,
        "extra_attributes": {},
        "dont_stop_the_music_enabled": false,
        "radio_source": [],
    })
}

/// What a player shows of its group's playback.
pub struct PlayerPlayback<'a> {
    pub state: PlaybackState,
    pub queue_id: &'a str,
    pub current: Option<&'a QueueItem>,
    pub elapsed: Duration,
    /// Its group is meant to be playing, but nothing is heard yet: see [`queue`].
    pub buffering: bool,
    /// The source the player's group plays in place of its queue, if it plays one.
    pub source: Option<SourcePlayback<'a>>,
    /// The sources announced for the player.
    pub sources: Vec<SourceEntry>,
}

/// A source as its group plays it.
pub struct SourcePlayback<'a> {
    pub id: &'a str,
    pub state: &'a SourceState,
}

/// A source announced for a player: Music Assistant's `PlayerSource`.
pub struct SourceEntry {
    pub id: String,
    pub name: String,
}

/// How a player stands in its group.
pub struct Grouping<'a> {
    pub synced_to: Option<&'a str>,
    /// The leader's followers; empty for a follower or a player alone.
    pub members: &'a [String],
    pub can_group_with: Vec<String>,
    /// The loudest member's volume.
    pub group_volume: Option<u8>,
}

/// What a player is: a device, as its endpoint in use describes it, or with
/// `return_protocol_players` one of a device's endpoints on its own.
pub struct PlayerView<'a> {
    pub id: &'a str,
    pub name: &'a str,
    /// `player`, or `protocol` for one endpoint of a device.
    pub kind: &'static str,
    pub enabled: bool,
    pub available: bool,
    pub endpoint: &'a EndpointState,
    pub health: &'a Health,
    pub features: Vec<&'static str>,
    /// The device's endpoints, when it has more than one.
    pub output_protocols: Vec<Value>,
    /// The endpoint playing for the device, when it is.
    pub active_output_protocol: Option<&'a str>,
}

pub fn player(
    view: &PlayerView<'_>,
    grouping: &Grouping<'_>,
    playback: &PlayerPlayback<'_>,
    viewer_owns: bool,
    base_url: Option<&str>,
) -> Value {
    let (id, endpoint) = (view.id, view.endpoint);
    // An app's player belongs to the user who connected it; to everyone else it is hidden.
    // A device's endpoints on their own are shown only to those who ask for them.
    let hidden = (endpoint.app && !viewer_owns) || view.kind == "protocol";
    let group_members: Vec<&str> = if grouping.members.is_empty() {
        Vec::new()
    } else {
        std::iter::once(id)
            .chain(grouping.members.iter().map(String::as_str))
            .collect()
    };
    let source_media = playback.source.as_ref().map(|source| {
        let elapsed = source.state.position_at(bopsie_core::clock::now()) as f64 / 1_000.0;
        source.state.media.as_ref().map_or(Value::Null, |media| {
            json!({
                "uri": media.uri,
                "media_type": "track",
                "title": media.title,
                "artist": media.artist,
                "album": media.album,
                "album_artist": media.artist,
                "image_url": media.artwork_url,
                "duration": media.duration_ms / 1_000,
                "stream_duration": null,
                "source_id": source.id,
                "queue_item_id": null,
                "custom_data": null,
                "elapsed_time": elapsed,
                "elapsed_time_last_updated": unix_now(),
            })
        })
    });
    let current_media = source_media.unwrap_or_else(|| {
        playback.current.map_or(Value::Null, |item| {
            let track = &item.track;
            json!({
                "uri": track.fields.uri,
                "media_type": "track",
                "title": track.fields.name,
                "artist": first_artist(track),
                "album": track.album.as_ref().map(|a| a.name.as_str()),
                "album_artist": first_artist(track),
                "image_url": artwork_url(track, base_url),
                "duration": track.duration,
                "stream_duration": null,
                "source_id": playback.queue_id,
                "queue_item_id": item.queue_item_id,
                "custom_data": null,
                "elapsed_time": playback.elapsed.as_secs_f64(),
                "elapsed_time_last_updated": unix_now(),
            })
        })
    });
    let (state, elapsed, active_source) = match &playback.source {
        Some(source) => (
            if source.state.playing {
                "playing"
            } else {
                "paused"
            },
            source.state.position_at(bopsie_core::clock::now()) as f64 / 1_000.0,
            source.id,
        ),
        None => (
            state_name(playback.state),
            playback.elapsed.as_secs_f64(),
            playback.queue_id,
        ),
    };
    let source_list: Vec<Value> = playback
        .sources
        .iter()
        .map(|source| {
            json!({
                "id": source.id,
                "name": source.name,
                "passive": false,
                "can_play_pause": true,
                "can_seek": true,
                "can_next_previous": true,
            })
        })
        .collect();
    json!({
        "player_id": id,
        "provider": endpoint.provider,
        "type": view.kind,
        "name": view.name,
        "available": view.available,
        "device_info": {
            "model": endpoint.device.model,
            "manufacturer": endpoint.device.manufacturer,
            "software_version": endpoint.device.software_version,
            "model_id": null,
            "manufacturer_id": null,
            "identifiers": {},
            "mac_address": endpoint.facts.mac.as_deref().map(mac_address),
            "ip_address": endpoint.facts.address.map(|a| a.to_string()),
        },
        "supported_features": view.features,
        "playback_state": state,
        "elapsed_time": elapsed,
        "elapsed_time_last_updated": unix_now(),
        "buffering": playback.buffering,
        "powered": true,
        "volume_level": endpoint.volume,
        "volume_muted": endpoint.muted,
        "group_members": group_members,
        "static_group_members": [],
        "can_group_with": grouping.can_group_with,
        "synced_to": grouping.synced_to,
        "active_sound_mode": null,
        "sound_mode_list": [],
        "options": [],
        "active_source": active_source,
        "source_list": source_list,
        "active_group": null,
        "current_media": current_media,
        "enabled": view.enabled,
        "hide_in_ui": hidden,
        "private": hidden,
        "expose_to_ha": false,
        "icon": if endpoint.app { "smartphone" } else { "speaker" },
        "group_volume": grouping.group_volume.or(endpoint.volume),
        "group_volume_muted": false,
        "extra_attributes": {"health": view.health},
        "power_control": "none",
        "volume_control": "native",
        "mute_control": "native",
        "output_protocols": view.output_protocols,
        "active_output_protocol": view.active_output_protocol,
        "needs_setup": false,
        "setup_reason": null,
        "has_setup_flow": false,
        "sleep_timer_expires_at": null,
        "active_source_audio": null,
        "display_name": view.name,
        "state": state,
        "group_childs": group_members,
        "extra_data": {},
        "hide_player_in_ui": if hidden { json!(["always"]) } else { json!([]) },
    })
}
