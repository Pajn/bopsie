//! Music Assistant's `Player`, `PlayerQueue` and `QueueItem`, field for field as 2.10.6 serves
//! them for a Sendspin player. Fields this server has no value for are present and null or
//! empty.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bopsie_core::media::Track;
use bopsie_players::EndpointState;
use bopsie_players::health::Health;
use serde_json::{Value, json};

use crate::state::{PlaybackState, Queue, QueueItem, RepeatMode};

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

pub fn player(
    id: &str,
    endpoint: &EndpointState,
    health: &Health,
    grouping: &Grouping<'_>,
    playback: &PlayerPlayback<'_>,
    viewer_owns: bool,
    base_url: Option<&str>,
) -> Value {
    // An app's player belongs to the user who connected it; to everyone else it is hidden.
    let hidden = endpoint.app && !viewer_owns;
    let group_members: Vec<&str> = if grouping.members.is_empty() {
        Vec::new()
    } else {
        std::iter::once(id)
            .chain(grouping.members.iter().map(String::as_str))
            .collect()
    };
    let current_media = playback.current.map_or(Value::Null, |item| {
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
    });
    let state = state_name(playback.state);
    json!({
        "player_id": id,
        "provider": endpoint.provider,
        "type": "player",
        "name": endpoint.name,
        "available": endpoint.available,
        "device_info": {
            "model": endpoint.device.model,
            "manufacturer": endpoint.device.manufacturer,
            "software_version": endpoint.device.software_version,
            "model_id": null,
            "manufacturer_id": null,
            "identifiers": {},
            "mac_address": null,
            "ip_address": null,
        },
        "supported_features": ["play_media", "volume_mute", "set_members", "volume_set"],
        "playback_state": state,
        "elapsed_time": playback.elapsed.as_secs_f64(),
        "elapsed_time_last_updated": unix_now(),
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
        "active_source": playback.queue_id,
        "source_list": [],
        "active_group": null,
        "current_media": current_media,
        "enabled": true,
        "hide_in_ui": hidden,
        "private": hidden,
        "expose_to_ha": false,
        "icon": if endpoint.app { "smartphone" } else { "speaker" },
        "group_volume": grouping.group_volume.or(endpoint.volume),
        "group_volume_muted": false,
        "extra_attributes": {"health": health},
        "power_control": "none",
        "volume_control": "native",
        "mute_control": "native",
        "output_protocols": [],
        "active_output_protocol": null,
        "needs_setup": false,
        "setup_reason": null,
        "has_setup_flow": false,
        "sleep_timer_expires_at": null,
        "active_source_audio": null,
        "display_name": endpoint.name,
        "state": state,
        "group_childs": group_members,
        "extra_data": {},
        "hide_player_in_ui": if hidden { json!(["always"]) } else { json!([]) },
    })
}
