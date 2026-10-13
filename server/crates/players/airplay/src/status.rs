//! `cliairplay`'s machine-readable output: `[STATUS]` and `[EVENT]` lines on stdout and stderr
//! (its `DESIGN.md` §12). Unknown lines, and unknown keys on known ones, are ignored, so a
//! newer binary that says more still parses.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Route {
        protocol: String,
        flow: String,
        timing: String,
    },
    Connected,
    ClockReady {
        mode: String,
        state: String,
        ready_at_unix_ms: Option<u64>,
    },
    /// The feed has a complete packet buffered: a START can be commanded.
    Audio {
        buffered_ms: u64,
    },
    /// The ack of a START: when it was asked for and when it will really be audible.
    Started {
        requested_unix_ms: u64,
        at_unix_ms: u64,
    },
    Flushed {
        head_unix_ms: Option<u64>,
    },
    Latency {
        lead_ms: Option<u64>,
        warm_lead_ms: Option<u64>,
    },
    Reanchor {
        shifted_frames: u64,
    },
    Playing {
        elapsed_ms: Option<u64>,
    },
    Paused {
        elapsed_ms: Option<u64>,
    },
    Stopped,
    Eof,
    IdleTimeout,
    Error {
        code: String,
        http: Option<u16>,
        detail: String,
    },
    /// A button pressed on the speaker or its remote.
    Remote {
        command: String,
    },
}

impl Status {
    /// Whether the process ends after this line.
    pub fn is_terminal(&self) -> bool {
        match self {
            Status::Stopped | Status::IdleTimeout => true,
            Status::Error { code, .. } => {
                matches!(
                    code.as_str(),
                    "auth_required" | "auth_failed" | "connect_failed"
                )
            }
            _ => false,
        }
    }
}

/// Splits `key=value key="quoted value"` pairs.
fn fields(text: &str) -> HashMap<&str, String> {
    let mut fields = HashMap::new();
    let mut rest = text.trim();
    while let Some(eq) = rest.find('=') {
        let key = rest[..eq].trim();
        let after = &rest[eq + 1..];
        let (value, next) = if let Some(quoted) = after.strip_prefix('"') {
            match quoted.find('"') {
                Some(end) => (quoted[..end].to_owned(), &quoted[end + 1..]),
                None => (quoted.to_owned(), ""),
            }
        } else {
            match after.find(' ') {
                Some(end) => (after[..end].to_owned(), &after[end..]),
                None => (after.to_owned(), ""),
            }
        };
        // A key is the last word before `=`.
        let key = key.rsplit(' ').next().unwrap_or(key);
        fields.insert(key, value);
        rest = next.trim_start();
    }
    fields
}

