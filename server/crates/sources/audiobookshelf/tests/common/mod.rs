//! A stand-in Audiobookshelf server: one library of three books by two authors, and one of
//! ebooks, with no audio, answering as the real one does the requests the source makes. It takes `reader`'s password `secret`,
//! and an access token only until it is told the token expired.

#![allow(dead_code)]

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde_json::{Value, json};

pub const PASSWORD: &str = "secret";

#[derive(Default)]
pub struct Stand {
    /// The access token it takes now.
    pub access: String,
    pub refresh: String,
    /// Refreshes it gave.
    pub refreshes: u32,
    /// Refuses to refresh.
    pub refuse_refresh: bool,
    /// Every request, as `METHOD path`.
    pub requests: Vec<String>,
    /// The files of the book it plays, `li_h`: their path, and how long each plays.
    pub files: Vec<(std::path::PathBuf, f64)>,
    /// Sessions it opened, by id.
    pub sessions: u32,
    /// Sessions it forgot, as a restarted server does.
    pub forgotten: Vec<String>,
    /// What it was told of listening: `sync <session>`, `close <session>` or
    /// `progress <book>`, with the body.
    pub reports: Vec<(String, Value)>,
    /// The book listened to last, ahead of the others; Snow Crash unless said.
    pub listened_last: Option<String>,
}

pub type Shared = Arc<Mutex<Stand>>;

pub fn book(id: &str, title: &str, author: (&str, &str), duration: f64) -> Value {
    json!({
        "id": id,
        "libraryId": "lib1",
        "mediaType": "book",
        "addedAt": 1_700_000_000_000u64,
        "updatedAt": 1_700_000_000_000u64,
        "media": {
            "id": format!("{id}-media"),
            "coverPath": format!("/metadata/items/{id}/cover.jpg"),
            "duration": duration,
            "metadata": {
                "title": title,
                "titleIgnorePrefix": title.trim_start_matches("The "),
                "subtitle": null,
                "authors": [{"id": author.0, "name": author.1}],
                "narrators": ["Nora Narrator"],
                "series": [{"id": "ser1", "name": "Series", "sequence": "1"}],
                "genres": ["Fantasy"],
                "publishedYear": "1937",
                "publisher": "Allen & Unwin",
                "description": "<p>A book.</p>",
                "descriptionPlain": "A book.",
                "language": "English",
            },
            "chapters": [
                {"id": 0, "start": 0.0, "end": duration / 2.0, "title": "One"},
                {"id": 1, "start": duration / 2.0, "end": duration, "title": "Two"},
            ],
            "tracks": [],
        },
    })
}

/// An ebook: a book with no audio.
pub fn ebook(id: &str, title: &str, author: (&str, &str)) -> Value {
    let mut ebook = book(id, title, author, 0.0);
    ebook["libraryId"] = json!("eb1");
    ebook["media"]["numAudioFiles"] = json!(0);
    ebook["media"]["chapters"] = json!([]);
    ebook
}

pub fn ebooks() -> Vec<Value> {
    vec![
        ebook("li_e1", "Anathem", ("aut1", "Neal Stephenson")),
        ebook("li_e2", "Cryptonomicon", ("aut1", "Neal Stephenson")),
    ]
}

/// Where listening is in each book, as `/api/me` lists it: Snow Crash listened to last, The
/// Hobbit before it, Anathem finished, and an ebook read.
pub fn media_progress(listened_last: Option<&str>) -> Value {
    let progress = |id: &str, at: f64, finished: bool, last: u64| {
        json!({
            "id": format!("prog-{id}"), "libraryItemId": id, "episodeId": null,
            "currentTime": at, "isFinished": finished, "hideFromContinueListening": false,
            "lastUpdate": last,
        })
    };
    let last = |id: &str, at: u64| {
        if listened_last == Some(id) {
            1_700_000_900_000
        } else {
            at
        }
    };
    json!([
        progress("li_h", 600.25, false, last("li_h", 1_700_000_100_000)),
        progress("li_s", 90.5, false, last("li_s", 1_700_000_200_000)),
        progress("li_a", 3_600.0, true, 1_700_000_300_000),
        progress("li_e2", 0.0, false, 1_700_000_400_000),
        {"id": "prog-ep", "libraryItemId": "pod-x", "episodeId": "ep1", "currentTime": 5.0,
         "isFinished": false, "lastUpdate": 1_700_000_500_000u64},
    ])
}

