//! Checks what a Spotify account can do through librespot: search and the saved library over
//! Spotify's Web API with librespot's own token (whose shared quota is often used up), the
//! playlists over librespot's protocol, and fetching, decrypting, decoding and seeking a
//! track. The first run announces a "Bopsie probe sign-in" device to
//! pick in the Spotify app; the login is kept in the directory given, for later runs.
//!
//! ```sh
//! cargo run -p bopsie-sources-spotify --example spotify-catalog-probe -- /tmp/spotify-probe "query"
//! ```

use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

use bopsie_audio::decode::{Decoder, Read as Decoded, SymphoniaDecoder};
use futures::StreamExt;
use librespot_audio::{AudioDecrypt, AudioFile};
use librespot_core::cache::Cache;
use librespot_core::config::DeviceType;
use librespot_core::{Session, SessionConfig, SpotifyId, SpotifyUri};
use librespot_discovery::Discovery;
use librespot_metadata::audio::AudioFileFormat;
use librespot_metadata::{Metadata, Track};
use serde_json::Value;

/// Spotify's own header before the Ogg stream.
const HEADER: u64 = 0xa7;

/// The part of a stream after Spotify's header, as a seekable source.
struct Subfile<T>(Mutex<T>, u64);

impl<T: Read + Seek> Read for Subfile<T> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.get_mut().unwrap().read(buf)
    }
}

impl<T: Read + Seek> Seek for Subfile<T> {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        let to = match to {
            SeekFrom::Start(at) => SeekFrom::Start(at + HEADER),
            other => other,
        };
        Ok(self.0.get_mut().unwrap().seek(to)? - HEADER)
    }
}

impl<T: Read + Seek + Send> symphonia::core::io::MediaSource for Subfile<T> {
    fn is_seekable(&self) -> bool {
        true
    }
    fn byte_len(&self) -> Option<u64> {
        Some(self.1 - HEADER)
    }
}

async fn web(session: &Session, path: &str) -> (u16, Value) {
    let token = session.login5().auth_token().await.expect("a token");
    let response = reqwest::Client::new()
        .get(format!("https://api.spotify.com/v1/{path}"))
        .bearer_auth(&token.access_token)
        .send()
        .await
        .expect("an answer");
    let status = response.status().as_u16();
    if status == 429 {
        println!(
            "  {path}: retry after {:?}",
            response.headers().get("retry-after")
        );
    }
    (status, response.json().await.unwrap_or(Value::Null))
}

fn count(body: &Value) -> String {
    match (body["total"].as_u64(), body["items"].as_array()) {
        (Some(total), Some(items)) => format!("{} of {total}", items.len()),
        _ => body.to_string().chars().take(200).collect(),
    }
}