pub fn parse(line: &str) -> Option<Status> {
    let line = line.trim();
    if let Some(event) = line.strip_prefix("[EVENT] remote ") {
        return Some(Status::Remote {
            command: fields(event).get("command")?.clone(),
        });
    }
    let body = line.strip_prefix("[STATUS] ")?;
    let (kind, rest) = body.split_once(' ').unwrap_or((body, ""));
    let fields = fields(rest);
    let text = |key: &str| fields.get(key).cloned().unwrap_or_default();
    let number = |key: &str| fields.get(key).and_then(|v| v.parse::<u64>().ok());
    Some(match kind {
        "route" => Status::Route {
            protocol: text("protocol"),
            flow: text("flow"),
            timing: text("timing"),
        },
        "connected" => Status::Connected,
        "clock_ready" => Status::ClockReady {
            mode: text("mode"),
            state: text("state"),
            ready_at_unix_ms: number("ready_at_unix_ms").filter(|&ms| ms > 0),
        },
        "audio" => Status::Audio {
            buffered_ms: number("buffered_ms").unwrap_or(0),
        },
        "started" => Status::Started {
            requested_unix_ms: number("requested_unix_ms")?,
            at_unix_ms: number("at_unix_ms")?,
        },
        "flushed" => Status::Flushed {
            head_unix_ms: number("head_unix_ms"),
        },
        "latency" => Status::Latency {
            lead_ms: number("lead_ms"),
            warm_lead_ms: number("warm_lead_ms"),
        },
        "REANCHOR" => Status::Reanchor {
            shifted_frames: number("shifted_frames").unwrap_or(0),
        },
        "playing" => Status::Playing {
            elapsed_ms: number("elapsed_ms"),
        },
        "paused" => Status::Paused {
            elapsed_ms: number("elapsed_ms"),
        },
        "stopped" => Status::Stopped,
        "eof" => Status::Eof,
        "idle_timeout" => Status::IdleTimeout,
        "error" => Status::Error {
            code: text("code"),
            http: fields.get("http").and_then(|v| v.parse().ok()),
            detail: text("detail"),
        },
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_from_the_contract() {
        let cases = [
            (
                "[STATUS] route protocol=airplay2 flow=native timing=ptp buffered=0",
                Status::Route {
                    protocol: "airplay2".into(),
                    flow: "native".into(),
                    timing: "ptp".into(),
                },
            ),
            ("[STATUS] connected", Status::Connected),
            (
                "[STATUS] clock_ready mode=ptp state=probing streak_ms=120 exchanges=3 ready_in_ms=2100 ready_at_unix_ms=1791930000123",
                Status::ClockReady {
                    mode: "ptp".into(),
                    state: "probing".into(),
                    ready_at_unix_ms: Some(1_791_930_000_123),
                },
            ),
            (
                "[STATUS] clock_ready mode=ntp state=cold streak_ms=0 exchanges=0 ready_in_ms=0 ready_at_unix_ms=0",
                Status::ClockReady {
                    mode: "ntp".into(),
                    state: "cold".into(),
                    ready_at_unix_ms: None,
                },
            ),
            (
                "[STATUS] audio buffered_ms=40",
                Status::Audio { buffered_ms: 40 },
            ),
            (
                "[STATUS] started requested_unix_ms=1000 at_unix_ms=1250",
                Status::Started {
                    requested_unix_ms: 1000,
                    at_unix_ms: 1250,
                },
            ),
            ("[STATUS] flushed", Status::Flushed { head_unix_ms: None }),
            (
                "[STATUS] flushed head_unix_ms=4000",
                Status::Flushed {
                    head_unix_ms: Some(4000),
                },
            ),
            (
                "[STATUS] latency lead_ms=2000 device_min_frames=11025 device_max_frames=88200 device_render_ms=80 warm_lead_ms=600",
                Status::Latency {
                    lead_ms: Some(2000),
                    warm_lead_ms: Some(600),
                },
            ),
            (
                "[STATUS] REANCHOR shifted_frames=441 total_shifted_frames=882 sample_rate=44100",
                Status::Reanchor {
                    shifted_frames: 441,
                },
            ),
            (
                "[STATUS] error code=auth_required http=401 detail=\"password needed, none given\"",
                Status::Error {
                    code: "auth_required".into(),
                    http: Some(401),
                    detail: "password needed, none given".into(),
                },
            ),
            ("[STATUS] eof", Status::Eof),
            ("[STATUS] idle_timeout", Status::IdleTimeout),
            (
                "[EVENT] remote command=nextitem",
                Status::Remote {
                    command: "nextitem".into(),
                },
            ),
        ];
        for (line, expected) in cases {
            assert_eq!(parse(line), Some(expected), "{line}");
        }
    }

    #[test]
    fn other_output_is_ignored() {
        assert_eq!(parse("[12:00:00.000] ap2_connect:123 connecting"), None);
        assert_eq!(parse("[STATUS] something_new key=1"), None);
        assert_eq!(
            parse("[STATUS] started requested_unix_ms=1"),
            None,
            "incomplete"
        );
        assert_eq!(parse("CREDENTIALS: abc"), None);
    }

    #[test]
    fn connect_and_auth_errors_end_the_process() {
        let error = |code: &str| Status::Error {
            code: code.into(),
            http: None,
            detail: String::new(),
        };
        assert!(error("auth_failed").is_terminal());
        assert!(!error("flush_failed").is_terminal());
        assert!(Status::Stopped.is_terminal());
    }
}
