//! Finding a recording by its sound: a track's Chromaprint fingerprint, from `fpcalc`, looked
//! up on AcoustID, which knows the MusicBrainz recordings and releases it is on. For files whose
//! tags are too poor to find their album by. <https://acoustid.org/webservice>

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use bopsie_core::db::Database;
use bopsie_core::pace::Pacer;
use rusqlite::OptionalExtension;
use serde::Deserialize;

use crate::musicbrainz::MbError;

/// AcoustID's own server.
pub const BASE: &str = "https://api.acoustid.org/v2";
/// How much of a track is fingerprinted, in seconds, as AcoustID's lookups expect.
const LENGTH: &str = "120";

/// `fpcalc` at `path`, or on the `PATH`, if it runs.
pub fn find_fpcalc(path: Option<&Path>) -> Option<PathBuf> {
    let candidate = path.map_or_else(|| PathBuf::from("fpcalc"), Path::to_path_buf);
    std::process::Command::new(&candidate)
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
        .then_some(candidate)
}

#[derive(Debug, Deserialize)]
struct Fpcalc {
    duration: f64,
    fingerprint: String,
}

#[derive(Debug, Deserialize)]
struct Lookup {
    #[serde(default)]
    results: Vec<Result_>,
}

#[derive(Debug, Deserialize)]
struct Result_ {
    score: f64,
    #[serde(default)]
    recordings: Vec<Recording>,
}

#[derive(Debug, Deserialize)]
struct Recording {
    id: String,
    #[serde(default)]
    releases: Vec<Release>,
}

#[derive(Debug, Deserialize)]
struct Release {
    id: String,
}

/// A recording a fingerprint matched, and the releases it is on.
#[derive(Debug, Clone, PartialEq)]
pub struct Heard {
    pub score: f64,
    pub recording: String,
    pub releases: Vec<String>,
}

#[derive(Clone)]
pub struct AcoustId {
    http: reqwest::Client,
    base: String,
    key: String,
    fpcalc: PathBuf,
    db: Database,
    pacer: Arc<tokio::sync::Mutex<Pacer>>,
}

impl AcoustId {
    /// A client of the service at `base` with application key `key`, fingerprinting with
    /// `fpcalc`.
    pub fn new(db: Database, base: impl Into<String>, key: String, fpcalc: PathBuf) -> Self {
        Self {
            http: reqwest::Client::builder()
                .user_agent(format!("Bopsie/{}", env!("CARGO_PKG_VERSION")))
                .timeout(Duration::from_secs(30))
                .build()
                .expect("a client without TLS settings builds"),
            base: base.into().trim_end_matches('/').to_owned(),
            key,
            fpcalc,
            db,
            // Three requests a second, as AcoustID allows.
            pacer: Arc::new(tokio::sync::Mutex::new(Pacer::new(
                Duration::from_millis(334),
                3,
            ))),
        }
    }

    /// The file's fingerprint and length in seconds: kept from before while its stamp is the
    /// same, else from `fpcalc`. `None` when it cannot be fingerprinted.
    pub async fn fingerprint(&self, path: &Path, stamp: &str) -> Option<(u32, String)> {
        let key = path.to_string_lossy().into_owned();
        let (lookup, stamp) = (key.clone(), stamp.to_owned());
        let kept_stamp = stamp.clone();
        let kept: Option<(u32, String)> = self
            .db
            .call(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT duration, fingerprint FROM fingerprints WHERE path = ?1 AND stamp = ?2",
                        rusqlite::params![lookup, kept_stamp],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .optional()?)
            })
            .await
            .ok()
            .flatten();
        if kept.is_some() {
            return kept;
        }
        let output = tokio::process::Command::new(&self.fpcalc)
            .args(["-json", "-length", LENGTH])
            .arg(path)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .await
            .ok()?;
        if !output.status.success() {
            tracing::debug!(path = %path.display(), "fpcalc could not fingerprint a file");
            return None;
        }
        let answer: Fpcalc = serde_json::from_slice(&output.stdout).ok()?;
        let found = (answer.duration.round() as u32, answer.fingerprint);
        let kept = found.clone();
        let _ = self
            .db
            .call(move |conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO fingerprints (path, stamp, duration, fingerprint)
                     VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![key, stamp, kept.0, kept.1],
                )?;
                Ok(())
            })
            .await;
        Some(found)
    }

    /// The recordings a fingerprint matches, best first.
    pub async fn lookup(&self, duration: u32, fingerprint: &str) -> Result<Vec<Heard>, MbError> {
        Pacer::wait(&self.pacer).await;
        let duration = duration.to_string();
        let response = self
            .http
            .post(format!("{}/lookup", self.base))
            .form(&[
                ("client", self.key.as_str()),
                ("meta", "recordings releaseids"),
                ("format", "json"),
                ("duration", duration.as_str()),
                ("fingerprint", fingerprint),
            ])
            .send()
            .await
            .map_err(|e| MbError::Unreachable(e.to_string()))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(MbError::Refused(format!("AcoustID: {status}: {body}")));
        }
        let lookup: Lookup = response
            .json()
            .await
            .map_err(|e| MbError::Refused(e.to_string()))?;
        let mut heard: Vec<Heard> = lookup
            .results
            .into_iter()
            .flat_map(|result| {
                result.recordings.into_iter().map(move |recording| Heard {
                    score: result.score,
                    recording: recording.id,
                    releases: recording.releases.into_iter().map(|r| r.id).collect(),
                })
            })
            .collect();
        heard.sort_by(|a, b| b.score.total_cmp(&a.score));
        Ok(heard)
    }
}
