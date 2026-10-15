//! The group planner: how a group plays, and which of its devices it can serve. A group plays
//! over one protocol, through one endpoint per device, at one format, with one lead; a device
//! that cannot play that way is dropped, with the reason.

use std::fmt;

use bopsie_audio::OutputFormat;
use bopsie_audio::timeline::Micros;

use crate::Device;
use crate::devices::Member;
use crate::quirks::SampleCap;

/// How a group's audio reaches its devices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    /// One `cliairplay` stream per device, on the shared PTP clock.
    AirplayDirect,
    /// One Sendspin stream per device, on the server's clock.
    SendspinDirect,
    /// The server streams to the leader, and the leader's own multiroom, such as LinkPlay's,
    /// carries the others.
    Native,
}

impl Strategy {
    /// The protocol every device plays over, when there is one.
    pub fn protocol(self) -> Option<&'static str> {
        match self {
            Self::AirplayDirect => Some("airplay"),
            Self::SendspinDirect => Some("sendspin"),
            Self::Native => None,
        }
    }

    fn for_protocol(protocol: &str) -> Option<Self> {
        match protocol {
            "airplay" => Some(Self::AirplayDirect),
            "sendspin" => Some(Self::SendspinDirect),
            _ => None,
        }
    }
}

/// What the planner may choose from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    /// The protocols a group may play over, in order of preference.
    pub protocols: Vec<String>,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            protocols: vec!["airplay".to_owned(), "sendspin".to_owned()],
        }
    }
}

/// One device's part in a plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Carrier {
    pub device: String,
    /// The endpoint that plays for it.
    pub endpoint: String,
    /// The format its output takes: the group's rate and channels, at the bit depth it
    /// prefers.
    pub format: OutputFormat,
    /// It follows the leader in its vendor's multiroom: the server streams only to the leader.
    pub native: bool,
}

/// Why a device cannot play in a group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    Unavailable,
    /// It plays alone only.
    CannotGroup,
    /// It cannot play over the group's protocol.
    NoCommonProtocol {
        protocol: &'static str,
    },
    /// It cannot play the group's rate and channels.
    NoCommonFormat {
        sample_rate: u32,
        channels: u16,
    },
    /// The group's protocol has no room for it.
    GroupFull {
        max: usize,
    },
    /// The leader's own multiroom cannot carry it: another vendor's, or another version.
    NotNativeCompatible {
        family: &'static str,
    },
}

fn protocol_name(protocol: &str) -> &str {
    match protocol {
        "airplay" => "AirPlay",
        "sendspin" => "Sendspin",
        "linkplay" => "LinkPlay",
        other => other,
    }
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => write!(f, "it isn't available"),
            Self::CannotGroup => write!(f, "it can only play on its own"),
            Self::NoCommonProtocol { protocol } => write!(
                f,
                "the group plays over {} and it doesn't",
                protocol_name(protocol)
            ),
            Self::NoCommonFormat {
                sample_rate,
                channels,
            } => write!(
                f,
                "it can't play the group's {sample_rate} Hz {channels}-channel audio"
            ),
            Self::GroupFull { max } => write!(f, "the group already has {max} speakers"),
            Self::NotNativeCompatible { family } => {
                write!(f, "it can't join a {} group", protocol_name(family))
            }
        }
    }
}

/// A device the plan leaves out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dropped {
    pub device: String,
    pub name: String,
    pub reason: Reason,
}

impl Dropped {
    /// What to tell the user, for a group led by `leader`.
    pub fn message(&self, leader: &str) -> String {
        format!("{} can't join {leader}: {}", self.name, self.reason)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub strategy: Strategy,
    /// The leader first, then the members it serves, in the order given.
    pub carriers: Vec<Carrier>,
    /// The group's rate and channels; each carrier's format has its own bit depth.
    pub sample_rate: u32,
    pub channels: u16,
    /// How far ahead of now playback starts: the most any carrier needs.
    pub lead: Micros,
    /// Members it cannot serve.
    pub dropped: Vec<Dropped>,
}

impl Plan {
    pub fn carrier(&self, device: &str) -> Option<&Carrier> {
        self.carriers.iter().find(|c| c.device == device)
    }

