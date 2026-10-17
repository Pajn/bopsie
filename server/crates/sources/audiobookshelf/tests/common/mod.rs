//! A stand-in Audiobookshelf server: one library of three books by two authors, answering as
//! the real one does the requests the source makes. It takes `reader`'s password `secret`,
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
    log(
        &stand,
        format!("GET /api/libraries/{library}/items?limit={limit}&page={page}"),
    );
    if !authorized(&stand, &headers) {
        return unauthorized();
    }
    assert_eq!(library, "lib1", "only the books' library is listed");
    let all = books();
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
            .filter(|b| b["media"]["metadata"]["authors"][0]["id"] == id)
            .collect();
        author["libraryItems"] = json!(theirs);
    }
    Json(author).into_response()
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
        .route("/api/authors/{id}", get(author))
        .with_state(stand.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    tokio::spawn(axum::serve(listener, app).into_future());
    (format!("http://{addr}"), stand)
}
