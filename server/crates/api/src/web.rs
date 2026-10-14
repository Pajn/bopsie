//! The web UI, served at `/` from what the router has no route for. It is a single-page app:
//! any page's URL gets its `index.html`, and its own router takes it from there. Paths under
//! the API's prefixes are never pages, so an unknown command URL stays a 404.

use std::borrow::Cow;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

use crate::Api;

/// The app as `expo export -p web` builds it. Without a build, the server builds anyway and
/// says so in place of the UI. Debug builds read the folder from disk on each request.
#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../../web/dist"]
#[allow_missing = true]
struct Embedded;

/// Prefixes the API owns; nothing under them is a page.
const RESERVED: &[&str] = &[
    "/api",
    "/api-docs",
    "/artwork",
    "/auth",
    "/imageproxy",
    "/info",
    "/linkplay",
    "/sendspin",
    "/setup",
    "/ws",
];

/// Where the files come from.
#[derive(Debug, Clone, Default)]
pub enum Web {
    /// Built into the server.
    #[default]
    Embedded,
    /// A directory on disk, from the config file.
    Dir(PathBuf),
}

struct File {
    data: Cow<'static, [u8]>,
    mime: String,
}

impl Web {
    fn get(&self, path: &str) -> Option<File> {
        match self {
            Web::Embedded => Embedded::get(path).map(|file| File {
                mime: file.metadata.mimetype().to_owned(),
                data: file.data,
            }),
            Web::Dir(dir) => {
                let relative = Path::new(path);
                // Only plain names: nothing that climbs out of the directory.
                if !relative
                    .components()
                    .all(|c| matches!(c, Component::Normal(_)))
                {
                    return None;
                }
                let data = std::fs::read(dir.join(relative)).ok()?;
                Some(File {
                    data: Cow::Owned(data),
                    mime: mime_guess::from_path(relative)
                        .first_or_octet_stream()
                        .to_string(),
                })
            }
        }
    }
}

fn reserved(path: &str) -> bool {
    RESERVED.iter().any(|prefix| {
        path.strip_prefix(prefix)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    })
}

/// File names that carry a hash of their contents, which never change under the same name.
fn immutable(path: &str) -> bool {
    path.starts_with("_expo/static/") || path.starts_with("assets/")
}

fn wants_html(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|accept| accept.to_str().ok())
        .is_some_and(|accept| accept.contains("text/html"))
}

fn not_found() -> Response {
    (StatusCode::NOT_FOUND, "Not Found").into_response()
}

fn serve(file: File, cache: &'static str, method: &Method) -> Response {
    let headers = [
        (header::CONTENT_TYPE, file.mime),
        (header::CACHE_CONTROL, cache.to_owned()),
    ];
    if method == Method::HEAD {
        return (headers, ()).into_response();
    }
    (headers, file.data.into_owned()).into_response()
}

const NOT_BUILT: &str = "<!doctype html><title>Bopsie</title>\
<p>This server was built without its web UI. Build it with <code>pnpm --filter bopsie-web \
export</code> before the server, or name a build with <code>dir</code> under <code>[web]</code> \
in the config file.</p>";

pub async fn fallback(
    State(api): State<Arc<Api>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Response {
    let path = uri.path();
    if reserved(path) {
        return not_found();
    }
    if method != Method::GET && method != Method::HEAD {
        let mut response = (StatusCode::METHOD_NOT_ALLOWED, "Method Not Allowed").into_response();
        response
            .headers_mut()
            .insert(header::ALLOW, HeaderValue::from_static("GET, HEAD"));
        return response;
    }
    let name = path.trim_start_matches('/');
    if !name.is_empty()
        && let Some(file) = api.web.get(name)
    {
        let cache = if immutable(name) {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache"
        };
        return serve(file, cache, &method);
    }
    if !wants_html(&headers) {
        return not_found();
    }
    match api.web.get("index.html") {
        Some(index) => serve(index, "no-cache", &method),
        None => (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            NOT_BUILT,
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserved_prefixes_are_whole_segments() {
        for path in [
            "/api",
            "/api/x",
            "/ws",
            "/ws/x",
            "/api-docs/commands.json",
            "/linkplay/a",
        ] {
            assert!(reserved(path), "{path}");
        }
        for path in ["/", "/apis", "/players", "/wsx", "/infos/1"] {
            assert!(!reserved(path), "{path}");
        }
    }

    #[test]
    fn a_directory_serves_only_what_is_inside_it() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("index.html"), "<html>").unwrap();
        let web = Web::Dir(dir.path().to_owned());
        let index = web.get("index.html").unwrap();
        assert_eq!(index.mime, "text/html");
        assert!(web.get("../secret.txt").is_none());
        assert!(web.get("/etc/passwd").is_none());
        assert!(web.get("missing.js").is_none());
    }
}
