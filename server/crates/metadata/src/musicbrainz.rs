//! MusicBrainz's web service: one request a second for everything that calls it, with the
//! User-Agent it asks for, waiting when it is busy, and answers kept for a month.
//! <https://musicbrainz.org/doc/MusicBrainz_API/Rate_Limiting>

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bopsie_core::db::Database;
use bopsie_core::pace::Pacer;
use rusqlite::OptionalExtension;
use serde::Deserialize;
use serde_json::Value;

/// MusicBrainz's own server.
pub const BASE: &str = "https://musicbrainz.org/ws/2";
/// The Cover Art Archive's.
pub const COVER_ART_BASE: &str = "https://coverartarchive.org";
/// How long an answer is used before it is asked for again.
const FRESH: Duration = Duration::from_secs(30 * 24 * 3600);
/// Tries after MusicBrainz says it is busy, waiting longer each time.
const BUSY_RETRIES: u32 = 4;
/// What a release is looked up with: its tracks and their recordings' ISRCs, artists, release
/// group with its genres, and links to other services.
const RELEASE_INCLUDES: &str = "recordings+artist-credits+release-groups+url-rels+isrcs+genres";

#[derive(Debug, thiserror::Error)]
pub enum MbError {
    #[error("MusicBrainz is unreachable: {0}")]
    Unreachable(String),
    #[error("MusicBrainz refused: {0}")]
    Refused(String),
    #[error("MusicBrainz stayed busy")]
    Busy,
    #[error(transparent)]
    Db(#[from] bopsie_core::db::DbError),
}

#[derive(Clone)]
pub struct MusicBrainz {
    http: reqwest::Client,
    base: String,
    db: Database,
    pacer: Arc<tokio::sync::Mutex<Pacer>>,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

impl MusicBrainz {
    /// A client of the service at `base`. `contact`, an email address or URL, goes in the
    /// User-Agent, as MusicBrainz asks.
    pub fn new(db: Database, base: impl Into<String>, contact: Option<&str>) -> Self {
        let mut agent = format!("Bopsie/{}", env!("CARGO_PKG_VERSION"));
        if let Some(contact) = contact.map(str::trim).filter(|c| !c.is_empty()) {
            agent.push_str(&format!(" ( {contact} )"));
        }
        Self {
            http: reqwest::Client::builder()
                .user_agent(agent)
                .timeout(Duration::from_secs(30))
                .build()
                .expect("a client without TLS settings builds"),
            base: base.into().trim_end_matches('/').to_owned(),
            db,
            pacer: Arc::new(tokio::sync::Mutex::new(Pacer::new(
                Duration::from_secs(1),
                1,
            ))),
        }
    }

    /// Paced at one request per `pace` instead, as a stand-in for MusicBrainz allows.
    pub fn paced(mut self, pace: Duration) -> Self {
        self.pacer = Arc::new(tokio::sync::Mutex::new(Pacer::new(pace, 1)));
        self
    }

