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
async fn session_survives_server_restart() {
    let mut app = TestApp::new().await;

    // Register — session is now authenticated.
    let uid = app.register("frank", "hunter2hunter2").await;
    let me = app.get("/api/auth/me").await;
    assert_eq!(me.status, StatusCode::OK);
    assert_eq!(me.json["id"], uid);

    // Simulate a server restart: rebuild the app over the SAME on-disk stores,
    // keeping the client's cookie. The SQLite-backed session store must return
    // the still-valid session.
    app.restart().await;

    let me2 = app.get("/api/auth/me").await;
    assert_eq!(
        me2.status,
        StatusCode::OK,
        "session should survive restart: {}",
        me2.json
    );
    assert_eq!(me2.json["id"], uid, "same user after restart");

    // Negative control: dropping the cookie + restart is unauthenticated.
    app.logout_local();
    app.restart().await;
    let me3 = app.get("/api/auth/me").await;
    assert_eq!(me3.status, StatusCode::UNAUTHORIZED);
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

// ── Password reset ──────────────────────────────────────────────────────────
//
// In tests the app is built with `email: None` and compiled in debug, so
// `forgot-password` hands the raw token back in the JSON body (`reset_token`) —
// the dev fallback for when no mailer is configured. Production release builds
// never expose it.

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn forgot_password_unknown_email_is_ok_and_leaks_nothing() {
    let mut app = TestApp::new().await;
    // No account enumeration: unknown address still returns 200, with no token.
    let r = app
        .post("/api/auth/forgot-password", &json!({ "email": "ghost@example.com" }))
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json["ok"], true);
    assert!(r.json.get("reset_token").is_none(), "must not issue a token for an unknown email: {}", r.json);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reset_password_full_flow() {
    let mut app = TestApp::new().await;
    app.register("dave", "old-password-123").await;
    app.logout_local();

    // Request a reset link; the dev build returns the raw token.
    let fp = app
        .post("/api/auth/forgot-password", &json!({ "email": "dave@example.com" }))
        .await;
    assert_eq!(fp.status, StatusCode::OK);
    let token = fp.json["reset_token"]
        .as_str()
        .expect("dev build should return reset_token")
        .to_string();

    // Redeem it for a new password.
    let rp = app
        .post(
            "/api/auth/reset-password",
            &json!({ "token": token, "new_password": "brand-new-456" }),
        )
        .await;
    assert_eq!(rp.status, StatusCode::OK, "reset: {}", rp.json);

    // Old password no longer works; new one does.
    assert_eq!(app.login("dave", "old-password-123").await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(app.login("dave", "brand-new-456").await.status, StatusCode::OK);

    // The token is single-use — a second redemption is rejected.
    app.logout_local();
    let reuse = app
        .post(
            "/api/auth/reset-password",
            &json!({ "token": token, "new_password": "another-789" }),
        )
        .await;
    assert_eq!(reuse.status, StatusCode::BAD_REQUEST, "token reuse must fail: {}", reuse.json);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reset_password_bogus_token_rejected() {
    let mut app = TestApp::new().await;
    let r = app
        .post(
            "/api/auth/reset-password",
            &json!({ "token": "not-a-real-token", "new_password": "whatever-123" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reset_password_empty_password_rejected() {
    let mut app = TestApp::new().await;
    app.register("erin", "password-000").await;
    app.logout_local();
    let fp = app
        .post("/api/auth/forgot-password", &json!({ "email": "erin@example.com" }))
        .await;
    let token = fp.json["reset_token"].as_str().unwrap().to_string();

    let r = app
        .post(
            "/api/auth/reset-password",
            &json!({ "token": token, "new_password": "" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
}
