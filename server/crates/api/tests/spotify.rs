//! The Spotify account from the web UI: its developer app, logging in, and what a server with
//! Spotify turned off says.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use bopsie_api::router;
use serde_json::json;
use tower::ServiceExt;

use common::*;

/// Where `/spotify/callback` sends the browser for `query`.
async fn callback(server: &Server, query: &str) -> String {
    let response = router(server.api.clone())
        .oneshot(
            Request::get(format!("/spotify/callback?{query}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    response.headers()[header::LOCATION]
        .to_str()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn a_server_without_spotify_says_so() {
    let (server, token) = start_set_up().await;
    let mut client = server.connect().await;
    client.result("auth", json!({"token": token})).await;
    let status = client.result("admin/spotify/status", json!({})).await;
    assert_eq!(status["enabled"], false);
    assert_eq!(
        client
            .error_code("admin/spotify/pair_start", json!({}))
            .await,
        1
    );
    assert!(
        callback(&server, "code=x&state=y")
            .await
            .contains("spotify_error=")
    );
}

#[tokio::test]
async fn logging_in_needs_a_developer_app_and_comes_back_to_the_server_page() {
    let (server, token) = start_set_up_with_spotify().await;
    let mut client = server.connect().await;
    client.result("auth", json!({"token": token})).await;
    let status = client.result("admin/spotify/status", json!({})).await;
    assert_eq!(status["enabled"], true);
    assert_eq!(status["client_id"], json!(null));
    assert_eq!(status["sign_in_name"], "Bopsie sign-in");

    let redirect = json!({"redirect_uri": "https://bopsie.example/spotify/callback"});
    // No developer app yet: authentication is required.
    assert_eq!(
        client
            .error_code("admin/spotify/login", redirect.clone())
            .await,
        20
    );
    client
        .result(
            "admin/spotify/set_client_id",
            json!({"client_id": "my-app"}),
        )
        .await;
    let login = client.result("admin/spotify/login", redirect).await;
    let url = login["url"].as_str().unwrap();
    assert!(url.contains("client_id=my-app"), "{url}");
    // Anywhere but this server's callback is refused.
    assert_eq!(
        client
            .error_code(
                "admin/spotify/login",
                json!({"redirect_uri": "https://elsewhere.example/"})
            )
            .await,
        3
    );

    // An answer the server did not ask for, or a refusal, comes back as an error.
    let back = callback(&server, "code=x&state=not-ours").await;
    assert!(back.starts_with("/server?spotify_error="), "{back}");
    let back = callback(&server, "error=access_denied").await;
    assert_eq!(back, "/server?spotify_error=access_denied");
    let status = client.result("admin/spotify/status", json!({})).await;
    assert_eq!(status["client_id"], "my-app");
    assert_eq!(status["web_user"], json!(null));
}
