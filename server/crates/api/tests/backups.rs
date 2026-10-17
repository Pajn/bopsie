//! Backups from the API: made, listed, downloaded, scheduled, deleted, and restored from an
//! upload, which asks the server to restart.

mod common;

use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use bopsie_api::router;
use serde_json::json;
use tower::ServiceExt;

use common::*;

/// The response to `request`, with its body's bytes.
async fn send(server: &Server, request: Request<Body>) -> (StatusCode, Vec<u8>) {
    let response = router(server.api.clone()).oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, bytes.to_vec())
}

#[tokio::test]
async fn a_backup_is_made_downloaded_and_restored() {
    let (server, token) = start_set_up().await;
    let mut client = server.connect().await;
    client.result("auth", json!({"token": token})).await;

    let status = client.result("admin/backups/status", json!({})).await;
    assert_eq!(status["backups"], json!([]));
    assert_eq!(status["schedule"], json!(null), "off unless set");

    let made = client
        .result("admin/backups/create", json!({"passphrase": "hunter2"}))
        .await;
    let name = made["name"].as_str().unwrap().to_owned();
    assert!(name.ends_with(".tar.zst.age"), "{name}");
    assert_eq!(made["encrypted"], true);
    let status = client.result("admin/backups/status", json!({})).await;
    assert_eq!(status["backups"][0]["name"], name.as_str());

    // Downloaded by an admin only.
    let (refused, _) = send(
        &server,
        Request::get(format!("/backups/{name}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(refused, StatusCode::UNAUTHORIZED);
    let (ok, bytes) = send(
        &server,
        Request::get(format!("/backups/{name}"))
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(ok, StatusCode::OK);
    assert_eq!(
        bytes,
        std::fs::read(server.api.backups.dir().join(&name)).unwrap()
    );
    let (missing, _) = send(
        &server,
        Request::get("/backups/..%2Fbopsie.db")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(missing, StatusCode::NOT_FOUND);

    // Uploaded back: its passphrase is needed, then it is staged and the server restarts.
    let upload = |passphrase: &str| {
        Request::post("/backups/restore")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header("x-backup-passphrase", passphrase)
            .body(Body::from(bytes.clone()))
            .unwrap()
    };
    let (wrong, _) = send(&server, upload("nope")).await;
    assert_eq!(wrong, StatusCode::UNAUTHORIZED);
    let restart = {
        let backups = server.api.backups.clone();
        tokio::spawn(async move { backups.restart_requested().await })
    };
    let (accepted, _) = send(&server, upload("hunter2")).await;
    assert_eq!(accepted, StatusCode::ACCEPTED);
    tokio::time::timeout(Duration::from_secs(5), restart)
        .await
        .expect("the server is asked to restart")
        .unwrap();
    let data = server.api.backups.dir().parent().unwrap().to_owned();
    assert!(data.join("restore/READY").is_file());
    assert!(data.join("restore/bopsie.db").is_file());

    client
        .result("admin/backups/delete", json!({"name": name}))
        .await;
    let status = client.result("admin/backups/status", json!({})).await;
    assert_eq!(status["backups"], json!([]));
}

async fn status_has_run(client: &mut Client) -> bool {
    let status = client.result("admin/backups/status", json!({})).await;
    status["last_run"]["backup"].is_string()
}

#[tokio::test]
async fn backups_are_made_on_a_schedule() {
    let (server, token) = start_set_up().await;
    let mut client = server.connect().await;
    client.result("auth", json!({"token": token})).await;
    let refused = client
        .send(
            "admin/backups/schedule",
            json!({"every": "daily", "hour": 24}),
        )
        .await;
    assert!(refused["error_code"].is_number(), "{refused}");
    client
        .result(
            "admin/backups/schedule",
            json!({"every": "daily", "hour": 3, "keep": 1, "passphrase": "s3cret"}),
        )
        .await;
    let status = client.result("admin/backups/status", json!({})).await;
    assert_eq!(status["schedule"]["every"], "daily");
    assert_eq!(status["encrypted"], true);
    assert!(
        status.to_string().find("s3cret").is_none(),
        "the passphrase is not told"
    );

    // Set just now: no backup was due since.
    let backups = &server.api.backups;
    let now = time::OffsetDateTime::now_utc();
    assert!(backups.scheduled_once(now).await.unwrap().is_none());

    // Set days ago: one was due, and none made since, as when the server was down then.
    let mut schedule = backups.schedule().await.unwrap().unwrap();
    schedule.since = (now - time::Duration::days(3)).unix_timestamp();
    backups.set_schedule(Some(schedule), None).await.unwrap();
    std::fs::create_dir_all(backups.dir()).unwrap();
    let old = "bopsie-scheduled-20200101-030000.tar.zst";
    std::fs::write(backups.dir().join(old), b"old").unwrap();
    let made = backups.scheduled_once(now).await.unwrap().expect("made");
    assert!(made.scheduled && made.encrypted);
    // Once, however often looked at; and the older one is pruned, keeping one.
    assert!(backups.scheduled_once(now).await.unwrap().is_none());
    let names: Vec<String> = backups
        .list()
        .unwrap()
        .into_iter()
        .map(|b| b.name)
        .collect();
    assert_eq!(names, [made.name]);
    assert!(status_has_run(&mut client).await);

    client.result("admin/backups/schedule", json!({})).await;
    let status = client.result("admin/backups/status", json!({})).await;
    assert_eq!(status["schedule"], json!(null));
}
