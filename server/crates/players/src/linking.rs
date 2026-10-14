//! Which endpoints are one device, and what each device is called.
//!
//! Two endpoints are one device when they share an identifier, tried as Music Assistant does:
//! MAC, then serial, then UUID, then address. An address is only trusted between different
//! protocols, since one host may serve several endpoints of one protocol, and never when it is
//! the server's own host. An app's player is its own device.

use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;

use crate::EndpointState;

/// Which endpoints, by index into `endpoints`, are one device. Every endpoint is in exactly one
/// group; groups and their members keep the order of `endpoints`.
pub fn link(endpoints: &[&EndpointState]) -> Vec<Vec<usize>> {
    let mut parent: Vec<usize> = (0..endpoints.len()).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for a in 0..endpoints.len() {
        for b in a + 1..endpoints.len() {
            if same_device(endpoints[a], endpoints[b]) {
                let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
                parent[ra.max(rb)] = ra.min(rb);
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..endpoints.len() {
        let r = root(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }
    groups.into_values().collect()
}

fn same_device(a: &EndpointState, b: &EndpointState) -> bool {
    if a.app || b.app {
        return false;
    }
    let same = |x: &Option<String>, y: &Option<String>| matches!((x, y), (Some(x), Some(y)) if !x.is_empty() && x.eq_ignore_ascii_case(y));
    same(&a.facts.mac, &b.facts.mac)
        || same(&a.facts.serial, &b.facts.serial)
        || same(&a.facts.uuid, &b.facts.uuid)
        || (a.provider != b.provider
            && matches!(
                (a.facts.address, b.facts.address),
                (Some(x), Some(y)) if x == y && trusted(x)
            ))
}

/// Whether an address can tell devices apart: not the server's own host, nor any.
fn trusted(address: IpAddr) -> bool {
    !address.is_loopback() && !address.is_unspecified()
}

/// Device ids for linked endpoints, which stay what they were. `groups` are endpoint ids, each
/// in order of protocol preference; `known` is each endpoint's device id from before. A device
/// id stays with the endpoint it was first given for: a group with that endpoint keeps it.
/// Otherwise a group takes the first device id its endpoints had that no other group has,
/// else its first endpoint's id. Returns each device id with its endpoints.
pub fn assign(
    groups: &[Vec<String>],
    known: &BTreeMap<String, String>,
) -> Vec<(String, Vec<String>)> {
    let mut ids: Vec<Option<String>> = vec![None; groups.len()];
    let mut taken = BTreeSet::new();
    // First, every device id whose own endpoint is in the group.
    for (i, group) in groups.iter().enumerate() {
        if let Some(id) = group
            .iter()
            .filter_map(|endpoint| known.get(endpoint))
            .find(|id| group.contains(id) && !taken.contains(*id))
        {
            taken.insert(id.clone());
            ids[i] = Some(id.clone());
        }
    }
    // Then any other device id the group's endpoints had, else an endpoint's own.
    for (i, group) in groups.iter().enumerate() {
        if ids[i].is_some() {
            continue;
        }
        let id = group
            .iter()
            .filter_map(|endpoint| known.get(endpoint))
            .chain(group.iter())
            .find(|id| !taken.contains(*id))
            .cloned()
            .unwrap_or_else(|| {
                let first = group.first().map_or("device", String::as_str);
                (2..)
                    .map(|n| format!("{first}-{n}"))
                    .find(|id| !taken.contains(id))
                    .expect("a free id")
            });
        taken.insert(id.clone());
        ids[i] = Some(id);
    }
    ids.into_iter()
        .flatten()
        .zip(groups.iter().cloned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Capabilities, DeviceInfo, Facts};

    fn endpoint(provider: &'static str, facts: Facts) -> EndpointState {
        EndpointState {
            name: String::new(),
            provider,
            available: true,
            volume: None,
            muted: None,
            device: DeviceInfo {
                model: None,
                manufacturer: None,
                software_version: None,
            },
            app: false,
            owner_token: None,
            facts,
            capabilities: Capabilities {
                formats: Vec::new(),
                lead: 0,
                sync_group: true,
                max_group_size: None,
                volume: true,
                mute: true,
                pairing: Default::default(),
            },
            quirks: Default::default(),
        }
    }

    fn at(address: &str) -> Facts {
        Facts {
            address: Some(address.parse().unwrap()),
            ..Facts::default()
        }
    }

    #[test]
    fn identifiers_link_endpoints() {
        let mac = |m: &str, a: &str| Facts {
            mac: Some(m.into()),
            ..at(a)
        };
        type Case = (&'static str, Vec<EndpointState>, Vec<Vec<usize>>);
        let cases: Vec<Case> = vec![
            (
                "the same MAC, in any case",
                vec![
                    endpoint("airplay", mac("aabbcc001122", "10.0.0.2")),
                    endpoint("sendspin", mac("AABBCC001122", "10.0.0.9")),
                ],
                vec![vec![0, 1]],
            ),
            (
                "different MACs at different addresses",
                vec![
                    endpoint("airplay", mac("aabbcc001122", "10.0.0.2")),
                    endpoint("airplay", mac("aabbcc001133", "10.0.0.3")),
                ],
                vec![vec![0], vec![1]],
            ),
            (
                "the same serial",
                vec![
                    endpoint(
                        "airplay",
                        Facts {
                            serial: Some("S1".into()),
                            ..Facts::default()
                        },
                    ),
                    endpoint(
                        "sendspin",
                        Facts {
                            serial: Some("S1".into()),
                            ..Facts::default()
                        },
                    ),
                ],
                vec![vec![0, 1]],
            ),
            (
                "the same UUID",
                vec![
                    endpoint(
                        "airplay",
                        Facts {
                            uuid: Some("u-1".into()),
                            ..Facts::default()
                        },
                    ),
                    endpoint(
                        "sendspin",
                        Facts {
                            uuid: Some("U-1".into()),
                            ..Facts::default()
                        },
                    ),
                ],
                vec![vec![0, 1]],
            ),
            (
                "one address, two protocols",
                vec![
                    endpoint("airplay", at("10.0.0.2")),
                    endpoint("sendspin", at("10.0.0.2")),
                ],
                vec![vec![0, 1]],
            ),
            (
                "one address, one protocol: a host serving several speakers",
                vec![
                    endpoint("airplay", at("10.0.0.2")),
                    endpoint("airplay", at("10.0.0.2")),
                ],
                vec![vec![0], vec![1]],
            ),
            (
                "the server's own host",
                vec![
                    endpoint("airplay", at("127.0.0.1")),
                    endpoint("sendspin", at("127.0.0.1")),
                ],
                vec![vec![0], vec![1]],
            ),
            (
                "empty identifiers",
                vec![
                    endpoint(
                        "airplay",
                        Facts {
                            serial: Some(String::new()),
                            ..Facts::default()
                        },
                    ),
                    endpoint(
                        "sendspin",
                        Facts {
                            serial: Some(String::new()),
                            ..Facts::default()
                        },
                    ),
                ],
                vec![vec![0], vec![1]],
            ),
            (
                "links through a third endpoint",
                vec![
                    endpoint("airplay", mac("aa", "10.0.0.2")),
                    endpoint("sendspin", at("10.0.0.2")),
                    endpoint("other", mac("AA", "10.0.0.7")),
                ],
                vec![vec![0, 1, 2]],
            ),
        ];
        for (case, endpoints, expected) in cases {
            let refs: Vec<&EndpointState> = endpoints.iter().collect();
            assert_eq!(link(&refs), expected, "{case}");
        }
    }

    #[test]
    fn an_app_is_its_own_device() {
        let mut phone = endpoint("sendspin", at("10.0.0.2"));
        phone.app = true;
        let speaker = endpoint("airplay", at("10.0.0.2"));
        assert_eq!(link(&[&phone, &speaker]), [vec![0], vec![1]]);
    }

    fn groups(groups: &[&[&str]]) -> Vec<Vec<String>> {
        groups
            .iter()
            .map(|g| g.iter().map(|s| (*s).to_owned()).collect())
            .collect()
    }

    fn known(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(e, d)| ((*e).to_owned(), (*d).to_owned()))
            .collect()
    }

    #[test]
    fn device_ids_stay_what_they_were() {
        let cases = [
            (
                "new endpoints are devices of their own",
                groups(&[&["ap1"], &["phone"]]),
                known(&[]),
                vec![("ap1", vec!["ap1"]), ("phone", vec!["phone"])],
            ),
            (
                "an endpoint joining keeps the device's id",
                groups(&[&["ap1", "ss1"]]),
                known(&[("ss1", "ss1")]),
                vec![("ss1", vec!["ap1", "ss1"])],
            ),
            (
                "two devices that link: the preferred protocol's id wins",
                groups(&[&["ap1", "ss1"]]),
                known(&[("ap1", "ap1"), ("ss1", "ss1")]),
                vec![("ap1", vec!["ap1", "ss1"])],
            ),
            (
                "a device that splits keeps its id with the endpoint it came from",
                groups(&[&["ap1"], &["ss1"]]),
                known(&[("ap1", "ss1"), ("ss1", "ss1")]),
                vec![("ap1", vec!["ap1"]), ("ss1", vec!["ss1"])],
            ),
            (
                "an endpoint whose own device went elsewhere",
                groups(&[&["ap1"], &["ss1", "ap2"]]),
                known(&[("ap1", "ap2"), ("ap2", "ap2")]),
                vec![("ap1", vec!["ap1"]), ("ap2", vec!["ss1", "ap2"])],
            ),
        ];
        for (case, groups, known, expected) in cases {
            let expected: Vec<(String, Vec<String>)> = expected
                .into_iter()
                .map(|(d, es)| (d.to_owned(), es.into_iter().map(str::to_owned).collect()))
                .collect();
            assert_eq!(assign(&groups, &known), expected, "{case}");
        }
    }

    #[test]
    fn device_ids_are_unique_whatever_was_known() {
        // Endpoint ids that collide with device ids given to others.
        let groups = groups(&[&["a"], &["b"], &["c"]]);
        let known = known(&[("a", "b"), ("b", "c"), ("c", "a")]);
        let ids: BTreeSet<String> = assign(&groups, &known)
            .into_iter()
            .map(|(d, _)| d)
            .collect();
        assert_eq!(ids.len(), 3);
    }
}
