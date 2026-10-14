//! Users and sign-ins through the API, as Music Assistant's `auth/user*` and `auth/tokens`.

mod common;

use std::time::Duration;

use serde_json::{Value, json};

use common::*;

async fn signed_in(server: &Server, token: &str) -> Client {
    let mut client = server.connect().await;
    let answer = client.send("auth", json!({"token": token})).await;
    assert!(answer.get("error_code").is_none(), "{answer}");
    client
}

/// Signs `username` in, returning a long-lived token named `device`.
async fn sign_in(server: &Server, username: &str, password: &str, device: &str) -> String {
    let mut anonymous = server.connect().await;
    let login = anonymous
        .result(
            "auth/login",
            json!({"username": username, "password": password}),
        )
        .await;
    assert_eq!(login["success"], true, "{login}");
    let mut session = signed_in(server, login["access_token"].as_str().unwrap()).await;
    session
        .result("auth/token/create", json!({"name": device}))
        .await
        .as_str()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn an_admin_manages_users_and_a_user_only_themselves() {
    let (server, token) = start_set_up().await;
    let mut admin = signed_in(&server, &token).await;

    let bob = admin
        .result(
            "auth/user/create",
            json!({"username": "bob", "password": "battery staple", "display_name": "Bob"}),
        )
        .await;
    assert_eq!(bob["role"], "user");
    let bob_id = bob["user_id"].as_str().unwrap().to_owned();
    let users = admin.result("auth/users", json!({})).await;
    assert_eq!(users.as_array().unwrap().len(), 2);

    let bob_token = sign_in(&server, "bob", "battery staple", "Bopsie on Android").await;
    let mut bob = signed_in(&server, &bob_token).await;
    // A user cannot list or create users, nor read the admin views.
    assert_eq!(bob.error_code("auth/users", json!({})).await, 22);
    assert_eq!(bob.error_code("admin/devices", json!({})).await, 22);
    // They may change their own profile, but not their role.
    let updated = bob
        .result("auth/user/update", json!({"display_name": "Robert"}))
        .await;
    assert_eq!(updated["display_name"], "Robert");
    assert_eq!(
        bob.error_code("auth/user/update", json!({"role": "admin"}))
            .await,
        22
    );
    // They see their own sign-ins, and not another's.
    let own = bob.result("auth/tokens", json!({})).await;
    assert!(
        own.as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "Bopsie on Android" && t["is_long_lived"] == true),
        "{own}"
    );
    let admin_id = admin.result("auth/me", json!({})).await["user_id"].clone();
    assert_eq!(
        bob.result("auth/tokens", json!({"user_id": admin_id}))
            .await,
        json!([])
    );

    // The admin sees Bob's sign-ins, and revoking one closes Bob's connection.
    let tokens = admin
        .result("auth/tokens", json!({"user_id": bob_id}))
        .await;
    let android = tokens
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "Bopsie on Android")
        .unwrap()["token_id"]
        .clone();
    admin
        .result("auth/token/revoke", json!({"token_id": android}))
        .await;
    assert_closed(&mut bob).await;

    // Disabling Bob signs him out everywhere and stops him signing in.
    let bob_token = sign_in(&server, "bob", "battery staple", "Bopsie on iPhone").await;
    let mut bob = signed_in(&server, &bob_token).await;
    admin
        .result("auth/user/disable", json!({"user_id": bob_id}))
        .await;
    assert_closed(&mut bob).await;
    let mut anonymous = server.connect().await;
    let login = anonymous
        .result(
            "auth/login",
            json!({"username": "bob", "password": "battery staple"}),
        )
        .await;
    assert_eq!(login["success"], false);
    admin
        .result("auth/user/enable", json!({"user_id": bob_id}))
        .await;

    // An admin cannot delete or disable themselves; Bob can be deleted.
    assert_ne!(
        admin
            .error_code("auth/user/delete", json!({"user_id": admin_id}))
            .await,
        0
    );
    admin
        .result("auth/user/delete", json!({"user_id": bob_id}))
        .await;
    assert_eq!(
        admin.result("auth/user", json!({"user_id": bob_id})).await,
        Value::Null
    );
}

/// The server closes the connection soon.
async fn assert_closed(client: &mut Client) {
    let closed = tokio::time::timeout(Duration::from_secs(5), async {
        while client.receive().await.is_some() {}
    })
    .await;
    assert!(closed.is_ok(), "the connection stayed open");
}