pub fn books() -> Vec<Value> {
    vec![
        book("li_a", "Anathem", ("aut1", "Neal Stephenson"), 3_600.0),
        book("li_h", "The Hobbit", ("aut2", "J. R. R. Tolkien"), 7_200.5),
        book("li_s", "Snow Crash", ("aut1", "Neal Stephenson"), 1_800.0),
    ]
}

fn authorized(stand: &Shared, headers: &HeaderMap) -> bool {
    let expected = format!("Bearer {}", stand.lock().unwrap().access);
    headers.get("authorization").and_then(|v| v.to_str().ok()) == Some(expected.as_str())
}

fn unauthorized() -> Response {
    (StatusCode::UNAUTHORIZED, "Unauthorized").into_response()
}

fn log(stand: &Shared, line: String) {
    stand.lock().unwrap().requests.push(line);
}

async fn login(
    State(stand): State<Shared>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    log(&stand, "POST /login".to_owned());
    if body["username"] != "reader" || body["password"] != PASSWORD {
        return unauthorized();
    }
    assert_eq!(
        headers.get("x-return-tokens").and_then(|v| v.to_str().ok()),
        Some("true")
    );
    let mut stand = stand.lock().unwrap();
    stand.access = "access-1".to_owned();
    stand.refresh = "refresh-1".to_owned();
    Json(json!({
        "user": {
            "id": "usr1",
            "username": "reader",
            "accessToken": stand.access,
            "refreshToken": stand.refresh,
            "token": "legacy",
        },
        "userDefaultLibraryId": "lib1",
    }))
    .into_response()
}

async fn refresh(State(stand): State<Shared>, headers: HeaderMap) -> Response {
    log(&stand, "POST /auth/refresh".to_owned());
    let mut stand = stand.lock().unwrap();
    let given = headers.get("x-refresh-token").and_then(|v| v.to_str().ok());
    if stand.refuse_refresh || given != Some(stand.refresh.as_str()) {
        return unauthorized();
    }
    stand.refreshes += 1;
    stand.access = format!("access-{}", stand.refreshes + 1);
    stand.refresh = format!("refresh-{}", stand.refreshes + 1);
    Json(json!({"user": {
        "username": "reader",
        "accessToken": stand.access,
        "refreshToken": stand.refresh,
    }}))
    .into_response()
}

async fn libraries(State(stand): State<Shared>, headers: HeaderMap) -> Response {
    log(&stand, "GET /api/libraries".to_owned());
    if !authorized(&stand, &headers) {
        return unauthorized();
    }
    Json(json!({"libraries": [
        {"id": "lib1", "name": "Audiobooks", "mediaType": "book", "displayOrder": 1,
         "icon": "audiobookshelf", "settings": {}},
        {"id": "eb1", "name": "Ebooks", "mediaType": "book", "displayOrder": 3,
         "icon": "book", "settings": {}},
        {"id": "pod1", "name": "Podcasts", "mediaType": "podcast", "displayOrder": 2,
         "icon": "podcast", "settings": {}},
    ]}))
    .into_response()
}

async fn items(
    State(stand): State<Shared>,
    headers: HeaderMap,
    Path(library): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let number = |key: &str| -> usize { query.get(key).and_then(|v| v.parse().ok()).unwrap_or(0) };
    let (limit, page) = (number("limit"), number("page"));
    let filter = query.get("filter");
    log(
        &stand,
        format!(
            "GET /api/libraries/{library}/items?limit={limit}&page={page}{}",
            filter.map(|f| format!("&filter={f}")).unwrap_or_default()
        ),
    );
    if !authorized(&stand, &headers) {
        return unauthorized();
    }
    let mut all = match library.as_str() {
        "lib1" => books(),
        "eb1" => ebooks(),
        _ => return (StatusCode::NOT_FOUND, "Not Found").into_response(),
    };
    if let Some(filter) = filter {
        assert_eq!(
            filter, "tracks.bm9uZQ==",
            "the filter for items without audio"
        );
        all.retain(|item| item["media"]["duration"] == 0.0);
    } else if library == "eb1" {
        assert_eq!(limit, 1, "the ebooks are counted, never listed");
    }
    let results: Vec<Value> = all.iter().skip(limit * page).take(limit).cloned().collect();
    Json(json!({"results": results, "total": all.len(), "limit": limit, "page": page}))
        .into_response()
}

