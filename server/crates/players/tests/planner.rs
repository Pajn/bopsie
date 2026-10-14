//! The group planner, over tables of devices.

use std::sync::Arc;

use bopsie_audio::OutputFormat;
use bopsie_audio::sink::Sink;
use bopsie_audio::timeline::Timeline;
use bopsie_core::quirks::{Quirks, Resolved, SampleCap, Source};
use bopsie_players::devices::Member;
use bopsie_players::health::{Health, Level};
use bopsie_players::planner::{Policy, Reason, Strategy, plan as make_plan};
use bopsie_players::{
    Capabilities, Device, DeviceInfo, Endpoint, EndpointState, Facts, NowPlaying,
};

struct Stub;

impl Endpoint for Stub {
    fn id(&self) -> &str {
        "stub"
    }
    fn state(&self) -> EndpointState {
        unimplemented!("the planner reads members' states")
    }
    fn output(&self, _: OutputFormat, _: Timeline) -> Box<dyn Sink> {
        unimplemented!("not played to")
    }
    fn set_volume(&self, _: u8) {}
    fn set_muted(&self, _: bool) {}
    fn set_now_playing(&self, _: Option<&NowPlaying>) {}
    fn set_group(&self, _: bool, _: &str) {}
}

const CD: OutputFormat = OutputFormat {
    sample_rate: 44_100,
    channels: 2,
    bits: 16,
};
const DAT: OutputFormat = OutputFormat {
    sample_rate: 48_000,
    channels: 2,
    bits: 16,
};
const HI_RES: OutputFormat = OutputFormat {
    sample_rate: 96_000,
    channels: 2,
    bits: 24,
};

/// An endpoint, as the planner sees it.
#[derive(Clone)]
struct Spec {
    id: &'static str,
    provider: &'static str,
    formats: Vec<OutputFormat>,
    available: bool,
    health: Level,
    quirks: Quirks,
    max_group_size: Option<usize>,
    lead_ms: i64,
}

fn airplay(id: &'static str) -> Spec {
    Spec {
        id,
        provider: "airplay",
        formats: vec![CD],
        available: true,
        health: Level::Good,
        quirks: Quirks::default(),
        max_group_size: Some(8),
        lead_ms: 2_500,
    }
}

fn sendspin(id: &'static str) -> Spec {
    Spec {
        id,
        provider: "sendspin",
        formats: vec![DAT, CD],
        available: true,
        health: Level::Good,
        quirks: Quirks::default(),
        max_group_size: None,
        lead_ms: 300,
    }
}

impl Spec {
    fn formats(mut self, formats: &[OutputFormat]) -> Self {
        self.formats = formats.to_vec();
        self
    }
    fn health(mut self, level: Level) -> Self {
        self.health = level;
        self
    }
    fn unavailable(mut self) -> Self {
        self.available = false;
        self
    }
    fn quirks(mut self, quirks: Quirks) -> Self {
        self.quirks = quirks;
        self
    }

    fn member(&self) -> Member {
        let mut resolved = Resolved::default();
        resolved.apply(&self.quirks, &Source::Config);
        Member {
            id: self.id.to_owned(),
            endpoint: Arc::new(Stub),
            state: EndpointState {
                name: self.id.to_owned(),
                provider: self.provider,
                available: self.available,
                volume: None,
                muted: None,
                device: DeviceInfo {
                    model: None,
                    manufacturer: None,
                    software_version: None,
                },
                app: false,
                owner_token: None,
                facts: Facts::default(),
                capabilities: Capabilities {
                    formats: self.formats.clone(),
                    lead: self.lead_ms * 1_000,
                    sync_group: true,
                    max_group_size: self.max_group_size,
                    volume: true,
                    mute: true,
                    pairing: Default::default(),
                    native_group: None,
                },
                quirks: resolved,
                native_leader: None,
            },
            health: Health {
                level: self.health,
                counts: Default::default(),
            },
        }
    }
}

fn device(id: &str, endpoints: &[Spec]) -> Device {
    Device {
        id: id.to_owned(),
        name: id[..1].to_uppercase() + &id[1..],
        enabled: true,
        members: endpoints.iter().map(Spec::member).collect(),
    }
}