    /// The carriers the server streams to: all but those the leader carries natively.
    pub fn streamed(&self) -> impl Iterator<Item = &Carrier> {
        self.carriers.iter().filter(|c| !c.native)
    }
}

/// The most a native group carries: LinkPlay groups play at 48 kHz 16-bit at most.
const NATIVE_CAP: SampleCap = SampleCap {
    sample_rate: 48_000,
    bits: 16,
};

/// Whether `format` is within a `max_sample_format` quirk.
fn within(format: &OutputFormat, cap: Option<SampleCap>) -> bool {
    cap.is_none_or(|cap| format.sample_rate <= cap.sample_rate && format.bits <= cap.bits)
}

/// The formats `member` really plays, in its order of preference.
fn formats(member: &Member) -> impl Iterator<Item = &OutputFormat> {
    let cap = member.state.quirks.quirks.max_sample_format;
    member
        .state
        .capabilities
        .formats
        .iter()
        .filter(move |f| within(f, cap))
}

/// `device`'s healthiest available endpoint over `protocol`, its preferred among equals.
fn endpoint_for<'a>(device: &'a Device, protocol: &str) -> Option<&'a Member> {
    device
        .members
        .iter()
        .filter(|m| m.state.provider == protocol && m.state.available)
        .min_by_key(|m| m.health.level)
}

fn lead(member: &Member) -> Micros {
    let floor = member
        .state
        .quirks
        .quirks
        .min_lead_ms
        .map_or(0, |ms| Micros::from(ms) * 1_000);
    member.state.capabilities.lead.max(floor)
}

/// How `leader` and `members` play together, preferring `current`'s strategy when it serves
/// as many: the strategy that serves the most members, then the one that launches the fewest
/// endpoints, earliest in `policy` among equals, and the leader's own multiroom after those.
/// `None` when the leader cannot play at all.
pub fn plan(
    leader: &Device,
    members: &[Device],
    policy: &Policy,
    current: Option<Strategy>,
) -> Option<Plan> {
    if !leader.available() {
        return None;
    }
    let mut best: Option<Plan> = None;
    let candidates = policy
        .protocols
        .iter()
        .filter_map(|p| Strategy::for_protocol(p))
        .map(|strategy| (strategy, plan_with(strategy, leader, members)))
        .chain(std::iter::once((
            Strategy::Native,
            plan_native(leader, members),
        )));
    for (strategy, candidate) in candidates {
        let Some(candidate) = candidate else {
            continue;
        };
        let better = match &best {
            None => true,
            Some(best) => {
                candidate.carriers.len() > best.carriers.len()
                    || (candidate.carriers.len() == best.carriers.len()
                        && if current == Some(strategy) {
                            best.strategy != strategy
                        } else {
                            current != Some(best.strategy)
                                && launches(&candidate, leader, members)
                                    < launches(best, leader, members)
                        })
            }
        };
        if better {
            best = Some(candidate);
        }
    }
    best
}

/// How many of the endpoints `plan` streams to have to be launched first.
fn launches(plan: &Plan, leader: &Device, members: &[Device]) -> usize {
    plan.streamed()
        .filter(|carrier| {
            std::iter::once(leader)
                .chain(members)
                .find(|device| device.id == carrier.device)
                .and_then(|device| device.member(&carrier.endpoint))
                .is_some_and(|member| member.state.capabilities.launched)
        })
        .count()
}

