//! What a LinkPlay device's HTTP API answers: `getStatusEx`, `getPlayerStatus` and
//! `multiroom:getSlaveList`. Every value comes as a string, numbers and flags included; text such
//! as titles is hex-encoded UTF-8. Fields a firmware leaves out are absent here too.

use serde_json::Value;

fn text(value: &Value, key: &str) -> Option<String> {
    match value.get(key)? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn number<T: std::str::FromStr>(value: &Value, key: &str) -> Option<T> {
    text(value, key)?.trim().parse().ok()
}

/// A value such as `0x1edf3bfd`.
fn hex_number(value: &Value, key: &str) -> Option<u64> {
    let text = text(value, key)?;
    let digits = text
        .trim()
        .trim_start_matches("0x")
        .trim_start_matches("0X");
    u64::from_str_radix(digits, 16).ok()
}

/// Hex-encoded UTF-8, as titles come. `unknown` and empty mean none.
pub fn decode_hex_text(encoded: &str) -> Option<String> {
    let bytes = hex::decode(encoded.trim()).ok()?;
    let decoded = String::from_utf8_lossy(&bytes).trim().to_owned();
    (!decoded.is_empty() && !decoded.eq_ignore_ascii_case("unknown")).then_some(decoded)
}

/// `streams` bit 0: the device has AirPlay, whether or not it announces it now.
const STREAM_AIRPLAY: u64 = 1;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatusEx {
    pub name: String,
    /// The model, such as `WiiM_Amp_Pro`.
    pub project: Option<String>,
    pub firmware: Option<String>,
    /// As `AA:BB:CC:DD:EE:FF`.
    pub mac: Option<String>,
    /// 24 hex digits.
    pub uuid: Option<String>,
    /// The multiroom protocol, such as `4.2`.
    pub wmrm_version: Option<String>,
    /// Following another device.
    pub follower: bool,
    pub master_ip: Option<String>,
    pub master_uuid: Option<String>,
    pub streams: Option<u64>,
}

impl StatusEx {
    pub fn parse(json: &str) -> Option<Self> {
        let value: Value = serde_json::from_str(json).ok()?;
        let follower = text(&value, "group").is_some_and(|g| g.trim() != "0");
        Some(Self {
            name: text(&value, "DeviceName")
                .or_else(|| text(&value, "GroupName"))
                .unwrap_or_default(),
            project: text(&value, "project").or_else(|| text(&value, "priv_prj")),
            firmware: text(&value, "firmware"),
            mac: text(&value, "MAC").filter(|m| !m.is_empty()),
            uuid: text(&value, "uuid").filter(|u| !u.is_empty()),
            wmrm_version: text(&value, "wmrm_version"),
            follower,
            master_ip: text(&value, "master_ip").filter(|_| follower),
            master_uuid: text(&value, "master_uuid").filter(|_| follower),
            streams: hex_number(&value, "streams_all").or_else(|| hex_number(&value, "streams")),
        })
    }

    pub fn airplay(&self) -> bool {
        self.streams.is_some_and(|s| s & STREAM_AIRPLAY != 0)
    }

    /// The multiroom protocol's major version: devices group only within one.
    pub fn multiroom_major(&self) -> Option<u32> {
        self.wmrm_version
            .as_deref()?
            .split('.')
            .next()?
            .parse()
            .ok()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Playing,
    Paused,
    Stopped,
    /// Loading what it was told to play.
    Loading,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlayerStatus {
    pub transport: Option<Transport>,
    /// Milliseconds into what plays; negative while counting down to when it is audible.
    pub position_ms: Option<i64>,
    pub volume: Option<u8>,
    pub muted: Option<bool>,
    /// The input, such as `10` for the network or `49` for HDMI.
    pub mode: Option<String>,
    pub title: Option<String>,
}

impl PlayerStatus {
    pub fn parse(json: &str) -> Option<Self> {
        let value: Value = serde_json::from_str(json).ok()?;
        let transport = text(&value, "status").and_then(|s| match s.as_str() {
            "play" => Some(Transport::Playing),
            "pause" => Some(Transport::Paused),
            "stop" | "none" => Some(Transport::Stopped),
            "load" | "loading" => Some(Transport::Loading),
            _ => None,
        });
        Some(Self {
            transport,
            position_ms: number(&value, "curpos"),
            volume: number::<u8>(&value, "vol").map(|v| v.min(100)),
            muted: text(&value, "mute").map(|m| m.trim() == "1"),
            mode: text(&value, "mode"),
            title: text(&value, "Title").and_then(|t| decode_hex_text(&t)),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Follower {
    pub name: String,
    pub uuid: String,
    pub ip: String,
    pub volume: Option<u8>,
    pub muted: Option<bool>,
}

/// A leader's `multiroom:getSlaveList`.
pub fn parse_followers(json: &str) -> Option<Vec<Follower>> {
    let value: Value = serde_json::from_str(json).ok()?;
    let list = match value.get("slave_list") {
        Some(Value::Array(list)) => list.clone(),
        _ => Vec::new(),
    };
    Some(
        list.iter()
            .filter_map(|f| {
                Some(Follower {
                    name: text(f, "name").unwrap_or_default(),
                    uuid: text(f, "uuid")?,
                    ip: text(f, "ip")?,
                    volume: number::<u8>(f, "volume").map(|v| v.min(100)),
                    muted: text(f, "mute").map(|m| m.trim() == "1"),
                })
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// As a WiiM Amp Pro on firmware 5.2 answers, shortened.
    const AMP: &str = r#"{"language":"en_us","ssid":"WiiM Amp Pro-1122","hardware":"AmlogicA113","firmware":"Linkplay.5.2.830031","Release":"20260921","project":"WiiM_Amp_Pro","priv_prj":"WiiM_Amp_Pro","DeviceName":"Living Room","GroupName":"Living Room","group":"0","uuid":"0123456789ABCDEF01234567","MAC":"AA:BB:CC:00:11:22","streams_all":"0xedf33fc","streams":"0xedf33fc","wmrm_version":"4.3"}"#;
    /// As an Audio Pro A28 following another device answers, shortened.
    const A28: &str = r#"{"project":"A28 Speaker","priv_prj":"A28 Speaker","firmware":"audiopro_a28-user 1.56 1.56 1.56.634247 test-keys","DeviceName":"Kitchen","GroupName":"Kitchen","group":"1","master_uuid":"0123456789ABCDEF01234567","master_ip":"10.0.0.140","slave_mask":"0","uuid":"89ABCDEF0123456789ABCDEF","MAC":"AA:BB:CC:00:11:33","streams_all":"0x1edf3bfd","streams":"0x1edf3bfd","wmrm_version":"4.2"}"#;

    #[test]
    fn status_of_a_leader_and_a_follower() {
        let amp = StatusEx::parse(AMP).unwrap();
        assert_eq!(amp.name, "Living Room");
        assert_eq!(amp.project.as_deref(), Some("WiiM_Amp_Pro"));
        assert_eq!(amp.mac.as_deref(), Some("AA:BB:CC:00:11:22"));
        assert!(!amp.follower && amp.master_ip.is_none());
        assert!(!amp.airplay(), "the Amp Pro has no AirPlay");
        assert_eq!(amp.multiroom_major(), Some(4));

        let a28 = StatusEx::parse(A28).unwrap();
        assert!(a28.follower);
        assert_eq!(a28.master_ip.as_deref(), Some("10.0.0.140"));
        assert_eq!(a28.master_uuid.as_deref(), Some("0123456789ABCDEF01234567"));
        assert!(a28.airplay(), "hidden while it follows, but there");
        assert_eq!(a28.wmrm_version.as_deref(), Some("4.2"));
        assert!(StatusEx::parse("unknown command").is_none());
    }

    #[test]
    fn player_status_counts_down_then_plays() {
        let status = PlayerStatus::parse(
            r#"{"type":"0","mode":"10","status":"play","curpos":"-535","totlen":"0","Title":"4F70656E696E67","Artist":"556E6B6E6F776E","vol":"5","mute":"0"}"#,
        )
        .unwrap();
        assert_eq!(status.transport, Some(Transport::Playing));
        assert_eq!(status.position_ms, Some(-535));
        assert_eq!(status.volume, Some(5));
        assert_eq!(status.muted, Some(false));
        assert_eq!(status.mode.as_deref(), Some("10"));
        assert_eq!(status.title.as_deref(), Some("Opening"));
        assert_eq!(decode_hex_text("556E6B6E6F776E"), None, "Unknown is none");
    }

    #[test]
    fn followers_of_a_leader() {
        let list = parse_followers(
            r#"{ "slaves": 1, "wmrm_version": "4.3", "slave_list": [ { "name": "Kitchen", "uuid": "89ABCDEF0123456789ABCDEF", "ip": "10.0.0.41", "version": "4.2", "volume": 53, "mute": 0 } ] }"#,
        )
        .unwrap();
        assert_eq!(
            list,
            [Follower {
                name: "Kitchen".into(),
                uuid: "89ABCDEF0123456789ABCDEF".into(),
                ip: "10.0.0.41".into(),
                volume: Some(53),
                muted: Some(false),
            }]
        );
        assert_eq!(
            parse_followers(r#"{ "slaves": 0, "wmrm_version": "4.2" }"#),
            Some(vec![])
        );
    }
}