/// Each carrier as (device, endpoint, sample rate).
fn carriers(plan: &bopsie_players::planner::Plan) -> Vec<(String, String, u32)> {
    plan.carriers
        .iter()
        .map(|c| (c.device.clone(), c.endpoint.clone(), c.format.sample_rate))
        .collect()
}

fn owned(rows: &[(&str, &str, u32)]) -> Vec<(String, String, u32)> {
    rows.iter()
        .map(|(d, e, r)| ((*d).to_owned(), (*e).to_owned(), *r))
        .collect()
}

fn policy() -> Policy {
    Policy::default()
}

#[test]
fn speakers_of_one_protocol_play_over_it() {
    let kitchen = device("kitchen", &[airplay("ap1")]);
    let bedroom = device("bedroom", &[airplay("ap2")]);
    let plan = make_plan(&kitchen, &[bedroom], &policy(), None).unwrap();
    assert_eq!(plan.strategy, Strategy::AirplayDirect);
    assert_eq!(
        carriers(&plan),
        owned(&[("kitchen", "ap1", 44_100), ("bedroom", "ap2", 44_100)])
    );
    assert!(plan.dropped.is_empty());
    assert_eq!(plan.lead, 2_500_000);

    let phone = device("phone", &[sendspin("ss1")]);
    let tablet = device("tablet", &[sendspin("ss2")]);
    let plan = make_plan(&phone, &[tablet], &policy(), None).unwrap();
    assert_eq!(plan.strategy, Strategy::SendspinDirect);
    assert_eq!(
        carriers(&plan),
        owned(&[("phone", "ss1", 48_000), ("tablet", "ss2", 48_000)])
    );
    assert_eq!(plan.lead, 300_000);
}

#[test]
fn a_device_without_the_groups_protocol_is_dropped_with_a_reason() {
    let kitchen = device("kitchen", &[airplay("ap1")]);
    let phone = device("phone", &[sendspin("ss1")]);
    let plan = make_plan(&kitchen, std::slice::from_ref(&phone), &policy(), None).unwrap();
    assert_eq!(plan.strategy, Strategy::AirplayDirect);
    assert_eq!(carriers(&plan), owned(&[("kitchen", "ap1", 44_100)]));
    assert_eq!(plan.dropped.len(), 1);
    assert_eq!(
        plan.dropped[0].reason,
        Reason::NoCommonProtocol {
            protocol: "airplay"
        }
    );
    assert_eq!(
        plan.dropped[0].message("Kitchen"),
        "Phone can't join Kitchen: the group plays over AirPlay and it doesn't"
    );

    // Led by the phone, the speaker is the one left out.
    let plan = plan_for(&phone, &[kitchen]);
    assert_eq!(plan.strategy, Strategy::SendspinDirect);
    assert_eq!(
        plan.dropped[0].reason,
        Reason::NoCommonProtocol {
            protocol: "sendspin"
        }
    );
}

fn plan_for(leader: &Device, members: &[Device]) -> bopsie_players::planner::Plan {
    make_plan(leader, members, &policy(), None).unwrap()
}

#[test]
fn a_device_over_two_protocols_plays_over_the_one_that_serves_the_most() {
    let linked = device("linked", &[airplay("ap1"), sendspin("ss1")]);
    let speaker = device("speaker", &[airplay("ap2")]);
    let phone = device("phone", &[sendspin("ss2")]);

    let plan = plan_for(&linked, std::slice::from_ref(&speaker));
    assert_eq!(plan.strategy, Strategy::AirplayDirect);
    assert_eq!(
        carriers(&plan),
        owned(&[("linked", "ap1", 44_100), ("speaker", "ap2", 44_100)])
    );

    let plan = plan_for(&linked, std::slice::from_ref(&phone));
    assert_eq!(
        plan.strategy,
        Strategy::SendspinDirect,
        "AirPlay would drop the phone"
    );
    assert_eq!(
        carriers(&plan),
        owned(&[("linked", "ss1", 48_000), ("phone", "ss2", 48_000)])
    );

    // Either serves both: the policy's order decides, unless the group already plays.
    let other = device("other", &[airplay("ap3"), sendspin("ss3")]);
    assert_eq!(
        plan_for(&linked, std::slice::from_ref(&other)).strategy,
        Strategy::AirplayDirect
    );
    assert_eq!(
        make_plan(
            &linked,
            std::slice::from_ref(&other),
            &policy(),
            Some(Strategy::SendspinDirect)
        )
        .unwrap()
        .strategy,
        Strategy::SendspinDirect
    );
    let sendspin_first = Policy {
        protocols: vec!["sendspin".into(), "airplay".into()],
    };
    assert_eq!(
        make_plan(&linked, &[other], &sendspin_first, None)
            .unwrap()
            .strategy,
        Strategy::SendspinDirect
    );
}