async fn item(State(stand): State<Shared>, headers: HeaderMap, Path(id): Path<String>) -> Response {
    log(&stand, format!("GET /api/items/{id}"));
    if !authorized(&stand, &headers) {
        return unauthorized();
    }
    let Some(mut book) = books().into_iter().find(|b| b["id"] == id) else {
        return (StatusCode::NOT_FOUND, "Not Found").into_response();
    };
    if id == "li_h" {
        book["userMediaProgress"] = json!({
            "id": "prog1", "libraryItemId": id, "currentTime": 600.25, "isFinished": false,
            "progress": 0.08, "duration": 7_200.5, "lastUpdate": 1_700_000_100_000u64,
        });
    }
    Json(book).into_response()
}

async fn play(State(stand): State<Shared>, headers: HeaderMap, Path(id): Path<String>) -> Response {
    log(&stand, format!("POST /api/items/{id}/play"));
    if !authorized(&stand, &headers) {
        return unauthorized();
    }
    let mut stand = stand.lock().unwrap();
    stand.sessions += 1;
    let mut offset = 0.0;
    // Listed out of order, as nothing promises they come in it.
    let mut tracks: Vec<Value> = stand
        .files
        .iter()
        .enumerate()
        .map(|(index, (_, seconds))| {
            let track = json!({
                "index": index + 1,
                "startOffset": offset,
                "duration": seconds,
                "mimeType": "audio/wav",
                "contentUrl": format!("/s/item/{id}/{}.wav", index + 1),
            });
            offset += seconds;
            track
        })
        .collect();
    tracks.reverse();
    Json(json!({
        "id": format!("ses{}", stand.sessions),
        "libraryItemId": id,
        "duration": offset,
        "currentTime": 0,
        "audioTracks": tracks,
        "chapters": [],
    }))
    .into_response()
}

/// A book's file, in part when a range is asked for, as the real server serves files.
async fn file(
    State(stand): State<Shared>,
    headers: HeaderMap,
    Path((id, name)): Path<(String, String)>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    log(&stand, format!("GET /s/item/{id}/{name}"));
    let stand = stand.lock().unwrap();
    if query.get("token") != Some(&stand.access) {
        return unauthorized();
    }
    let index: usize = name.trim_end_matches(".wav").parse().unwrap();
    let bytes = std::fs::read(&stand.files[index - 1].0).unwrap();
    let total = bytes.len();
    let range = headers
        .get("range")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("bytes="))
        .and_then(|v| {
            let (start, end) = v.split_once('-')?;
            let start: usize = start.parse().ok()?;
            let end: usize = end.parse().unwrap_or(total - 1).min(total - 1);
            (start <= end).then_some((start, end))
        });
    match range {
        Some((start, end)) => (
            StatusCode::PARTIAL_CONTENT,
            [
                ("content-type", "audio/wav".to_owned()),
                ("accept-ranges", "bytes".to_owned()),
                ("content-range", format!("bytes {start}-{end}/{total}")),
            ],
            bytes[start..=end].to_vec(),
        )
            .into_response(),
        None => (
            [
                ("content-type", "audio/wav".to_owned()),
                ("accept-ranges", "bytes".to_owned()),
            ],
            bytes,
        )
            .into_response(),
    }
}