fn plan_with(strategy: Strategy, leader: &Device, members: &[Device]) -> Option<Plan> {
    let protocol = strategy.protocol()?;
    let head = endpoint_for(leader, protocol)?;
    let grouped = !members.is_empty();
    if grouped && !head.state.capabilities.sync_group {
        return None;
    }

    // Which members could play over the protocol at all.
    let mut dropped = Vec::new();
    let mut able: Vec<(&Device, &Member)> = Vec::new();
    for device in members {
        let drop = |reason| Dropped {
            device: device.id.clone(),
            name: device.name.clone(),
            reason,
        };
        if !device.available() {
            dropped.push(drop(Reason::Unavailable));
            continue;
        }
        let Some(member) = endpoint_for(device, protocol) else {
            dropped.push(drop(Reason::NoCommonProtocol { protocol }));
            continue;
        };
        if !member.state.capabilities.sync_group {
            dropped.push(drop(Reason::CannotGroup));
            continue;
        }
        able.push((device, member));
    }

    // The leader's format that the most of them can play, its preferred among equals.
    let (format, _) = formats(head)
        .map(|format| {
            let fits = able
                .iter()
                .filter(|(_, m)| matching(m, format).is_some())
                .count();
            (*format, fits)
        })
        .fold(
            None,
            |best: Option<(OutputFormat, usize)>, (format, fits)| match best {
                Some((_, most)) if most >= fits => best,
                _ => Some((format, fits)),
            },
        )?;

    let mut carriers = vec![Carrier {
        device: leader.id.clone(),
        endpoint: head.id.clone(),
        format,
        native: false,
    }];
    let mut max = head.state.capabilities.max_group_size;
    let mut lead_needed = lead(head);
    for (device, member) in able {
        let drop = |reason| Dropped {
            device: device.id.clone(),
            name: device.name.clone(),
            reason,
        };
        let Some(own) = matching(member, &format) else {
            dropped.push(drop(Reason::NoCommonFormat {
                sample_rate: format.sample_rate,
                channels: format.channels,
            }));
            continue;
        };
        let limit = [max, member.state.capabilities.max_group_size]
            .into_iter()
            .flatten()
            .min();
        if limit.is_some_and(|limit| carriers.len() >= limit) {
            dropped.push(drop(Reason::GroupFull {
                max: limit.unwrap_or_default(),
            }));
            continue;
        }
        max = limit;
        lead_needed = lead_needed.max(lead(member));
        carriers.push(Carrier {
            device: device.id.clone(),
            endpoint: member.id.clone(),
            format: own,
            native: false,
        });
    }
    // Dropped members in the order they were given.
    dropped.sort_by_key(|d| members.iter().position(|m| m.id == d.device));
    Some(Plan {
        strategy,
        carriers,
        sample_rate: format.sample_rate,
        channels: format.channels,
        lead: lead_needed,
        dropped,
    })
}

/// `device`'s healthiest available endpoint with a native multiroom.
fn native_endpoint(device: &Device) -> Option<&Member> {
    device
        .members
        .iter()
        .filter(|m| m.state.available && m.state.capabilities.native_group.is_some())
        .min_by_key(|m| m.health.level)
}

/// The leader streamed to, carrying every member its own multiroom can.
fn plan_native(leader: &Device, members: &[Device]) -> Option<Plan> {
    let head = native_endpoint(leader)?;
    let native = head.state.capabilities.native_group?;
    let format = *formats(head).find(|f| within(f, Some(NATIVE_CAP)))?;
    let mut carriers = vec![Carrier {
        device: leader.id.clone(),
        endpoint: head.id.clone(),
        format,
        native: false,
    }];
    let mut dropped = Vec::new();
    for device in members {
        let drop = |reason| Dropped {
            device: device.id.clone(),
            name: device.name.clone(),
            reason,
        };
        if !device.available() {
            dropped.push(drop(Reason::Unavailable));
            continue;
        }
        let member = device
            .members
            .iter()
            .find(|m| m.state.available && m.state.capabilities.native_group == Some(native));
        let Some(member) = member else {
            dropped.push(drop(Reason::NotNativeCompatible {
                family: native.family,
            }));
            continue;
        };
        carriers.push(Carrier {
            device: device.id.clone(),
            endpoint: member.id.clone(),
            format,
            native: true,
        });
    }
    Some(Plan {
        strategy: Strategy::Native,
        carriers,
        sample_rate: format.sample_rate,
        channels: format.channels,
        lead: lead(head),
        dropped,
    })
}

/// `member`'s preferred format at `format`'s rate and channels.
fn matching(member: &Member, format: &OutputFormat) -> Option<OutputFormat> {
    formats(member)
        .find(|f| f.sample_rate == format.sample_rate && f.channels == format.channels)
        .copied()
}