#[test]
fn a_device_is_reached_through_its_healthiest_endpoint() {
    let leader = device("kitchen", &[airplay("ap1")]);
    let speaker = device(
        "speaker",
        &[
            airplay("ap2").health(Level::Failing),
            airplay("ap3").health(Level::Degraded),
            airplay("ap4").unavailable(),
        ],
    );
    let plan = plan_for(&leader, &[speaker]);
    assert_eq!(plan.carrier("speaker").unwrap().endpoint, "ap3");
}

#[test]
fn the_group_plays_the_leaders_format_that_most_can_play() {
    let leader = device("phone", &[sendspin("ss1").formats(&[HI_RES, DAT, CD])]);
    let cd_only = device("old", &[sendspin("ss2").formats(&[CD])]);
    let dat = device("tablet", &[sendspin("ss3").formats(&[DAT])]);
    let plan = plan_for(&leader, &[cd_only.clone(), dat.clone()]);
    // DAT and CD each serve two of three; the leader prefers DAT.
    assert_eq!(
        carriers(&plan),
        owned(&[("phone", "ss1", 48_000), ("tablet", "ss3", 48_000)])
    );
    assert_eq!(
        plan.dropped[0].reason,
        Reason::NoCommonFormat {
            sample_rate: 48_000,
            channels: 2
        }
    );
    // With two that play CD only, CD serves more.
    let older = device("older", &[sendspin("ss4").formats(&[CD])]);
    let plan = plan_for(&leader, &[cd_only, older, dat]);
    assert_eq!(plan.sample_rate, 44_100);
    assert_eq!(plan.carriers.len(), 3);
    assert_eq!(plan.dropped[0].device, "tablet");
}

#[test]
fn a_format_quirk_caps_what_an_endpoint_is_sent() {
    let cap = Quirks {
        max_sample_format: Some(SampleCap {
            sample_rate: 44_100,
            bits: 16,
        }),
        ..Quirks::default()
    };
    let capped = device(
        "sonos",
        &[sendspin("ss1").formats(&[HI_RES, CD]).quirks(cap)],
    );
    let plan = plan_for(&capped, &[]);
    assert_eq!(carriers(&plan), owned(&[("sonos", "ss1", 44_100)]));
}

#[test]
fn a_group_holds_no_more_than_its_protocol_allows() {
    let leader = device("leader", &[airplay("ap0")]);
    let ids = ["ap1", "ap2", "ap3", "ap4", "ap5", "ap6", "ap7", "ap8"];
    let members: Vec<Device> = ids
        .iter()
        .map(|id| device(&format!("d{id}"), &[airplay(id)]))
        .collect();
    let plan = plan_for(&leader, &members);
    assert_eq!(plan.carriers.len(), 8);
    assert_eq!(plan.dropped.len(), 1);
    assert_eq!(plan.dropped[0].device, "dap8");
    assert_eq!(plan.dropped[0].reason, Reason::GroupFull { max: 8 });
}

#[test]
fn the_lead_is_the_most_any_member_needs() {
    let leader = device("phone", &[sendspin("ss1")]);
    let slow = device(
        "slow",
        &[sendspin("ss2").quirks(Quirks {
            min_lead_ms: Some(4_000),
            ..Quirks::default()
        })],
    );
    assert_eq!(plan_for(&leader, &[slow]).lead, 4_000_000);
}

#[test]
fn unavailable_devices() {
    let leader = device("kitchen", &[airplay("ap1")]);
    let gone = device("gone", &[airplay("ap2").unavailable()]);
    let plan = plan_for(&leader, std::slice::from_ref(&gone));
    assert_eq!(plan.dropped[0].reason, Reason::Unavailable);
    assert_eq!(
        plan.dropped[0].message("Kitchen"),
        "Gone can't join Kitchen: it isn't available"
    );
    assert!(make_plan(&gone, &[], &policy(), None).is_none());
}
