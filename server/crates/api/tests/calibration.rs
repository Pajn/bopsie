//! Calibrating a group's sync through the API: the group pauses, each speaker is given its
//! chirps on the group's timeline, and what a phone heard sets each one's `sync_adjust_ms`.

mod common;

use std::time::Duration;

use serde_json::{Value, json};

use common::speaker::*;
use common::*;

async fn signed_in(server: &Server, token: &str) -> Client {
    let mut client = server.connect().await;
    client.result("auth", json!({"token": token})).await;
    client
}

async fn eventually(mut holds: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    while !holds() {
        assert!(tokio::time::Instant::now() < deadline, "never held");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn a_group_is_calibrated_from_what_a_phone_heard() {
    let (server, token) = start_set_up_with(10).await;
    let kitchen = Speaker::register(&server.players, "kitchen", "airplay", &[CD], None);
    let phone = Speaker::register(&server.players, "phone", "sendspin", &[CD], None);
    let mut client = signed_in(&server, &token).await;

    // Alone, there is nothing to put in step.
    let alone = client
        .send("admin/calibration/start", json!({"player_id": "kitchen"}))
        .await;
    assert_eq!(alone["error_code"], 9, "{alone}");

    client
        .result(
            "players/cmd/group",
            json!({"player_id": "phone", "target_player": "kitchen"}),
        )
        .await;
    let before = (kitchen.frames(), phone.frames());
    let calibration = client
        .result("admin/calibration/start", json!({"player_id": "phone"}))
        .await;
    assert_eq!(calibration["chirp"]["duration_ms"], 150);
    let chirps = calibration["chirps"].as_array().unwrap();
    let order: Vec<&str> = chirps
        .iter()
        .map(|c| c["endpoint_id"].as_str().unwrap())
        .collect();
    assert_eq!(
        order,
        ["kitchen", "phone", "kitchen", "phone", "kitchen", "phone"]
    );
    let times: Vec<i64> = chirps
        .iter()
        .map(|c| c["at_us"].as_i64().unwrap())
        .collect();
    assert!(
        times.windows(2).all(|w| w[1] - w[0] == 1_500_000),
        "{times:?}"
    );
    assert_eq!(calibration["ends_at_us"], times[0] + 9_000_000);
    assert_eq!(chirps[0]["name"], "Kitchen");
    let now = calibration["now_us"].as_i64().unwrap();
    assert!(
        now < times[0] && times[0] - now < 5_000_000,
        "{now} then {}",
        times[0]
    );

    let again = client
        .send("admin/calibration/start", json!({"player_id": "kitchen"}))
        .await;
    assert_eq!(again["error_code"], 9, "one at a time: {again}");

    // Each speaker is given the whole calibration: nine seconds of audio.
    eventually(|| {
        kitchen.frames() >= before.0 + 9 * 44_100 && phone.frames() >= before.1 + 9 * 44_100
    })
    .await;

    // The phone heard the speaker 24.6 ms ahead of the kitchen: it waits that long.
    let applied = client
        .result(
            "admin/calibration/apply",
            json!({"delays_ms": {"kitchen": 40.0, "phone": 15.4}}),
        )
        .await;
    assert_eq!(
        applied,
        json!([
            {"endpoint_id": "kitchen", "previous_ms": 0, "sync_adjust_ms": 0},
            {"endpoint_id": "phone", "previous_ms": 0, "sync_adjust_ms": 25},
        ])
    );
    // Heard in step after that, nothing moves.
    let applied = client
        .result(
            "admin/calibration/apply",
            json!({"delays_ms": {"kitchen": 40.0, "phone": 40.3}}),
        )
        .await;
    assert_eq!(applied[1]["previous_ms"], 25);
    assert_eq!(applied[1]["sync_adjust_ms"], 25);

    let refused: Value = client
        .send(
            "admin/calibration/apply",
            json!({"delays_ms": {"kitchen": 40.0}}),
        )
        .await;
    assert!(refused["error_code"].is_number(), "{refused}");
}
