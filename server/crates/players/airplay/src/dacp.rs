//! DACP: how an AirPlay speaker reports its own controls. The server announces
//! `iTunes_Ctrl_<DACP id>._dacp._tcp` over mDNS; a speaker sends what its buttons, remote or app
//! ask for as HTTP requests to it, naming the stream by the `Active-Remote` id the stream was
//! started with. As Music Assistant's (`providers/airplay/provider.py`, `_handle_dacp_request`).

use std::time::Duration;

use bopsie_players::Remote;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// The service speakers look for.
pub const SERVICE: &str = "_dacp._tcp.local.";

/// Where the server listens, the first port free in this range.
pub const PORTS: std::ops::RangeInclusive<u16> = 39831..=49831;

/// What a request asks for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Request {
    Remote(Remote),
    /// A pause some speakers send just before they say they switched source: acted on only if
    /// that does not follow.
    DiscretePause,
    /// The speaker's volume, 0 to 100, or `None` when it muted itself.
    Volume(Option<u8>),
    /// The speaker went to another source (`true`), or is ready to play again.
    PreventPlayback(bool),
}

/// AirPlay's volume in dB, -30 to 0, as 0 to 100; `None` for mute (-144).
pub fn volume_from_db(db: f64) -> Option<u8> {
    if db <= -144.0 {
        return None;
    }
    let clamped = db.clamp(-30.0, 0.0);
    Some(((clamped + 30.0) * 100.0 / 30.0).round() as u8)
}

/// What the request for `path` asks for, if anything the server acts on.
pub fn parse(path: &str) -> Option<Request> {
    let control = path.strip_prefix("/ctrl-int/1/")?;
    let (name, query) = control.split_once('?').unwrap_or((control, ""));
    Some(match name {
        "play" => Request::Remote(Remote::Play),
        "pause" => Request::Remote(Remote::Pause),
        "playpause" => Request::Remote(Remote::PlayPause),
        "stop" => Request::Remote(Remote::Stop),
        "nextitem" => Request::Remote(Remote::Next),
        "previtem" => Request::Remote(Remote::Previous),
        "volumeup" => Request::Remote(Remote::VolumeUp),
        "volumedown" => Request::Remote(Remote::VolumeDown),
        "discrete-pause" => Request::DiscretePause,
        "setproperty" => {
            if let Some(db) = query.strip_prefix("dmcp.device-volume=") {
                Request::Volume(volume_from_db(db.parse().ok()?))
            } else if let Some(volume) = query.strip_prefix("dmcp.volume=") {
                Request::Volume(Some(
                    volume.parse::<f64>().ok()?.clamp(0.0, 100.0).round() as u8
                ))
            } else {
                let prevent = query.strip_prefix("dmcp.device-prevent-playback=")?;
                Request::PreventPlayback(prevent == "1")
            }
        }
        _ => return None,
    })
}

/// The longest request read: they are a request line and a few headers.
const MAX_REQUEST: usize = 8 * 1024;
const READ_TIMEOUT: Duration = Duration::from_secs(5);

/// As Music Assistant answers, which speakers accept.
const RESPONSE: &str = "HTTP/1.0 204 No Content\r\nDAAP-Server: iTunes/7.6.2 (Windows; N;)\r\n\
Content-Type: application/x-dmap-tagged\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

/// The first port in [`PORTS`] free to listen on.
pub async fn bind() -> std::io::Result<TcpListener> {
    let mut last = None;
    for port in PORTS {
        match TcpListener::bind(("0.0.0.0", port)).await {
            Ok(listener) => return Ok(listener),
            Err(error) => last = Some(error),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("no port to listen on")))
}

/// Answers requests on `listener`, handing each to `handle` with its `Active-Remote` id and
/// path, until the listener fails.
pub async fn serve(listener: TcpListener, handle: impl Fn(&str, &str) + Send + Sync + 'static) {
    let handle = std::sync::Arc::new(handle);
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            return;
        };
        let handle = handle.clone();
        tokio::spawn(async move {
            if let Ok(Some((remote, path))) =
                tokio::time::timeout(READ_TIMEOUT, answer(stream)).await
            {
                handle(&remote, &path);
            }
        });
    }
}

/// Reads one request and answers it; its `Active-Remote` and path.
async fn answer(mut stream: TcpStream) -> Option<(String, String)> {
    let mut request = Vec::new();
    let mut buffer = [0u8; 1024];
    while !request.windows(4).any(|w| w == b"\r\n\r\n") && request.len() < MAX_REQUEST {
        let read = stream.read(&mut buffer).await.ok()?;
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..read]);
    }
    let _ = stream.write_all(RESPONSE.as_bytes()).await;
    let _ = stream.shutdown().await;
    let text = String::from_utf8_lossy(&request);
    let mut lines = text.split("\r\n");
    let path = lines.next()?.split(' ').nth(1)?.to_owned();
    let remote = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.trim().eq_ignore_ascii_case("Active-Remote"))?
        .1
        .trim()
        .to_owned();
    Some((remote, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests() {
        let cases = [
            ("/ctrl-int/1/pause", Some(Request::Remote(Remote::Pause))),
            (
                "/ctrl-int/1/playpause",
                Some(Request::Remote(Remote::PlayPause)),
            ),
            ("/ctrl-int/1/nextitem", Some(Request::Remote(Remote::Next))),
            (
                "/ctrl-int/1/previtem",
                Some(Request::Remote(Remote::Previous)),
            ),
            (
                "/ctrl-int/1/volumeup",
                Some(Request::Remote(Remote::VolumeUp)),
            ),
            ("/ctrl-int/1/discrete-pause", Some(Request::DiscretePause)),
            (
                "/ctrl-int/1/setproperty?dmcp.device-volume=-15.000000",
                Some(Request::Volume(Some(50))),
            ),
            (
                "/ctrl-int/1/setproperty?dmcp.device-volume=-144.000000",
                Some(Request::Volume(None)),
            ),
            (
                "/ctrl-int/1/setproperty?dmcp.volume=40",
                Some(Request::Volume(Some(40))),
            ),
            (
                "/ctrl-int/1/setproperty?dmcp.device-prevent-playback=1",
                Some(Request::PreventPlayback(true)),
            ),
            (
                "/ctrl-int/1/setproperty?dmcp.device-prevent-playback=0",
                Some(Request::PreventPlayback(false)),
            ),
            ("/ctrl-int/1/shuffle_songs", None),
            ("/server-info", None),
        ];
        for (path, expected) in cases {
            assert_eq!(parse(path), expected, "{path}");
        }
    }

    #[test]
    fn volumes() {
        assert_eq!(volume_from_db(0.0), Some(100));
        assert_eq!(volume_from_db(-30.0), Some(0));
        assert_eq!(volume_from_db(-40.0), Some(0));
        assert_eq!(volume_from_db(-144.0), None);
    }

    #[tokio::test]
    async fn a_request_is_answered_and_handed_on_with_its_stream() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (seen, mut heard) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(serve(listener, move |remote: &str, path: &str| {
            let _ = seen.send((remote.to_owned(), path.to_owned()));
        }));
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(
                b"GET /ctrl-int/1/pause HTTP/1.1\r\nHost: x\r\nActive-Remote: 1986535575\r\n\r\n",
            )
            .await
            .unwrap();
        let mut answer = String::new();
        stream.read_to_string(&mut answer).await.unwrap();
        assert!(answer.starts_with("HTTP/1.0 204"), "{answer}");
        assert_eq!(
            heard.recv().await.unwrap(),
            ("1986535575".to_owned(), "/ctrl-int/1/pause".to_owned())
        );
    }
}