    /// The JSON at `path`, such as `release/<id>?inc=…`, or `None` when there is none.
    async fn get(&self, path: &str) -> Result<Option<Value>, MbError> {
        let separator = if path.contains('?') { '&' } else { '?' };
        let url = format!("{}/{path}{separator}fmt=json", self.base);
        let key = url.clone();
        let cached: Option<String> = self
            .db
            .call(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT body FROM metadata_cache WHERE url = ?1 AND fetched > ?2",
                        rusqlite::params![key, now() - FRESH.as_secs() as i64],
                        |row| row.get(0),
                    )
                    .optional()?)
            })
            .await?;
        if let Some(body) = cached {
            return Ok(serde_json::from_str::<Option<Value>>(&body).unwrap_or(None));
        }
        let answer = self.fetch(&url).await?;
        let body = serde_json::to_string(&answer).unwrap_or_else(|_| "null".to_owned());
        self.db
            .call(move |conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO metadata_cache (url, body, fetched) VALUES (?1, ?2, ?3)",
                    rusqlite::params![url, body, now()],
                )?;
                Ok(())
            })
            .await?;
        Ok(answer)
    }

    async fn fetch(&self, url: &str) -> Result<Option<Value>, MbError> {
        let mut busy = 0;
        loop {
            Pacer::wait(&self.pacer).await;
            let response = self
                .http
                .get(url)
                .send()
                .await
                .map_err(|e| MbError::Unreachable(e.to_string()))?;
            match response.status().as_u16() {
                200..=299 => {
                    return response
                        .json()
                        .await
                        .map(Some)
                        .map_err(|e| MbError::Refused(e.to_string()));
                }
                404 => return Ok(None),
                429 | 503 if busy < BUSY_RETRIES => {
                    let after = response
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse().ok())
                        .map_or(Duration::from_secs(2 << busy), Duration::from_secs);
                    busy += 1;
                    tracing::debug!(?after, url, "MusicBrainz is busy; waiting");
                    tokio::time::sleep(after).await;
                }
                503 | 429 => return Err(MbError::Busy),
                status => {
                    let body = response.text().await.unwrap_or_default();
                    return Err(MbError::Refused(format!("{status}: {body}")));
                }
            }
        }
    }

    /// Release `id` with its tracks, if MusicBrainz has it.
    pub async fn release(&self, id: &str) -> Result<Option<Release>, MbError> {
        let answer = self
            .get(&format!("release/{id}?inc={RELEASE_INCLUDES}"))
            .await?;
        Ok(answer.and_then(|value| serde_json::from_value(value).ok()))
    }

    /// The image at `url`, following redirects, with its file extension: `None` when there is
    /// none, or it is not a JPEG or PNG.
    pub async fn image(&self, url: &str) -> Result<Option<(Vec<u8>, &'static str)>, MbError> {
        Pacer::wait(&self.pacer).await;
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| MbError::Unreachable(e.to_string()))?;
        if !response.status().is_success() {
            return Ok(None);
        }
        let extension = match response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
        {
            Some(t) if t.starts_with("image/jpeg") => "jpg",
            Some(t) if t.starts_with("image/png") => "png",
            _ => return Ok(None),
        };
        let bytes = response
            .bytes()
            .await
            .map_err(|e| MbError::Unreachable(e.to_string()))?;
        Ok(Some((bytes.to_vec(), extension)))
    }

    /// Where artist `id` is on other services, as MusicBrainz links it.
    pub async fn artist_links(&self, id: &str) -> Result<ArtistLinks, MbError> {
        let answer = self.get(&format!("artist/{id}?inc=url-rels")).await?;
        let relations: Vec<Relation> = answer
            .and_then(|value| serde_json::from_value(value["relations"].clone()).ok())
            .unwrap_or_default();
        let after = |marker: &str| {
            relations
                .iter()
                .filter_map(|relation| relation.url.as_ref())
                .find_map(|url| {
                    let rest = url.resource.split_once(marker)?.1;
                    let id = rest.split(['?', '/', '#']).next()?;
                    (!id.is_empty()).then(|| id.to_owned())
                })
        };
        Ok(ArtistLinks {
            spotify: after("open.spotify.com/artist/"),
            wikidata: after("wikidata.org/wiki/"),
        })
    }

    /// Releases matching a search such as `release:"…" AND artist:"…"`, best first.
    pub async fn search(&self, query: &str, limit: u32) -> Result<Vec<Found>, MbError> {
        let answer = self
            .get(&format!("release?query={}&limit={limit}", encode(query)))
            .await?;
        Ok(answer
            .and_then(|value| serde_json::from_value::<Search>(value).ok())
            .map(|search| search.releases)
            .unwrap_or_default())
    }
}

/// `text` as a URL query value.
fn encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// `text` as a quoted Lucene phrase, as MusicBrainz's search reads it.
pub fn phrase(text: &str) -> String {
    let escaped: String = text
        .chars()
        .flat_map(|c| match c {
            '"' | '\\' => vec!['\\', c],
            c => vec![c],
        })
        .collect();
    format!("\"{escaped}\"")
}

#[derive(Debug, Deserialize)]
struct Search {
    #[serde(default)]
    releases: Vec<Found>,
}

