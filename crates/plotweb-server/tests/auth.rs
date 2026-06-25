mod common;

use axum::http::StatusCode;
use common::TestApp;
use serde_json::json;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn health_ok() {
    let mut app = TestApp::new().await;
    let r = app.get("/health").await;
    assert_eq!(r.status, StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn register_login_me_logout_flow() {
    let mut app = TestApp::new().await;

    // Unauthenticated /me is rejected.
    let r = app.get("/api/auth/me").await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);

    // Register logs the session in.
    app.register("alice", "hunter2hunter2").await;
    let me = app.get("/api/auth/me").await;
    assert_eq!(me.status, StatusCode::OK);
    assert_eq!(me.json["username"], "alice");

    // Logout clears the session.
    let lo = app.post("/api/auth/logout", &json!({})).await;
    assert_eq!(lo.status, StatusCode::OK);
    app.logout_local();
    let me2 = app.get("/api/auth/me").await;
    assert_eq!(me2.status, StatusCode::UNAUTHORIZED);

    // Login with the right password re-authenticates.
    let li = app.login("alice", "hunter2hunter2").await;
    assert_eq!(li.status, StatusCode::OK);
    let me3 = app.get("/api/auth/me").await;
    assert_eq!(me3.status, StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn register_validates_required_fields() {
    let mut app = TestApp::new().await;
    let r = app
        .post(
            "/api/auth/register",
            &json!({ "username": "", "email": "x@y.com", "password": "abc" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn duplicate_register_conflicts() {
    let mut app = TestApp::new().await;
    app.register("bob", "password123").await;
    app.logout_local();
    let dup = app
        .post(
            "/api/auth/register",
            &json!({ "username": "bob", "email": "bob@example.com", "password": "password123" }),
        )
        .await;
    assert_eq!(dup.status, StatusCode::CONFLICT);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn login_wrong_password_rejected() {
    let mut app = TestApp::new().await;
    app.register("carol", "correct-horse").await;
    app.logout_local();
    let r = app.login("carol", "wrong-password").await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn login_unknown_user_rejected() {
    // Exercises the timing-equalization branch (dummy argon2 verify) added in the
    // audit fix — it must still return 401, not error.
    let mut app = TestApp::new().await;
    let r = app.login("nobody", "whatever123").await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
}
