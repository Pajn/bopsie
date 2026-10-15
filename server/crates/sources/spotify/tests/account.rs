//! The Web API login and calls, against a stand-in for Spotify's accounts and API servers.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use bopsie_core::db::Database;
use bopsie_sources_spotify::{AccountError, Endpoints, SpotifyAccount};
use serde_json::json;
use sha2::{Digest, Sha256};

#[derive(Default)]
struct Spotify {
    /// The code challenge the listener's browser would have taken to Spotify.
    challenge: Option<String>,
    /// Access tokens handed out, newest last.
    issued: Vec<String>,
    /// Calls refused for the rate limit so far.
    limited: usize,
    /// Calls failed on Spotify's side so far.
    failed: usize,
    base: String,
}

type Shared = Arc<Mutex<Spotify>>;

fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().fold(0u32, |n, b| (n << 8) | u32::from(*b)) << (8 * (3 - chunk.len()));
        for i in 0..=chunk.len() {
            out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

async fn token(State(spotify): State<Shared>, body: String) -> Response {
    let form: HashMap<String, String> = body
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(k, v)| (k.to_owned(), v.replace("%20", " ")))
        .collect();
    let mut spotify = spotify.lock().unwrap();
    match form["grant_type"].as_str() {
        "authorization_code" => {
            let verified = spotify.challenge.as_deref()
                == Some(&base64url(&Sha256::digest(
                    form["code_verifier"].as_bytes(),
                )));
            if !verified || form["code"] != "the-code" || form["client_id"] != "my-app" {
                return (StatusCode::BAD_REQUEST, "invalid_grant").into_response();
            }
        }
        "refresh_token" if form["refresh_token"] == "refresh-1" => {}
        _ => return (StatusCode::BAD_REQUEST, "invalid_grant").into_response(),
    }
    let access = format!("access-{}", spotify.issued.len() + 1);
    spotify.issued.push(access.clone());
    axum::Json(json!({"access_token": access, "expires_in": 3600, "refresh_token": "refresh-1"}))
        .into_response()
}

fn authorized(spotify: &Spotify, headers: &HeaderMap) -> bool {
    let newest = spotify.issued.last().map(|t| format!("Bearer {t}"));
    headers.get("authorization").and_then(|v| v.to_str().ok()) == newest.as_deref()
}

async fn me(State(spotify): State<Shared>, headers: HeaderMap) -> Response {
    if !authorized(&spotify.lock().unwrap(), &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    axum::Json(json!({"id": "tester", "display_name": "Test Listener"})).into_response()
}

async fn albums(
    State(spotify): State<Shared>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let mut spotify = spotify.lock().unwrap();
    if !authorized(&spotify, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if spotify.limited == 0 {
        spotify.limited += 1;
        return (StatusCode::TOO_MANY_REQUESTS, [("retry-after", "1")]).into_response();
    }
    if spotify.failed == 0 {
        spotify.failed += 1;
        return StatusCode::BAD_GATEWAY.into_response();
    }
    let offset: usize = query.get("offset").map_or(0, |o| o.parse().unwrap());
    let all = ["a", "b", "c"];
    let page: Vec<_> = all
        .iter()
        .skip(offset)
        .take(2)
        .map(|id| json!({"album": {"id": id}}))
        .collect();
    let next = (offset + 2 < all.len())
        .then(|| format!("{}/v1/me/albums?offset={}", spotify.base, offset + 2));
    axum::Json(json!({"items": page, "next": next, "total": all.len()})).into_response()
}

async fn stand_in() -> (Endpoints, Shared) {
    let spotify = Shared::default();
    let app = Router::new()
        .route("/api/token", post(token))
        .route("/v1/me", get(me))
        .route("/v1/me/albums", get(albums))
        .with_state(spotify.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    spotify.lock().unwrap().base = base.clone();
    tokio::spawn(axum::serve(listener, app).into_future());
    (
        Endpoints {
            accounts: base.clone(),
            api: format!("{base}/v1"),
        },
        spotify,
    )
}

fn param<'a>(url: &'a str, name: &str) -> &'a str {
    url.split(['?', '&'])
        .find_map(|pair| pair.strip_prefix(&format!("{name}=")))
        .unwrap()
}

#[tokio::test]
async fn a_web_api_login_is_kept_and_its_calls_paged_paced_and_refreshed() {
    let db = Database::open_in_memory(&[]).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (endpoints, spotify) = stand_in().await;
    let account = SpotifyAccount::open(db.clone(), dir.path().into(), endpoints.clone())
        .await
        .unwrap();
    assert!(matches!(
        account.authorize_url("https://bopsie.example/spotify/callback"),
        Err(AccountError::NoClientId)
    ));
    account
        .set_client_id(Some(" my-app ".into()))
        .await
        .unwrap();

    let url = account
        .authorize_url("https://bopsie.example/spotify/callback")
        .unwrap();
    assert!(url.contains("client_id=my-app"), "{url}");
    assert!(url.contains("code_challenge_method=S256"), "{url}");
    assert!(
        url.contains("redirect_uri=https%3A%2F%2Fbopsie.example%2Fspotify%2Fcallback"),
        "{url}"
    );
    spotify.lock().unwrap().challenge = Some(param(&url, "code_challenge").to_owned());
    let state = param(&url, "state").to_owned();
    assert!(matches!(
        account.callback("the-code", "someone-else's").await,
        Err(AccountError::UnexpectedCallback)
    ));
    account.callback("the-code", &state).await.unwrap();
    assert_eq!(
        account.status().await.web_user.as_deref(),
        Some("Test Listener")
    );

    // Kept: the server opened again is still logged in, and refreshes its access.
    let account = SpotifyAccount::open(db.clone(), dir.path().into(), endpoints)
        .await
        .unwrap();
    assert_eq!(
        account.status().await.web_user.as_deref(),
        Some("Test Listener")
    );
    let started = Instant::now();
    let all = account
        .get_all("me/albums?offset=0", |answer| answer)
        .await
        .unwrap();
    let ids: Vec<_> = all.iter().map(|a| a["album"]["id"].clone()).collect();
    assert_eq!(ids, [json!("a"), json!("b"), json!("c")]);
    assert!(
        started.elapsed() >= Duration::from_secs(1),
        "waited out the rate limit"
    );
    assert_eq!(spotify.lock().unwrap().failed, 1, "tried again after a 502");
    assert_eq!(spotify.lock().unwrap().issued.len(), 2, "refreshed once");

    // Signing out forgets it.
    account.web_sign_out().await.unwrap();
    assert_eq!(account.status().await.web_user, None);
    assert!(matches!(
        account.get("me").await,
        Err(AccountError::NotLoggedIn)
    ));
}