/// A release as a search finds it, before its tracks are looked up.
#[derive(Debug, Clone, Deserialize)]
pub struct Found {
    pub id: String,
    #[serde(default)]
    pub score: u32,
    #[serde(default)]
    pub title: String,
    #[serde(rename = "track-count", default)]
    pub track_count: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Credit {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub joinphrase: String,
    pub artist: CreditedArtist,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreditedArtist {
    pub id: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Genre {
    pub name: String,
    #[serde(default)]
    pub count: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReleaseGroup {
    pub id: String,
    #[serde(rename = "first-release-date", default)]
    pub first_release_date: Option<String>,
    #[serde(default)]
    pub genres: Vec<Genre>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Url {
    pub resource: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Relation {
    pub url: Option<Url>,
}

/// Where an artist is on other services.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArtistLinks {
    /// Its Spotify id.
    pub spotify: Option<String>,
    /// Its Wikidata entity, such as `Q1299`.
    pub wikidata: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Recording {
    pub id: String,
    #[serde(default)]
    pub isrcs: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Track {
    pub id: String,
    #[serde(default)]
    pub position: u32,
    pub title: String,
    pub length: Option<u64>,
    pub recording: Recording,
    #[serde(rename = "artist-credit", default)]
    pub artist_credit: Vec<Credit>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Medium {
    #[serde(default)]
    pub position: u32,
    pub format: Option<String>,
    #[serde(default)]
    pub tracks: Vec<Track>,
}

/// A release, with what [`crate::score`] compares and what is kept of it.
#[derive(Debug, Clone, Deserialize)]
pub struct Release {
    pub id: String,
    pub title: String,
    pub date: Option<String>,
    pub country: Option<String>,
    pub barcode: Option<String>,
    #[serde(rename = "artist-credit", default)]
    pub artist_credit: Vec<Credit>,
    #[serde(rename = "release-group")]
    pub release_group: Option<ReleaseGroup>,
    #[serde(default)]
    pub relations: Vec<Relation>,
    #[serde(default)]
    pub media: Vec<Medium>,
}

fn year(date: Option<&str>) -> Option<i32> {
    date?.get(..4)?.parse().ok()
}

/// Credited artists' names as one, as a release prints them.
pub fn credited(credits: &[Credit]) -> String {
    credits
        .iter()
        .map(|credit| format!("{}{}", credit.name, credit.joinphrase))
        .collect()
}

impl Release {
    /// The years it came out: this release's, and its release group's first.
    pub fn years(&self) -> Vec<i32> {
        let first = self
            .release_group
            .as_ref()
            .and_then(|group| year(group.first_release_date.as_deref()));
        year(self.date.as_deref())
            .into_iter()
            .chain(first)
            .collect()
    }

    /// The Spotify albums it links to, by id.
    pub fn spotify_albums(&self) -> Vec<String> {
        self.relations
            .iter()
            .filter_map(|relation| relation.url.as_ref())
            .filter_map(|url| {
                let id = url
                    .resource
                    .split_once("open.spotify.com/album/")?
                    .1
                    .split(['?', '/'])
                    .next()?;
                (!id.is_empty()).then(|| id.to_owned())
            })
            .collect()
    }

    /// As a person reads it: "Artist – Title (2011, GB, CD)".
    pub fn describe(&self) -> String {
        let formats: Vec<&str> = self
            .media
            .iter()
            .filter_map(|medium| medium.format.as_deref())
            .collect();
        let mut formats_shown: Vec<&str> = Vec::new();
        for format in formats {
            if !formats_shown.contains(&format) {
                formats_shown.push(format);
            }
        }
        let format = formats_shown.join(" + ");
        let year = year(self.date.as_deref()).map(|y| y.to_string());
        let details: Vec<&str> = [
            year.as_deref(),
            self.country.as_deref(),
            Some(format.as_str()),
        ]
        .into_iter()
        .flatten()
        .filter(|detail| !detail.is_empty())
        .collect();
        let mut out = format!("{} – {}", credited(&self.artist_credit), self.title);
        if !details.is_empty() {
            out.push_str(&format!(" ({})", details.join(", ")));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_search_is_a_quoted_phrase_in_the_query_string() {
        assert_eq!(phrase(r#"Say "Hi" \o/"#), r#""Say \"Hi\" \\o/""#);
        assert_eq!(encode("a b&c=ö"), "a%20b%26c%3D%C3%B6");
    }
}