async fn session_report(
    State(stand): State<Shared>,
    headers: HeaderMap,
    Path((id, what)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Response {
    if !authorized(&stand, &headers) {
        return unauthorized();
    }
    let mut stand = stand.lock().unwrap();
    let known = id
        .strip_prefix("ses")
        .and_then(|n| n.parse::<u32>().ok())
        .is_some_and(|n| n <= stand.sessions)
        && !stand.forgotten.contains(&id);
    if !known {
        return (StatusCode::NOT_FOUND, "Not Found").into_response();
    }
    stand.reports.push((format!("{what} {id}"), body));
    (StatusCode::OK, "OK").into_response()
}

async fn progress(
    State(stand): State<Shared>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    if !authorized(&stand, &headers) {
        return unauthorized();
    }
    stand
        .lock()
        .unwrap()
        .reports
        .push((format!("progress {id}"), body));
    (StatusCode::OK, "OK").into_response()
}

async fn cover(State(stand): State<Shared>, Path(id): Path<String>) -> Response {
    log(&stand, format!("GET /api/items/{id}/cover"));
    (StatusCode::OK, format!("cover of {id}")).into_response()
}

async fn search(
    State(stand): State<Shared>,
    headers: HeaderMap,
    Path(library): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    log(&stand, format!("GET /api/libraries/{library}/search"));
    if !authorized(&stand, &headers) {
        return unauthorized();
    }
    let q = query.get("q").cloned().unwrap_or_default().to_lowercase();
    let title = |b: &Value| {
        b["media"]["metadata"]["title"]
            .as_str()
            .unwrap()
            .to_lowercase()
    };
    let book: Vec<Value> = books()
        .into_iter()
        .filter(|b| title(b).contains(&q))
        .map(|b| json!({"libraryItem": b, "matchKey": "title", "matchText": "x"}))
        .collect();
    let authors: Vec<Value> = [("aut1", "Neal Stephenson"), ("aut2", "J. R. R. Tolkien")]
        .into_iter()
        .filter(|(_, name)| name.to_lowercase().contains(&q))
        .map(|(id, name)| json!({"id": id, "name": name, "imagePath": null, "numBooks": 1}))
        .collect();
    Json(json!({
        "book": book, "authors": authors, "narrators": [], "series": [], "tags": [], "genres": [],
    }))
    .into_response()
}

async fn author(
    State(stand): State<Shared>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    log(&stand, format!("GET /api/authors/{id}"));
    if !authorized(&stand, &headers) {
        return unauthorized();
    }
    let name = match id.as_str() {
        "aut1" => "Neal Stephenson",
        "aut2" => "J. R. R. Tolkien",
        _ => return (StatusCode::NOT_FOUND, "Not Found").into_response(),
    };
    let mut author = json!({
        "id": id, "name": name, "description": "Writes books.",
        "imagePath": "/metadata/authors/x.jpg",
    });
    if query.get("include").map(String::as_str) == Some("items") {
        let theirs: Vec<Value> = books()
            .into_iter()
            .chain(ebooks())
            .filter(|b| b["media"]["metadata"]["authors"][0]["id"] == id)
            .collect();
        author["libraryItems"] = json!(theirs);
    }
    Json(author).into_response()
}

async fn me(State(stand): State<Shared>, headers: HeaderMap) -> Response {
    log(&stand, "GET /api/me".to_owned());
    if !authorized(&stand, &headers) {
        return unauthorized();
    }
    let last = stand.lock().unwrap().listened_last.clone();
    Json(json!({"id": "user1", "username": "reader", "mediaProgress": media_progress(last.as_deref())}))
        .into_response()
}

/// A library's shelves, as its home page shows them: what listening continues in, minified as
/// the server lists items, without progress.
async fn personalized(
    State(stand): State<Shared>,
    headers: HeaderMap,
    Path(library): Path<String>,
) -> Response {
    log(&stand, format!("GET /api/libraries/{library}/personalized"));
    if !authorized(&stand, &headers) {
        return unauthorized();
    }
    let continuing: Vec<Value> = match library.as_str() {
        // In the order the server keeps them; a finished book is not among them.
        "lib1" => books().into_iter().filter(|b| b["id"] != "li_a").collect(),
        "eb1" => ebooks()
            .into_iter()
            .filter(|b| b["id"] == "li_e2")
            .collect(),
        _ => return (StatusCode::NOT_FOUND, "Not Found").into_response(),
    };
    Json(json!([
        {"id": "continue-listening", "label": "Continue Listening", "type": "book",
         "entities": continuing},
        {"id": "recently-added", "label": "Recently Added", "type": "book",
         "entities": books()},
    ]))
    .into_response()
}

/// Starts the stand-in; answers with its address and its state.
pub async fn start() -> (String, Shared) {
    let stand: Shared = Arc::default();
    let app = Router::new()
        .route("/login", post(login))
        .route("/auth/refresh", post(refresh))
        .route("/api/libraries", get(libraries))
        .route("/api/libraries/{id}/items", get(items))
        .route("/api/libraries/{id}/search", get(search))
        .route("/api/items/{id}", get(item))
        .route("/api/items/{id}/cover", get(cover))
        .route("/api/items/{id}/play", post(play))
        .route("/s/item/{id}/{name}", get(file))
        .route("/api/session/{id}/{what}", post(session_report))
        .route("/api/me/progress/{id}", axum::routing::patch(progress))
        .route("/api/authors/{id}", get(author))
        .route("/api/me", get(me))
        .route("/api/libraries/{id}/personalized", get(personalized))
        .with_state(stand.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    tokio::spawn(axum::serve(listener, app).into_future());
    (format!("http://{addr}"), stand)
}
