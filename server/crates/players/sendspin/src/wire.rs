//! The legacy (unencrypted) Sendspin wire, as Music Assistant 2.10.6 speaks it: JSON text frames
//! `{"type", "payload"}` and binary audio frames with a 9-byte header. Unknown message types and
//! fields are ignored, as the protocol requires.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// The binary message type of an audio chunk.
pub const AUDIO_CHUNK: u8 = 4;
/// Type byte and a big-endian `i64` timestamp.
pub const AUDIO_HEADER_BYTES: usize = 9;

/// The `product_name`s Music Assistant registers as standalone players with their own queue.
pub const APP_PRODUCTS: &[&str] = &["Web Browser", "Web Player", "Mobile Application", "PWA"];

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DeviceInfo {
    #[serde(default)]
    pub product_name: Option<String>,
    #[serde(default)]
    pub manufacturer: Option<String>,
    #[serde(default)]
    pub software_version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StreamFormat {
    pub codec: Codec,
    pub sample_rate: u32,
    pub channels: u16,
    pub bit_depth: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Codec {
    Pcm,
    Flac,
    Opus,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PlayerSupport {
    /// In order of preference.
    pub supported_formats: Vec<StreamFormat>,
    /// The most not-yet-played audio, header and payload, the player can hold.
    pub buffer_capacity: u64,
    #[serde(default)]
    pub supported_commands: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ClientHello {
    #[serde(default)]
    pub client_id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub version: Option<u32>,
    #[serde(default)]
    pub supported_roles: Vec<String>,
    #[serde(default)]
    pub device_info: Option<DeviceInfo>,
    #[serde(default, rename = "player@v1_support")]
    pub player_v1_support: Option<PlayerSupport>,
    /// The name Music Assistant 2.7.10 and earlier used.
    #[serde(default)]
    pub player_support: Option<PlayerSupport>,
}

impl ClientHello {
    pub fn player_support(&self) -> Option<&PlayerSupport> {
        self.player_v1_support
            .as_ref()
            .or(self.player_support.as_ref())
    }

    /// Whether this is an app's own player, registered standalone with its own queue.
    pub fn is_app(&self) -> bool {
        self.device_info.as_ref().is_some_and(|info| {
            info.product_name
                .as_deref()
                .is_some_and(|name| APP_PRODUCTS.contains(&name))
                || info.manufacturer.as_deref() == Some("Music Assistant")
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct PlayerState {
    #[serde(default)]
    pub volume: Option<u8>,
    #[serde(default)]
    pub muted: Option<bool>,
    #[serde(default, alias = "output_delay_ms")]
    pub static_delay_ms: Option<u32>,
    #[serde(default)]
    pub required_lead_time_ms: Option<u32>,
    #[serde(default)]
    pub min_buffer_ms: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct ClientState {
    /// The current field.
    #[serde(default)]
    pub available: Option<bool>,
    /// The field Music Assistant 2.9 and earlier used: `synchronized`, `error` or
    /// `external_source`.
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub player: Option<PlayerState>,
}

impl ClientState {
    /// Whether the player can play now, from whichever field it sent.
    pub fn is_available(&self) -> bool {
        match (self.available, self.state.as_deref()) {
            (Some(available), _) => available,
            (None, Some(state)) => state != "external_source",
            (None, None) => true,
        }
    }
}

/// A message from a client.
#[derive(Debug, Clone, PartialEq)]
pub enum ClientMessage {
    Hello(Box<ClientHello>),
    State(ClientState),
    /// `client/time`, with the client's transmit time.
    Time {
        client_transmitted: i64,
    },
    Command(Value),
    Goodbye {
        reason: Option<String>,
    },
    /// Any other type, which is ignored.
    Other(String),
}

#[derive(Debug, thiserror::Error)]
#[error("malformed Sendspin message: {0}")]
pub struct WireError(String);

pub fn parse(text: &str) -> Result<ClientMessage, WireError> {
    #[derive(Deserialize)]
    struct Envelope {
        #[serde(rename = "type")]
        kind: String,
        #[serde(default)]
        payload: Value,
    }
    let envelope: Envelope = serde_json::from_str(text).map_err(|e| WireError(e.to_string()))?;
    let payload = envelope.payload;
    fn parse<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, WireError> {
        serde_json::from_value(value).map_err(|e| WireError(e.to_string()))
    }
    Ok(match envelope.kind.as_str() {
        "client/hello" => ClientMessage::Hello(Box::new(parse(payload)?)),
        "client/state" => ClientMessage::State(parse(payload)?),
        "client/time" => ClientMessage::Time {
            client_transmitted: payload
                .get("client_transmitted")
                .and_then(Value::as_i64)
                .ok_or_else(|| WireError("client/time without client_transmitted".to_owned()))?,
        },
        "client/command" => ClientMessage::Command(payload),
        "client/goodbye" => ClientMessage::Goodbye {
            reason: payload
                .get("reason")
                .and_then(Value::as_str)
                .map(str::to_owned),
        },
        other => ClientMessage::Other(other.to_owned()),
    })
}

fn message(kind: &str, payload: Value) -> String {
    json!({"type": kind, "payload": payload}).to_string()
}

/// The legacy `server/hello`, whose `active_roles` take the place of `server/activate`.
pub fn server_hello(server_id: &str, name: &str, roles: &[&str]) -> String {
    message(
        "server/hello",
        json!({
            "server_id": server_id,
            "name": name,
            "version": 1,
            "connection_reason": "discovery",
            "active_roles": roles,
        }),
    )
}

pub fn server_time(
    client_transmitted: i64,
    server_received: i64,
    server_transmitted: i64,
) -> String {
    message(
        "server/time",
        json!({
            "client_transmitted": client_transmitted,
            "server_received": server_received,
            "server_transmitted": server_transmitted,
        }),
    )
}

pub fn stream_start(server_transmitted: i64, format: StreamFormat) -> String {
    message(
        "stream/start",
        json!({"server_transmitted": server_transmitted, "player": format}),
    )
}

pub fn stream_clear(server_transmitted: i64) -> String {
    message(
        "stream/clear",
        json!({"server_transmitted": server_transmitted, "roles": ["player"]}),
    )
}

pub fn stream_end(server_transmitted: i64) -> String {
    message(
        "stream/end",
        json!({"server_transmitted": server_transmitted, "roles": ["player"]}),
    )
}

pub fn group_update(playing: bool, group_id: &str) -> String {
    let state = if playing { "playing" } else { "stopped" };
    message(
        "group/update",
        json!({"playback_state": state, "group_id": group_id}),
    )
}

pub fn volume_command(volume: u8) -> String {
    message(
        "server/command",
        json!({"player": {"command": "volume", "volume": volume}}),
    )
}

pub fn mute_command(muted: bool) -> String {
    message(
        "server/command",
        json!({"player": {"command": "mute", "mute": muted}}),
    )
}

/// What is playing, for the metadata role. Every field is sent, null when unknown.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Metadata {
    pub timestamp: i64,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album_artist: Option<String>,
    pub album: Option<String>,
    pub artwork_url: Option<String>,
    pub year: Option<i32>,
    pub track: Option<u32>,
    pub progress: Option<Progress>,
    pub repeat: Option<String>,
    pub shuffle: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Progress {
    pub track_progress: u64,
    pub track_duration: u64,
    /// Thousandths: 1000 plays at normal speed, 0 is paused.
    pub playback_speed: u32,
}

pub fn server_state_metadata(metadata: &Metadata) -> String {
    message("server/state", json!({"metadata": metadata}))
}

/// An audio chunk: type 4, the server time its first sample must leave the speaker, then the
/// audio.
pub fn audio_chunk(timestamp: i64, audio: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(AUDIO_HEADER_BYTES + audio.len());
    frame.push(AUDIO_CHUNK);
    frame.extend_from_slice(&timestamp.to_be_bytes());
    frame.extend_from_slice(audio);
    frame
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The hello Bopsie's test client sends.
    const HELLO: &str = r#"{"type":"client/hello","payload":{
        "client_id":"phone-1","name":"Integration test phone","version":1,
        "supported_roles":["player@v1","controller@v1","metadata@v1"],
        "device_info":{"product_name":"Mobile Application","manufacturer":"Bopsie","software_version":"0.0.0"},
        "player@v1_support":{"supported_formats":[{"codec":"pcm","channels":2,"sample_rate":48000,"bit_depth":16}],
            "buffer_capacity":1048576,"supported_commands":["volume","mute"]},
        "player_support":{"supported_formats":[{"codec":"pcm","channels":2,"sample_rate":48000,"bit_depth":16}],
            "buffer_capacity":1048576,"supported_commands":["volume","mute"]}}}"#;

    #[test]
    fn reads_an_app_hello() {
        let ClientMessage::Hello(hello) = parse(HELLO).unwrap() else {
            panic!("not a hello");
        };
        assert_eq!(hello.client_id.as_deref(), Some("phone-1"));
        assert!(hello.is_app());
        let support = hello.player_support().unwrap();
        assert_eq!(support.buffer_capacity, 1_048_576);
        assert_eq!(
            support.supported_formats,
            [StreamFormat {
                codec: Codec::Pcm,
                sample_rate: 48_000,
                channels: 2,
                bit_depth: 16
            }]
        );
    }

    #[test]
    fn hardware_is_not_an_app() {
        let hello = HELLO.replace("Mobile Application", "WiiM Pro");
        let ClientMessage::Hello(hello) = parse(&hello).unwrap() else {
            panic!("not a hello");
        };
        assert!(!hello.is_app());
    }

    #[test]
    fn reads_both_generations_of_client_state() {
        let state = r#"{"type":"client/state","payload":{"available":true,"state":"synchronized",
            "player":{"volume":50,"muted":false,"static_delay_ms":0,"required_lead_time_ms":300,"min_buffer_ms":300}}}"#;
        let ClientMessage::State(state) = parse(state).unwrap() else {
            panic!("not a state");
        };
        assert!(state.is_available());
        let player = state.player.unwrap();
        assert_eq!(
            (player.volume, player.required_lead_time_ms),
            (Some(50), Some(300))
        );

        let old = r#"{"type":"client/state","payload":{"state":"external_source"}}"#;
        let ClientMessage::State(old) = parse(old).unwrap() else {
            panic!("not a state");
        };
        assert!(!old.is_available());
        let renamed = r#"{"type":"client/state","payload":{"player":{"output_delay_ms":40}}}"#;
        let ClientMessage::State(renamed) = parse(renamed).unwrap() else {
            panic!("not a state");
        };
        assert_eq!(renamed.player.unwrap().static_delay_ms, Some(40));
    }

    #[test]
    fn other_messages() {
        assert_eq!(
            parse(r#"{"type":"client/time","payload":{"client_transmitted":1}}"#).unwrap(),
            ClientMessage::Time {
                client_transmitted: 1
            }
        );
        assert_eq!(
            parse(r#"{"type":"client/goodbye","payload":{"reason":"user_request"}}"#).unwrap(),
            ClientMessage::Goodbye {
                reason: Some("user_request".to_owned())
            }
        );
        assert_eq!(
            parse(r#"{"type":"something/new"}"#).unwrap(),
            ClientMessage::Other("something/new".to_owned())
        );
        assert!(parse("not json").is_err());
        assert!(parse(r#"{"type":"client/time","payload":{}}"#).is_err());
    }

    #[test]
    fn server_messages_match_music_assistant() {
        let start: Value = serde_json::from_str(&stream_start(
            9,
            StreamFormat {
                codec: Codec::Pcm,
                sample_rate: 48_000,
                channels: 2,
                bit_depth: 16,
            },
        ))
        .unwrap();
        // As Music Assistant 2.10.6 sent it.
        assert_eq!(
            start,
            json!({"type": "stream/start", "payload": {"server_transmitted": 9,
                "player": {"codec": "pcm", "sample_rate": 48000, "channels": 2, "bit_depth": 16}}})
        );
        let end: Value = serde_json::from_str(&stream_end(5)).unwrap();
        assert_eq!(
            end,
            json!({"type": "stream/end", "payload": {"server_transmitted": 5, "roles": ["player"]}})
        );
        let volume: Value = serde_json::from_str(&volume_command(70)).unwrap();
        assert_eq!(
            volume,
            json!({"type": "server/command", "payload": {"player": {"command": "volume", "volume": 70}}})
        );
    }

    #[test]
    fn audio_chunks_carry_a_big_endian_timestamp() {
        // A header Music Assistant 2.10.6 sent.
        let frame = audio_chunk(9_599_235_649, &[1, 2, 3, 4]);
        assert_eq!(frame[..9], [4, 0, 0, 0, 2, 60, 40, 182, 65]);
        assert_eq!(&frame[9..], [1, 2, 3, 4]);
    }
}
