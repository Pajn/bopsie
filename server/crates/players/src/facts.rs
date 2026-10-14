//! What an endpoint is and what it can do: the facts linking and quirk rules match on, and the
//! capabilities the planner and the API read.

use std::net::IpAddr;

use bopsie_audio::OutputFormat;
use bopsie_audio::timeline::Micros;
pub use bopsie_core::quirks::Pairing;

/// What discovery learned about an endpoint, beyond its [`crate::DeviceInfo`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    /// The hardware address, as lowercase hex digits without separators.
    pub mac: Option<String>,
    pub serial: Option<String>,
    pub uuid: Option<String>,
    /// Where the endpoint was reached.
    pub address: Option<IpAddr>,
    /// AirPlay's feature bits.
    pub features: Option<u64>,
    /// AirPlay's status flags.
    pub status_flags: Option<u64>,
}

/// What an endpoint can do over its protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    /// The PCM formats it takes, in order of preference.
    pub formats: Vec<OutputFormat>,
    /// How far ahead of now playback must start for this endpoint.
    pub lead: Micros,
    /// Whether it can play in step with others in a group over its protocol.
    pub sync_group: bool,
    /// The most endpoints a group over its protocol may have, if that is limited.
    pub max_group_size: Option<usize>,
    pub volume: bool,
    pub mute: bool,
    pub pairing: Pairing,
    /// The vendor multiroom it can group by, where the devices themselves carry the audio.
    pub native_group: Option<NativeGroup>,
}

/// A vendor's own multiroom: devices group with others of the same family and version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeGroup {
    /// Such as `linkplay`.
    pub family: &'static str,
    /// Devices group only within one version.
    pub version: u32,
}