#[tokio::main]
async fn main() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,libmdns=error".into()),
        )
        .init();
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(
        args.next()
            .unwrap_or_else(|| "/tmp/spotify-probe".to_owned()),
    );
    let query = args.next().unwrap_or_else(|| "Massive Attack".to_owned());
    let cache = Cache::new(Some(&dir), None, None, None).expect("a cache");
    let config = SessionConfig::default();

    let credentials = match cache.credentials() {
        Some(credentials) => credentials,
        None => {
            let mut discovery =
                Discovery::builder(config.device_id.clone(), config.client_id.clone())
                    .name("Bopsie probe sign-in")
                    .device_type(DeviceType::Computer)
                    .launch()
                    .expect("announced");
            println!("pick \"Bopsie probe sign-in\" in the Spotify app");
            discovery.next().await.expect("credentials")
        }
    };
    let session = Session::new(config, Some(cache));
    session.connect(credentials, true).await.expect("connected");
    println!("signed in as {}", session.username());

    let token = session.login5().auth_token().await;
    println!(
        "login5 token: {:?}",
        token
            .as_ref()
            .map(|t| t.scopes.clone())
            .map_err(|e| e.to_string())
    );
    let search = format!(
        "search?q={}&type=track,album,artist,playlist&limit=10",
        query.replace(' ', "%20")
    );
    let (status, found) = web(&session, &search).await;
    println!("search: {status} tracks {}", count(&found["tracks"]));
    for (path, what) in [
        ("me/tracks?limit=50", "liked songs"),
        ("me/albums?limit=50", "saved albums"),
        ("me/following?type=artist&limit=50", "followed artists"),
    ] {
        let (status, body) = web(&session, path).await;
        let body = if what == "followed artists" {
            body["artists"].clone()
        } else {
            body
        };
        println!("{what}: {status} {}", count(&body));
    }
    let (status, playlists) = web(&session, "me/playlists?limit=50").await;
    println!("playlists: {status} {}", count(&playlists));
    let user = session.username();
    let mut owned = None;
    let mut followed = None;
    for playlist in playlists["items"].as_array().into_iter().flatten() {
        let mine = playlist["owner"]["id"].as_str() == Some(user.as_str());
        let slot = if mine { &mut owned } else { &mut followed };
        if slot.is_none() {
            *slot = playlist["id"].as_str().map(str::to_owned);
        }
    }
    for (what, id) in [("own playlist", owned), ("followed playlist", followed)] {
        let Some(id) = id else {
            println!("{what}: none");
            continue;
        };
        let (status, items) = web(&session, &format!("playlists/{id}/items?limit=5")).await;
        println!("{what} items over the Web API: {status} {}", count(&items));
        let uri = SpotifyUri::from_uri(&format!("spotify:playlist:{id}")).unwrap();
        match librespot_metadata::Playlist::get(&session, &uri).await {
            Ok(list) => println!("{what} over spclient: {} items", list.tracks().count()),
            Err(error) => println!("{what} over spclient: {error}"),
        }
    }

    // Your playlists over spclient, which the Web API's quota does not cover.
    match session.spclient().get_rootlist(0, None).await {
        Ok(bytes) => {
            use protobuf::Message;
            let list =
                librespot_protocol::playlist4_external::SelectedListContent::parse_from_bytes(
                    &bytes,
                );
            match list {
                Ok(list) => {
                    let uris: Vec<String> = list
                        .contents
                        .items
                        .iter()
                        .map(|item| item.uri().to_owned())
                        .filter(|uri| uri.starts_with("spotify:playlist:"))
                        .collect();
                    println!("rootlist over spclient: {} playlists", uris.len());
                    if let Some(first) = uris.first() {
                        let uri = SpotifyUri::from_uri(first).unwrap();
                        match librespot_metadata::Playlist::get(&session, &uri).await {
                            Ok(list) => println!(
                                "  {} over spclient: {} tracks",
                                list.attributes.name,
                                list.tracks().count()
                            ),
                            Err(error) => println!("  first playlist: {error}"),
                        }
                    }
                }
                Err(error) => println!("rootlist unreadable: {error}"),
            }
        }
        Err(error) => println!("rootlist over spclient: {error}"),
    }

    // A track known to play, when the search said nothing.
    let track = found["tracks"]["items"][0]["id"]
        .as_str()
        .unwrap_or("3eDYzM3QrzbKGze39nyRYU");
    let id = SpotifyId::from_base62(track).unwrap();
    let uri = SpotifyUri::Track { id };
    let started = Instant::now();
    let track = Track::get(&session, &uri).await.expect("track metadata");
    println!("playing {} ({} formats)", track.name, track.files.len());
    let format = [
        AudioFileFormat::OGG_VORBIS_320,
        AudioFileFormat::OGG_VORBIS_160,
        AudioFileFormat::OGG_VORBIS_96,
    ]
    .into_iter()
    .find(|format| track.files.contains_key(format))
    .expect("an Ogg Vorbis file");
    let file_id = track.files[&format];
    let key = session.audio_key().request(id, file_id).await;
    println!(
        "{format:?} key: {}",
        if key.is_ok() { "ok" } else { "refused" }
    );
    let file = AudioFile::open(&session, file_id, 40 * 1024)
        .await
        .expect("the file");
    let length = file
        .get_stream_loader_controller()
        .map(|c| c.len() as u64)
        .unwrap_or(0);
    let decoded = tokio::task::spawn_blocking(move || {
        let mut decrypted = AudioDecrypt::new(key.ok(), file);
        decrypted.seek(SeekFrom::Start(HEADER)).unwrap();
        let source = Subfile(Mutex::new(decrypted), length);
        let mut decoder =
            SymphoniaDecoder::from_source(Box::new(source), Some("ogg")).expect("decodable");
        let format = decoder.format();
        let mut frames = 0;
        let mut out = Vec::new();
        while frames < u64::from(format.sample_rate) * 5 {
            out.clear();
            if decoder.read(&mut out).expect("decoded") == Decoded::End {
                break;
            }
            frames += (out.len() / usize::from(format.channels)) as u64;
        }
        let seeked = decoder.seek(u64::from(format.sample_rate) * 60).ok();
        (format, frames, seeked)
    })
    .await
    .unwrap();
    println!(
        "decoded {:?}: {} frames, seek to 60 s gave {:?}, in {:?}",
        decoded.0,
        decoded.1,
        decoded.2,
        started.elapsed()
    );
}
