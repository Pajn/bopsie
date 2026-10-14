//! The web UI at `/`: its files, the page any app URL gets, and what stays the API's.

mod common;

use std::path::Path;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use bopsie_api::{Web, router};
use tower::ServiceExt;

const INDEX: &str = "<!doctype html><title>Bopsie</title>";
const SCRIPT: &str = "_expo/static/js/web/index-0123abcd.js";

/// A build as `expo export -p web` lays it out.
fn build(dir: &Path) {
    std::fs::create_dir_all(dir.join("_expo/static/js/web")).unwrap();
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::write(dir.join("index.html"), INDEX).unwrap();
    std::fs::write(dir.join(SCRIPT), "console.log(1)").unwrap();
    std::fs::write(dir.join("favicon.ico"), [0u8; 4]).unwrap();
}

struct Answer {
    status: StatusCode,
    content_type: String,
    cache: String,
    body: String,
}

async fn send(server: &common::Server, method: Method, path: &str, accept: &str) -> Answer {
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::ACCEPT, accept)
        .body(Body::empty())
        .unwrap();
    let response = router(server.api.clone()).oneshot(request).await.unwrap();
    let header = |name| {
        response
            .headers()
            .get(name)
            .map(|v: &header::HeaderValue| v.to_str().unwrap().to_owned())
            .unwrap_or_default()
    };
    let (content_type, cache) = (header(header::CONTENT_TYPE), header(header::CACHE_CONTROL));
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    Answer {
        status,
        content_type,
        cache,
        body: String::from_utf8_lossy(&body).into_owned(),
    }
}

const HTML: &str = "text/html,application/xhtml+xml,*/*;q=0.8";

#[tokio::test]
async fn pages_get_the_app_and_files_are_served_with_their_type() {
    let dir = tempfile::tempdir().unwrap();
    build(dir.path());
    let server = common::start_serving(Web::Dir(dir.path().to_owned())).await;

    for path in ["/", "/players", "/players/ap1/detail?tab=raw"] {
        let page = send(&server, Method::GET, path, HTML).await;
        assert_eq!(page.status, StatusCode::OK, "{path}");
        assert_eq!(page.body, INDEX, "{path}");
        assert_eq!(page.cache, "no-cache", "{path}");
    }

    let script = send(&server, Method::GET, &format!("/{SCRIPT}"), "*/*").await;
    assert_eq!(script.status, StatusCode::OK);
    assert_eq!(script.content_type, "text/javascript");
    assert_eq!(script.cache, "public, max-age=31536000, immutable");
    assert_eq!(script.body, "console.log(1)");

    let icon = send(&server, Method::GET, "/favicon.ico", "*/*").await;
    assert_eq!(icon.status, StatusCode::OK);
    assert_eq!(icon.cache, "no-cache", "its name carries no hash");

    let head = send(&server, Method::HEAD, "/players", HTML).await;
    assert_eq!(head.status, StatusCode::OK);
    assert!(head.body.is_empty());
}

#[tokio::test]
async fn what_is_not_a_page_stays_not_found() {
    let dir = tempfile::tempdir().unwrap();
    build(dir.path());
    let server = common::start_serving(Web::Dir(dir.path().to_owned())).await;

    // The API's own prefixes, even from a browser.
    for path in [
        "/api/nope",
        "/ws/x",
        "/auth/other",
        "/linkplay/stream/x",
        "/api-docs/x",
    ] {
        let answer = send(&server, Method::GET, path, HTML).await;
        assert_eq!(answer.status, StatusCode::NOT_FOUND, "{path}");
        assert_ne!(answer.body, INDEX, "{path}");
    }
    // A missing file a script asked for.
    let missing = send(&server, Method::GET, "/missing.js", "*/*").await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    // Nothing outside the build.
    let outside = send(&server, Method::GET, "/../Cargo.toml", "*/*").await;
    assert_eq!(outside.status, StatusCode::NOT_FOUND);
    // Only reading.
    let post = send(&server, Method::POST, "/players", HTML).await;
    assert_eq!(post.status, StatusCode::METHOD_NOT_ALLOWED);
    // The API still answers where it did.
    let (status, info) = server.get("/info").await;
    assert_eq!(status, StatusCode::OK);
    assert!(info.get("server_id").is_some());
}

#[tokio::test]
async fn a_server_without_the_ui_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let server = common::start_serving(Web::Dir(dir.path().to_owned())).await;
    let page = send(&server, Method::GET, "/", HTML).await;
    assert_eq!(page.status, StatusCode::NOT_FOUND);
    assert!(page.content_type.starts_with("text/html"));
    assert!(page.body.contains("without its web UI"), "{}", page.body);
}
